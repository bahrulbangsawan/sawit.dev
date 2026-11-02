//! Connecting to a real server: TLS by sslmode, cancelling a running query, a pooler that refuses
//! session settings, several hosts, sockets, and failures sorted by kind.

use sawit_core::db::{self, ConnProfile};

use super::cluster;

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
