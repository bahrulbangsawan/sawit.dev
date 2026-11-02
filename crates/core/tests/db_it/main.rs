//! Database features against a real throwaway cluster (skipped without
//! PostgreSQL binaries).

mod changes;
mod connections;
mod pages;
mod read_only;
mod schema;

use std::path::PathBuf;

use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::control::{self, Preflight};
use sawit_core::db::ConnProfile;
use sawit_core::instances::{AppPaths, Instance, ListenScope};

struct Cluster {
    _dir: tempfile::TempDir,
    inst: Instance,
}

impl Drop for Cluster {
    fn drop(&mut self) {
        let inst = self.inst.clone();
        let _ = std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(control::stop(&inst))
        })
        .join();
    }
}

async fn cluster() -> Option<(Cluster, ConnProfile)> {
    let roots = SearchRoots::detect(PathBuf::from("/nonexistent")).await;
    let bins = discover(&roots).await.into_iter().next()?;
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: dir.path().join("db"),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let inst = Instance {
        name: "dbit".into(),
        bin_dir: bins.bin_dir,
        data_dir: paths.cluster_dir("dbit"),
        port,
        listen: ListenScope::Localhost,
        managed: true,
    };
    control::init_cluster(&inst).await.unwrap();
    control::start(&inst, &paths, vec![], Preflight::Force)
        .await
        .unwrap();
    let profile = ConnProfile::new("localhost", port, "postgres", "postgres");
    Some((Cluster { _dir: dir, inst }, profile))
}
