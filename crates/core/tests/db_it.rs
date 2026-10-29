//! Database features against a real throwaway cluster (skipped without
//! PostgreSQL binaries).

use std::path::PathBuf;

use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::control::{self, Preflight};
use sawit_core::db::{self, ConnProfile, RelKind};
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
    let marker = std::env::temp_dir().join(format!("sawit-pwned-{}", std::process::id()));
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

    async fn uses_tls(profile: &ConnProfile) -> sawit_core::Result<bool> {
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

#[tokio::test]
async fn running_queries_can_be_cancelled() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = std::sync::Arc::new(db::connect(&profile, None).await.unwrap());
    let token = client.cancel_token();
    let running = {
        let client = client.clone();
        tokio::spawn(async move { db::run_sql(&client, "SELECT pg_sleep(30)", 10).await })
    };
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let started = std::time::Instant::now();
    db::cancel(&token, &profile).await.unwrap();
    let err = running.await.unwrap().unwrap_err();
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(err.to_string().contains("canceled"), "{err}");
    // The connection survives a cancel.
    assert!(db::run_sql(&client, "SELECT 1", 1).await.is_ok());
}

/// Forwards to `upstream` but refuses the `options` startup parameter, as
/// PgBouncer (Neon's `-pooler` endpoints, Supabase) does. Returns its port.
async fn pooler_like_proxy(upstream: u16) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            tokio::spawn(async move {
                let packet = loop {
                    let len = client.read_i32().await.ok()? as usize;
                    let mut body = vec![0; len - 4];
                    client.read_exact(&mut body).await.ok()?;
                    if body[..4] == 80877103i32.to_be_bytes() {
                        client.write_all(b"N").await.ok()?; // no TLS
                        continue;
                    }
                    break [&(len as i32).to_be_bytes()[..], &body].concat();
                };
                if packet.windows(8).any(|w| w == b"options\0") {
                    let fields =
                        b"SFATAL\0VFATAL\0C08P01\0Munsupported startup parameter: options\0\0";
                    let mut err = vec![b'E'];
                    err.extend_from_slice(&((fields.len() + 4) as i32).to_be_bytes());
                    err.extend_from_slice(fields);
                    let _ = client.write_all(&err).await;
                    return None;
                }
                let mut server = tokio::net::TcpStream::connect(("127.0.0.1", upstream))
                    .await
                    .ok()?;
                server.write_all(&packet).await.ok()?;
                tokio::io::copy_bidirectional(&mut client, &mut server)
                    .await
                    .ok()
            });
        }
    });
    port
}

#[tokio::test]
async fn works_through_a_pooler_without_session_settings() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE heap_t (x int);
             INSERT INTO heap_t SELECT g FROM generate_series(1, 120) g;",
        )
        .await
        .unwrap();
    let pooled = ConnProfile {
        port: pooler_like_proxy(profile.port).await,
        ..profile.clone()
    };
    let (rw, ro, schemas) = db::connect_all(&pooled, None).await.unwrap();
    assert!(schemas.iter().any(|s| s.name == "public"));
    // Nothing was SET on the session: behind a real transaction pooler it
    // would leak into other clients' transactions.
    for client in [&rw, &ro] {
        let row = client
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .unwrap();
        assert_eq!(row.get::<_, String>(0), "off");
    }
    // Keyless pages and counts still work.
    let page = db::fetch_page(&ro, "public", "heap_t", &[], 100, 50)
        .await
        .unwrap();
    assert_eq!(page.rows.len(), 20);
    assert_eq!(page.rows[0][0].as_deref(), Some("101"));
    assert_eq!(
        db::row_count(&ro, "public", "heap_t").await.unwrap().rows,
        120
    );
}

