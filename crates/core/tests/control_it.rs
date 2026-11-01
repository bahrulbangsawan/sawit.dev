//! Real lifecycle tests against installed PostgreSQL binaries. Skipped (with a
//! message) when no binaries are found.

use std::path::PathBuf;

use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::control::{self, Preflight, StartOutcome};
use sawit_core::instances::{AppPaths, Instance, ListenScope};
use sawit_core::pgctl::PgCtlStatus;
use sawit_core::ports::PortVerdict;

struct Fixture {
    _dir: tempfile::TempDir,
    paths: AppPaths,
    inst: Instance,
}

async fn fixture() -> Option<Fixture> {
    let roots = SearchRoots::detect(PathBuf::from("/nonexistent")).await;
    let Some(bins) = discover(&roots).await.into_iter().next() else {
        eprintln!("skip: no PostgreSQL binaries found");
        return None;
    };
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: dir.path().join("sawit dev it"),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let inst = Instance {
        name: "it".into(),
        bin_dir: bins.bin_dir,
        data_dir: paths.cluster_dir("it"),
        port,
        listen: ListenScope::Localhost,
        managed: true,
    };
    Some(Fixture {
        _dir: dir,
        paths,
        inst,
    })
}

#[tokio::test]
async fn lifecycle_init_start_query_stop() {
    let Some(f) = fixture().await else { return };
    control::init_cluster(&f.inst).await.unwrap();
    assert_eq!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Stopped
    );
    let outcome = control::start(&f.inst, &f.paths, vec![], Preflight::Check)
        .await
        .unwrap();
    assert!(matches!(outcome, StartOutcome::Started), "{outcome:?}");
    assert!(matches!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Running { .. }
    ));

    let url = format!(
        "host=localhost port={} user=postgres dbname=postgres",
        f.inst.port
    );
    let (client, conn) = tokio_postgres::connect(&url, tokio_postgres::NoTls)
        .await
        .unwrap();
    tokio::spawn(conn);
    let one: i32 = client.query_one("SELECT 1", &[]).await.unwrap().get(0);
    assert_eq!(one, 1);

    control::stop(&f.inst).await.unwrap();
    assert_eq!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Stopped
    );
}

#[tokio::test]
async fn start_when_running_reports_already_running() {
    let Some(f) = fixture().await else { return };
    control::init_cluster(&f.inst).await.unwrap();
    control::start(&f.inst, &f.paths, vec![], Preflight::Check)
        .await
        .unwrap();
    let again = control::start(&f.inst, &f.paths, vec![], Preflight::Check)
        .await
        .unwrap();
    assert!(
        matches!(again, StartOutcome::AlreadyRunning { .. }),
        "{again:?}"
    );
    let restarted = control::restart(&f.inst, &f.paths, vec![], Preflight::Check)
        .await
        .unwrap();
    assert!(matches!(restarted, StartOutcome::Started), "{restarted:?}");
    control::stop(&f.inst).await.unwrap();
}

#[tokio::test]
async fn start_blocked_by_foreign_listener() {
    let Some(f) = fixture().await else { return };
    control::init_cluster(&f.inst).await.unwrap();
    let _squatter = std::net::TcpListener::bind(("127.0.0.1", f.inst.port)).unwrap();
    let outcome = control::start(&f.inst, &f.paths, vec![], Preflight::Check)
        .await
        .unwrap();
    match outcome {
        StartOutcome::Blocked(report) => {
            assert!(matches!(report.verdict, PortVerdict::Conflict { .. }))
        }
        other => panic!("expected Blocked, got {other:?}"),
    }
    // Forcing must not panic; the result depends on the OS.
    let _ = control::start(&f.inst, &f.paths, vec![], Preflight::Force).await;
    let _ = control::stop(&f.inst).await;
}

/// `sawit nyalakan` starts instances as the Start button does: a port another process holds is
/// explained and the start held (exit 1, still stopped); once free, it starts; started, it is a
/// note; `--ulang -y` restarts it; and a name that isn't an instance's is refused.
#[tokio::test]
async fn nyalakan_starts_held_and_running_instances() {
    use sawit_core::instances::Registry;
    use sawit_core::kebun::{NyalakanArgs, nyalakan};

    /// Stops the instance however the test ends.
    struct Stop(Instance);
    impl Drop for Stop {
        fn drop(&mut self) {
            let inst = self.0.clone();
            let _ = std::thread::spawn(move || {
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(control::stop(&inst))
            })
            .join();
        }
    }

    let Some(f) = fixture().await else { return };
    control::init_cluster(&f.inst).await.unwrap();
    let _stop = Stop(f.inst.clone());
    let mut registry = Registry::default();
    registry.add(f.inst.clone()).unwrap();
    registry.save(&f.paths.instances_file()).unwrap();
    let every = NyalakanArgs::default();

    let squatter = std::net::TcpListener::bind(("127.0.0.1", f.inst.port)).unwrap();
    assert_eq!(nyalakan(every.clone(), &f.paths).await, 1);
    assert_eq!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Stopped
    );
    drop(squatter);

    assert_eq!(nyalakan(every.clone(), &f.paths).await, 0);
    let Ok(PgCtlStatus::Running { pid }) = control::status(&f.inst).await else {
        panic!("not started");
    };
    // Already running: a note, and nothing changes.
    assert_eq!(nyalakan(every, &f.paths).await, 0);
    assert_eq!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Running { pid }
    );
    let again = NyalakanArgs {
        names: vec!["it".into()],
        again: true,
        yes: true,
    };
    assert_eq!(nyalakan(again, &f.paths).await, 0);
    assert!(matches!(
        control::status(&f.inst).await.unwrap(),
        PgCtlStatus::Running { pid: new } if new != pid
    ));
    let unknown = NyalakanArgs {
        names: vec!["nope".into()],
        ..NyalakanArgs::default()
    };
    assert_eq!(nyalakan(unknown, &f.paths).await, 1);
}
