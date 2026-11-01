//! Instances' start messages, the rules for removing one, the binaries a version picks, and
//! the end of a log.

use super::*;
use sawit_core::binaries::{BinarySource, PgVersion};
use std::path::PathBuf;

fn inst(managed: bool) -> Instance {
    Instance {
        name: "shop".into(),
        bin_dir: PathBuf::from("/bin"),
        data_dir: PathBuf::from("/data/shop"),
        port: 5432,
        listen: ListenScope::Localhost,
        managed,
    }
}

/// The app offers Start Anyway only when the port is held on another
/// address: a start that would fail sends the agent to stop what holds
/// the port, and only the other kind mentions starting anyway.
#[test]
fn a_held_start_says_what_can_be_done() {
    use sawit_core::ports::{BindAddr, Listener};
    use sawit_core::probe::PortReport;
    let listener = |addr| Listener {
        pid: 42,
        command: "postgres".into(),
        addr,
        port: 5432,
    };
    let said = |verdict| {
        let report = PortReport {
            port: 5432,
            intended: vec![BindAddr::LoopbackV4],
            verdict,
            owners: vec![],
        };
        let r = Sawit::start_text("shop", &inst(true), Ok(StartOutcome::Blocked(report)));
        assert_eq!(r.is_error, Some(true));
        let texts = r.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<String>()
    };
    let conflict = said(PortVerdict::Conflict {
        same_addr: vec![listener(BindAddr::LoopbackV4)],
        shadowing: vec![],
    });
    for word in ["start would fail", "stop_processes", "stop_instance"] {
        assert!(conflict.contains(word), "{conflict}");
    }
    assert!(!conflict.contains("Start Anyway"), "{conflict}");
    let shadowing = said(PortVerdict::Shadowing {
        listeners: vec![listener(BindAddr::AnyV6)],
    });
    for word in ["Start Anyway", "app_instance", "stop_processes"] {
        assert!(shadowing.contains(word), "{shadowing}");
    }
}

#[test]
fn removing_keeps_data_unless_confirmed() {
    assert!(check_remove(&inst(true), false, None).is_ok());
    assert!(check_remove(&inst(false), false, None).is_ok());
    assert!(check_remove(&inst(true), true, None).is_err());
    assert!(check_remove(&inst(true), true, Some("nope")).is_err());
    assert!(check_remove(&inst(true), true, Some("shop")).is_ok());
    let e = check_remove(&inst(false), true, Some("shop")).unwrap_err();
    assert!(e.contains("didn't create"), "{e}");
}

#[test]
fn binaries_by_version() {
    let b = |major| PgBinaries {
        bin_dir: PathBuf::from(format!("/pg{major}")),
        version: PgVersion { major, minor: 1 },
        source: BinarySource::Homebrew,
    };
    let installed = vec![b(17), b(16)];
    assert_eq!(pick_binaries(&installed, None).unwrap().version.major, 17);
    assert_eq!(
        pick_binaries(&installed, Some(16)).unwrap().version.major,
        16
    );
    assert!(pick_binaries(&installed, Some(18)).is_none());
    assert!(pick_binaries(&[], None).is_none());
}

#[test]
fn log_tail() {
    assert_eq!(last_lines("a\nb\nc\n", 2), "b\nc");
    assert_eq!(last_lines("a", 5), "a");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.log");
    std::fs::write(
        &path,
        (1..=10).map(|i| format!("line {i}\n")).collect::<String>(),
    )
    .unwrap();
    assert_eq!(tail_file(&path, 2).unwrap(), "line 9\nline 10");
}