#[tokio::test]
async fn schema_carries_keys_estimates_and_draws_as_a_diagram() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE users (id bigint PRIMARY KEY, email varchar(255) NOT NULL);
             CREATE TABLE line (order_id int, n int, user_id bigint REFERENCES users(id),
                                PRIMARY KEY (order_id, n));
             INSERT INTO users SELECT g, 'u' || g FROM generate_series(1, 500) g;
             ANALYZE users;",
        )
        .await
        .unwrap();
    // Keyword/value connection strings connect like URLs.
    let (kv, _) = db::parse_database_url(&format!(
        "host=localhost port={} dbname=postgres user=postgres sslmode=disable",
        profile.port
    ))
    .unwrap();
    let (_rw, _ro, schemas) = db::connect_all(&kv, None).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let rel = |n: &str| public.relations.iter().find(|r| r.name == n).unwrap();
    assert_eq!(rel("users").primary_key, vec!["id"]);
    assert_eq!(rel("users").estimated_rows, Some(500));
    assert_eq!(rel("line").primary_key, vec!["order_id", "n"]);
    assert_eq!(rel("line").estimated_rows, None); // never analyzed
    assert_eq!(
        rel("line").foreign_keys,
        vec![db::ForeignKey {
            columns: vec!["user_id".into()],
            ref_schema: "public".into(),
            ref_table: "users".into(),
            ref_columns: vec!["id".into()],
        }]
    );
    let dbml = db::diagram::to_dbml(&schemas, Some("public"));
    assert!(
        dbml.contains("Ref: public.line.user_id > public.users.id"),
        "{dbml}"
    );
    let er = db::diagram::to_mermaid(&schemas, Some("public"));
    let drawn = sawit_core::mermaid::render(&er, &Default::default()).unwrap();
    assert_eq!(drawn.kind, "er");
    assert!(drawn.svg.contains("public.users"));
}

#[tokio::test]
async fn connection_strings_failover_and_failures_against_a_real_server() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let dead = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port(); // nothing listens once the listener is dropped
    let port = profile.port;

    // A dead first host falls over to the second; `options` reach the server.
    let (multi, _) = db::parse_database_url(&format!(
        "postgres://postgres@127.0.0.1:{dead},localhost:{port}/postgres?options=-c%20search_path%3Dfrom_url&connect_timeout=3"
    ))
    .unwrap();
    let client = db::connect(&multi, None).await.unwrap();
    let row = client.query_one("SHOW search_path", &[]).await.unwrap();
    assert_eq!(row.get::<_, String>(0), "from_url");

    // Unix socket (the server's default directory), when it has one.
    if std::path::Path::new(&format!("/tmp/.s.PGSQL.{port}")).exists() {
        let (sock, _) = db::parse_database_url(&format!(
            "postgresql://postgres@%2Ftmp:{port}/postgres?sslmode=require"
        ))
        .unwrap();
        assert!(sock.is_socket());
        db::connect(&sock, None).await.expect("socket connection");
    }

    // Failures say what kind they are.
    let kind = |p: ConnProfile| async move {
        db::failure_kind(&db::connect(&p, None).await.expect_err("should fail"))
    };
    let refused = ConnProfile {
        port: dead,
        ..profile.clone()
    };
    assert_eq!(kind(refused).await, db::FailureKind::Network);
    let unknown_host = ConnProfile {
        host: "no-such-host.invalid".into(),
        ..profile.clone()
    };
    assert_eq!(kind(unknown_host).await, db::FailureKind::Network);
    let missing_db = ConnProfile {
        database: "no_such_db".into(),
        ..profile.clone()
    };
    assert_eq!(kind(missing_db).await, db::FailureKind::DatabaseMissing);
    let no_tls = ConnProfile {
        ssl: db::SslMode::Require,
        port: pooler_like_proxy(port).await, // answers "no TLS"
        ..profile.clone()
    };
    assert_eq!(kind(no_tls).await, db::FailureKind::Tls);

    // A session killed on the server (restart, idle reaper, failover) is
    // noticed, and a fresh connection works.
    let (rw, ro, _) = db::connect_all(&profile, None).await.unwrap();
    let pid: i32 = ro
        .query_one("SELECT pg_backend_pid()", &[])
        .await
        .unwrap()
        .get(0);
    rw.query_one("SELECT pg_terminate_backend($1)", &[&pid])
        .await
        .unwrap();
    let _ = ro.simple_query("SELECT 1").await; // fails: terminated
    for _ in 0..50 {
        if ro.is_closed() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ro.is_closed());
    assert!(!rw.is_closed());
    let again = db::connect_read_only(&profile, None).await.unwrap();
    again.simple_query("SELECT 1").await.unwrap();
}

