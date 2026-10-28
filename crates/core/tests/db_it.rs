//! Database features against a real throwaway cluster (skipped without
//! PostgreSQL binaries).

use std::path::PathBuf;

use pgdesk_core::binaries::{SearchRoots, discover};
use pgdesk_core::control::{self, Preflight};
use pgdesk_core::db::{self, ConnProfile, RelKind};
use pgdesk_core::instances::{AppPaths, Instance, ListenScope};

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

#[tokio::test]
async fn schema_paging_and_sql() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE SCHEMA app;
             CREATE TABLE app.users (id bigint PRIMARY KEY, email text NOT NULL, note text);
             INSERT INTO app.users SELECT g, 'user' || g || '@x.dev', CASE WHEN g % 2 = 0 THEN NULL ELSE 'n' END
               FROM generate_series(1, 250000) g;
             CREATE VIEW app.active AS SELECT id FROM app.users WHERE id < 10;
             ANALYZE app.users;",
        )
        .await
        .unwrap();

    // Schema tree.
    let schemas = db::load_schema(&client).await.unwrap();
    let app = schemas
        .iter()
        .find(|s| s.name == "app")
        .expect("app schema");
    let users = app.relations.iter().find(|r| r.name == "users").unwrap();
    assert_eq!(users.kind, RelKind::Table);
    let cols: Vec<_> = users
        .columns
        .iter()
        .map(|c| (c.name.as_str(), c.data_type.as_str(), c.nullable))
        .collect();
    assert_eq!(
        cols,
        vec![
            ("id", "bigint", false),
            ("email", "text", false),
            ("note", "text", true)
        ]
    );
    assert!(
        app.relations
            .iter()
            .any(|r| r.name == "active" && r.kind == RelKind::View)
    );
    assert!(
        !schemas
            .iter()
            .any(|s| s.name == "pg_catalog" || s.name == "information_schema")
    );

    // Count and pages.
    assert_eq!(
        db::row_count(&client, "app", "users").await.unwrap(),
        250_000
    );
    let page = db::fetch_page(&client, "app", "users", 200_000, 200)
        .await
        .unwrap();
    assert_eq!(page.columns, vec!["id", "email", "note"]);
    assert_eq!(page.rows.len(), 200);
    assert_eq!(page.rows[0][0].as_deref(), Some("200001"));
    assert_eq!(page.rows[0][2].as_deref(), Some("n")); // odd id
    assert_eq!(page.rows[1][2], None); // even id → NULL

    // Ad-hoc SQL: results, affected rows, errors.
    let out = db::run_sql(&client, "UPDATE app.users SET note = 'z' WHERE id <= 3; SELECT id, note FROM app.users WHERE id <= 3 ORDER BY id", 1000)
        .await
        .unwrap();
    assert_eq!(out.affected, vec![3]);
    let last = out.results.last().unwrap();
    assert_eq!(last.rows.len(), 3);
    assert_eq!(last.rows[2][1].as_deref(), Some("z"));
    let truncated = db::run_sql(&client, "SELECT * FROM app.users", 50)
        .await
        .unwrap();
    assert_eq!(truncated.results[0].rows.len(), 50);
    assert!(truncated.results[0].truncated);
    let err = db::run_sql(&client, "SELECT * FROM missing_table", 10)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing_table"), "{err}");

    // Read-only connections refuse writes.
    let ro = db::connect_read_only(&profile, None).await.unwrap();
    assert!(
        db::run_read_only(&ro, "SELECT count(*) FROM app.users", 10)
            .await
            .is_ok()
    );
    assert!(
        db::run_read_only(&ro, "DELETE FROM app.users", 10)
            .await
            .is_err()
    );
    assert!(
        db::run_read_only(&ro, "COMMIT; DELETE FROM app.users", 10)
            .await
            .is_err()
    );
    assert!(
        db::run_read_only(
            &ro,
            "SET default_transaction_read_only = off; DELETE FROM app.users",
            10
        )
        .await
        .is_err()
    );
    assert_eq!(
        db::row_count(&client, "app", "users").await.unwrap(),
        250_000
    );

    let dbs = db::list_databases(&client).await.unwrap();
    assert!(dbs.contains(&"postgres".to_string()));
}
