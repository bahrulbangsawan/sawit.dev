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
        db::RowCount {
            rows: 250_000,
            exact: true
        }
    );
    let page = db::fetch_page(&client, "app", "users", &[], 200_000, 200)
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
        db::row_count(&client, "app", "users").await.unwrap().rows,
        250_000
    );

    let dbs = db::list_databases(&client).await.unwrap();
    assert!(dbs.contains(&"postgres".to_string()));

    // Views get a bounded exact count (no unbounded count(*) on huge views).
    assert_eq!(
        db::row_count(&client, "app", "active").await.unwrap().rows,
        9
    );
}

#[tokio::test]
async fn pages_are_stable_on_tables_above_synchronized_scan_threshold() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    // ~70 MB: above shared_buffers/4, where seqscans synchronize.
    client
        .batch_execute(
            "CREATE TABLE big AS SELECT g AS id, repeat('x', 200) AS pad FROM generate_series(1, 300000) g;",
        )
        .await
        .unwrap();
    for (offset, first) in [
        (200_000u64, "200001"),
        (200_200, "200201"),
        (0, "1"),
        (150_000, "150001"),
    ] {
        let page = db::fetch_page(&client, "public", "big", &[], offset, 200)
            .await
            .unwrap();
        assert_eq!(page.rows[0][0].as_deref(), Some(first), "offset {offset}");
    }
}

#[tokio::test]
async fn read_only_cannot_be_escaped() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute("CREATE TABLE t (x int); INSERT INTO t VALUES (1);")
        .await
        .unwrap();
    db::ensure_read_only_role(&admin).await.unwrap();
    db::ensure_read_only_role(&admin).await.unwrap(); // idempotent

    let ro_profile = ConnProfile {
        user: db::READ_ONLY_ROLE.into(),
        ..profile.clone()
    };
    let ro = db::connect_read_only(&ro_profile, None).await.unwrap();
    let marker = std::env::temp_dir().join(format!("pgdesk-pwned-{}", std::process::id()));
    let attacks = [
        "COMMIT; BEGIN READ WRITE; CREATE TABLE pwned(x int); COMMIT;".to_string(),
        "CREATE TABLE pwned(x int)".to_string(),
        format!("COPY (SELECT 1) TO PROGRAM 'touch {}'", marker.display()),
        format!(
            "DO $$ BEGIN EXECUTE 'COPY (SELECT 1) TO PROGRAM ''touch {}'''; END $$",
            marker.display()
        ),
    ];
    for sql in &attacks {
        assert!(
            db::run_read_only(&ro, sql, 10).await.is_err(),
            "allowed: {sql}"
        );
    }
    // Saved profiles may use powerful users: the single-statement rule must
    // still stop COMMIT/BEGIN READ WRITE smuggling on a superuser connection.
    let su_ro = db::connect_read_only(&profile, None).await.unwrap();
    assert!(
        db::run_read_only(
            &su_ro,
            "COMMIT; BEGIN READ WRITE; CREATE TABLE pwned(x int); COMMIT;",
            10
        )
        .await
        .is_err()
    );
    assert!(!marker.exists(), "shell command ran");
    let tables = db::run_sql(
        &admin,
        "SELECT count(*) FROM pg_tables WHERE tablename = 'pwned'",
        1,
    )
    .await
    .unwrap();
    assert_eq!(tables.results[0].rows[0][0].as_deref(), Some("0"));
    // Reading still works for the read-only role.
    let read = db::run_read_only(&ro, "SELECT x FROM t", 10).await.unwrap();
    assert_eq!(read.results[0].rows[0][0].as_deref(), Some("1"));
}