#[tokio::test]
async fn schema_lists_indexes_constraints_and_matviews() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE TABLE teams (id int PRIMARY KEY);
             CREATE TABLE players (
               id int PRIMARY KEY,
               team int REFERENCES teams(id),
               email text CONSTRAINT players_email_key UNIQUE,
               age int CONSTRAINT adult CHECK (age >= 18));
             CREATE INDEX players_team_idx ON players USING hash (team);
             CREATE MATERIALIZED VIEW roster AS SELECT id, team FROM players;
             CREATE INDEX roster_team ON roster (team);",
        )
        .await
        .unwrap();
    let schemas = db::load_schema(&client).await.unwrap();
    let public = schemas.iter().find(|s| s.name == "public").unwrap();
    let rel = |name: &str| public.relations.iter().find(|r| r.name == name).unwrap();

    let players = rel("players");
    let kinds: Vec<_> = players
        .constraints
        .iter()
        .map(|c| (c.name.as_str(), c.kind))
        .filter(|(_, k)| *k != db::ConstraintKind::Other) // NOT NULL rows on PG 18
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("adult", db::ConstraintKind::Check),
            ("players_email_key", db::ConstraintKind::Unique),
            ("players_pkey", db::ConstraintKind::PrimaryKey),
            ("players_team_fkey", db::ConstraintKind::ForeignKey),
        ]
    );
    let check = players.constraints.iter().find(|c| c.name == "adult").unwrap();
    assert_eq!(check.definition, "CHECK ((age >= 18))");
    // The key and foreign key the diagram uses still load.
    assert_eq!(players.primary_key, vec!["id"]);
    assert_eq!(players.foreign_keys[0].ref_table, "teams");

    let indexes: Vec<_> = players
        .indexes
        .iter()
        .map(|i| (i.name.as_str(), i.method.as_str(), i.unique, i.primary))
        .collect();
    assert_eq!(
        indexes,
        vec![
            ("players_email_key", "btree", true, false),
            ("players_pkey", "btree", true, true),
            ("players_team_idx", "hash", false, false),
        ]
    );
    assert!(players.indexes[2].definition.contains("USING hash (team)"));

    let roster = rel("roster");
    assert_eq!(roster.kind, RelKind::MaterializedView);
    assert_eq!(roster.indexes.len(), 1);
    assert!(roster.constraints.is_empty());
}

#[tokio::test]
async fn rows_filter_search_and_sort_on_the_server() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            // A column named like a row alias must not break the row search.
            "CREATE TABLE people (id int PRIMARY KEY, name text, age int, t text);
             INSERT INTO people VALUES
               (1, 'Ana', 30, 'a'), (2, 'bob', NULL, 'b'), (3, 'Cara', 25, 'c'),
               (4, '100%_real', 40, 'd'), (5, 'Abe', 30, 'e');",
        )
        .await
        .unwrap();
    let pk = vec!["id".to_string()];
    let ids = |set: &db::ResultSet| -> Vec<String> {
        set.rows.iter().map(|r| r[0].clone().unwrap()).collect()
    };
    let run = |q: db::RowQuery| {
        let (client, pk) = (&client, pk.clone());
        async move {
            let set = db::fetch_rows(client, "public", "people", &pk, &q, 0, 50)
                .await
                .unwrap();
            let count = db::row_count_matching(client, "public", "people", &q)
                .await
                .unwrap()
                .unwrap();
            (ids(&set), count.rows)
        }
    };
    let filter = |column: &str, op, value: &str| db::RowFilter {
        column: column.into(),
        op,
        value: value.into(),
    };

    // Sort by age, NULLs last, ties by key.
    let by_age = db::RowQuery {
        sort: Some(("age".into(), db::SortDir::Asc)),
        ..Default::default()
    };
    assert_eq!(run(by_age).await, (["3", "1", "5", "4", "2"].map(String::from).to_vec(), 5));
    // Typed comparison: '30' is compared as an int.
    let q = db::RowQuery {
        filters: vec![filter("age", db::FilterOp::Ge, "30")],
        sort: Some(("age".into(), db::SortDir::Desc)),
        ..Default::default()
    };
    assert_eq!(run(q).await, (["4", "1", "5"].map(String::from).to_vec(), 3));
    // LIKE wildcards in the value are literal.
    let q = db::RowQuery {
        filters: vec![filter("name", db::FilterOp::Contains, "%_")],
        ..Default::default()
    };
    assert_eq!(run(q).await, (vec!["4".to_string()], 1));
    // Whole-row search ignores case and finds any column.
    let q = db::RowQuery {
        search: "BOB".into(),
        ..Default::default()
    };
    assert_eq!(run(q).await, (vec!["2".to_string()], 1));
    let q = db::RowQuery {
        filters: vec![
            filter("age", db::FilterOp::IsNotNull, ""),
            filter("name", db::FilterOp::StartsWith, "a"),
        ],
        ..Default::default()
    };
    assert_eq!(run(q).await, (["1", "5"].map(String::from).to_vec(), 2));
    // A value the column can't take is an error, not an empty page.
    let bad = db::RowQuery {
        filters: vec![filter("age", db::FilterOp::Eq, "old")],
        ..Default::default()
    };
    assert!(
        db::fetch_rows(&client, "public", "people", &pk, &bad, 0, 50)
            .await
            .is_err()
    );
}