#[tokio::test]
async fn table_pages_follow_the_primary_key() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            r#"CREATE TABLE "Orders" ("Region" text, id int, label text, PRIMARY KEY ("Region", id));
               INSERT INTO "Orders" SELECT CASE WHEN g % 2 = 0 THEN 'b' ELSE 'a' END, 200 - g, 'o' || g
                 FROM generate_series(1, 120) g;
               CREATE TABLE empty_t (x int, y text);
               CREATE VIEW v AS SELECT label FROM "Orders";"#,
        )
        .await
        .unwrap();

    let pk = db::primary_key(&client, "public", "Orders").await.unwrap();
    assert_eq!(pk, vec!["Region", "id"]);
    assert!(
        db::primary_key(&client, "public", "v")
            .await
            .unwrap()
            .is_empty()
    );

    // Pages of 50, ordered by the key: stable whatever the physical order.
    let p0 = db::fetch_page(&client, "public", "Orders", &pk, 0, 50)
        .await
        .unwrap();
    assert_eq!(p0.columns, vec!["Region", "id", "label"]);
    assert_eq!(p0.rows.len(), 50);
    assert_eq!(p0.rows[0][0].as_deref(), Some("a"));
    assert_eq!(p0.rows[0][1].as_deref(), Some("81")); // smallest odd g=119 → 200-119
    let p2 = db::fetch_page(&client, "public", "Orders", &pk, 100, 50)
        .await
        .unwrap();
    assert_eq!(p2.rows.len(), 20);
    assert_eq!(p2.rows[19][0].as_deref(), Some("b"));
    assert_eq!(p2.rows[19][1].as_deref(), Some("198"));

    // Empty tables still report their columns; views page without a key.
    let empty = db::fetch_page(&client, "public", "empty_t", &[], 0, 50)
        .await
        .unwrap();
    assert_eq!(empty.columns, vec!["x", "y"]);
    assert!(empty.rows.is_empty());
    assert_eq!(
        db::row_count(&client, "public", "empty_t").await.unwrap(),
        db::RowCount {
            rows: 0,
            exact: true
        }
    );
    let view = db::fetch_page(&client, "public", "v", &[], 0, 50)
        .await
        .unwrap();
    assert_eq!(view.rows.len(), 50);

    // A count that outlives the limit falls back to the estimate, and the
    // connection stays usable.
    client
        .batch_execute("CREATE VIEW slow AS SELECT pg_sleep(2)::text AS s")
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let slow = db::row_count_within(&client, "public", "slow", "200ms")
        .await
        .unwrap();
    assert!(!slow.exact);
    assert!(started.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(
        db::row_count(&client, "public", "empty_t")
            .await
            .unwrap()
            .rows,
        0
    );
}

#[tokio::test]
async fn tls_follows_sslmode() {
    let Some((cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let data = &cluster.inst.data_dir;
    let made = std::process::Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        ])
        .args(["-subj", "/CN=localhost", "-keyout"])
        .arg(data.join("server.key"))
        .arg("-out")
        .arg(data.join("server.crt"))
        .output();
    if !made.is_ok_and(|o| o.status.success()) {
        eprintln!("skip: openssl unavailable");
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        data.join("server.key"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute("ALTER SYSTEM SET ssl = on")
        .await
        .unwrap();
    admin
        .batch_execute("SELECT pg_reload_conf()")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    async fn uses_tls(profile: &ConnProfile) -> pgdesk_core::Result<bool> {
        let c = db::connect(profile, None).await?;
        let row = c
            .query_one(
                "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
                &[],
            )
            .await
            .unwrap();
        Ok(row.get(0))
    }
    let with = |ssl| ConnProfile {
        ssl,
        ..profile.clone()
    };
    assert!(!uses_tls(&with(db::SslMode::Disable)).await.unwrap());
    assert!(uses_tls(&with(db::SslMode::Prefer)).await.unwrap());
    assert!(uses_tls(&with(db::SslMode::Require)).await.unwrap());
    // Self-signed: encryption is fine, verification must fail.
    assert!(uses_tls(&with(db::SslMode::VerifyFull)).await.is_err());
    // Read-only connections honour it too.
    let ro = db::connect_read_only(&with(db::SslMode::Require), None)
        .await
        .unwrap();
    let row = ro
        .query_one(
            "SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()",
            &[],
        )
        .await
        .unwrap();
    assert!(row.get::<_, bool>(0));
}

#[tokio::test]
async fn schema_marks_relations_the_role_cannot_read() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE open_t (x int); CREATE TABLE secret_t (x int);
             CREATE ROLE limited LOGIN; GRANT SELECT ON open_t TO limited;",
        )
        .await
        .unwrap();
    let limited = ConnProfile {
        user: "limited".into(),
        ..profile.clone()
    };
    let client = db::connect(&limited, None).await.unwrap();
    let schemas = db::load_schema(&client).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let readable = |name: &str| {
        public
            .relations
            .iter()
            .find(|r| r.name == name)
            .unwrap()
            .readable
    };
    assert!(readable("open_t"));
    assert!(!readable("secret_t"));
}
