//! Writes to a local D1 database through a dev server's Local Explorer or straight to the file,
//! and the statements each kind takes.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::db::d1::LocalD1;
use crate::db::explorer::tests::fake_explorer;
use crate::db::profile::{ConnProfile, DbKind};
use crate::db::rows::{BulkChange, RowQuery, RowTarget};

fn local_d1(dir: &Path) -> (ConnProfile, PathBuf) {
    let file = dir.join("db.sqlite");
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute_batch(
            "PRAGMA journal_mode = WAL; CREATE TABLE t (id INTEGER PRIMARY KEY, x TEXT); INSERT INTO t (x) VALUES ('a'), ('b');",
        )
        .unwrap();
    drop(conn);
    let profile = ConnProfile::d1_local(&LocalD1 {
        project: dir.to_path_buf(),
        worker: Some("api".into()),
        binding: "DB".into(),
        database_name: Some("shop".into()),
        miniflare_id: "5d0c".into(),
        file: file.clone(),
        exists: true,
    });
    (profile, file)
}

fn handle(profile: &ConnProfile, read_only: bool, addrs: Vec<SocketAddr>) -> Conn {
    let mut lite = Lite::new(profile.clone(), read_only);
    lite.explorer_addrs = Some(addrs);
    Conn::Lite(Arc::new(lite))
}

#[tokio::test]
async fn writes_go_through_a_running_explorer_and_never_touch_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (profile, file) = local_d1(dir.path());
    let before = std::fs::read(&file).unwrap();
    let (addr, seen) = fake_explorer("api", "5d0c").await;
    let rw = handle(&profile, false, vec![addr]);
    let Conn::Lite(lite) = &rw else {
        unreachable!()
    };
    assert_eq!(
        lite.write_route().await,
        WriteRoute::Explorer {
            addr,
            id: "5d0c".into()
        }
    );
    let (out, route) = lite.write("DELETE FROM t", 10).await.unwrap();
    assert!(route.label().contains("Local Explorer"));
    assert_eq!(out.affected, [2]);
    assert!(seen.lock().unwrap().iter().any(
        |(line, body)| line.contains("/d1/database/5d0c/raw") && body.contains("DELETE FROM t")
    ));
    // workerd (here, the fake) made the change; the file is as it was.
    assert_eq!(std::fs::read(&file).unwrap(), before);
    let n = rw
        .row_count_matching("main", "t", &RowQuery::default())
        .await
        .unwrap();
    assert_eq!(n.unwrap().rows, 2);
}

#[tokio::test]
async fn without_a_dev_server_writes_go_to_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let (profile, _) = local_d1(dir.path());
    let rw = handle(&profile, false, vec![]);
    let Conn::Lite(lite) = &rw else {
        unreachable!()
    };
    assert!(matches!(lite.write_route().await, WriteRoute::File(_)));
    assert_eq!(rw.run_bulk("DELETE FROM t WHERE x = 'a'").await.unwrap(), 1);
    // Reads take the read-only path, even on the read-write handle.
    let read = rw.run_sql("SELECT count(*) FROM t", 10).await.unwrap();
    assert_eq!(read.results[0].rows[0][0].as_deref(), Some("1"));
    let ro = handle(&profile, true, vec![]);
    let e = ro.run_bulk("DELETE FROM t").await.unwrap_err();
    assert!(e.to_string().contains("only reads"), "{e}");
    let e = ro.run_read_only("DELETE FROM t", 10).await.unwrap_err();
    assert!(e.to_string().contains("read-only"), "{e}");
}

/// By hand only, against a real `wrangler dev` in a scratch project:
/// `SAWIT_D1_LIVE=<project folder> cargo test -p sawit-core live_wrangler_dev -- --ignored`.
/// The project's `DB` binding needs a `products` table.
#[tokio::test]
#[ignore = "needs a running wrangler dev in a scratch project"]
async fn live_wrangler_dev() {
    let project = PathBuf::from(std::env::var("SAWIT_D1_LIVE").unwrap());
    let d1 = crate::db::d1::find_local(&project, "DB").expect("a DB binding");
    assert!(d1.exists, "{}", d1.file.display());
    let profile = ConnProfile::d1_local(&d1);
    let rw = Conn::lite(profile.clone(), false);
    let Conn::Lite(lite) = &rw else {
        unreachable!()
    };
    let route = lite.write_route().await;
    assert!(matches!(route, WriteRoute::Explorer { .. }), "{route:?}");
    let (out, _) = lite
        .write(
            "INSERT INTO products (name, price) VALUES ('Sawit live', 1.0)",
            10,
        )
        .await
        .unwrap();
    assert_eq!(out.affected, [1]);
    // Read back from the file, read-only, while workerd holds it.
    let ro = Conn::lite(profile, true);
    let n = ro
        .run_read_only("SELECT count(*) FROM products WHERE name = 'Sawit live'", 1)
        .await
        .unwrap();
    assert_ne!(n.results[0].rows[0][0].as_deref(), Some("0"));
    assert!(!holders(&d1.file).await.is_empty());
}

#[test]
fn statements_follow_the_kind() {
    assert_eq!(
        relation_sql_for(DbKind::Postgres, "public", "t"),
        r#""public"."t""#
    );
    assert_eq!(relation_sql_for(DbKind::D1Local, "main", "t"), r#""t""#);
    assert_eq!(
        empty_sql_for(DbKind::D1Remote, "main", "t"),
        r#"DELETE FROM "t""#
    );
    assert_eq!(
        drop_sql_for(DbKind::Sqlite, "main", "v", crate::db::RelKind::View, true),
        r#"DROP VIEW "v""#
    );
    let all = RowTarget::Matching(RowQuery::default());
    assert_eq!(
        bulk_sql_for(DbKind::D1Local, "main", "t", &[], &all, &BulkChange::Delete),
        r#"DELETE FROM "t""#
    );
}

#[test]
fn routes_say_where_writes_go() {
    let remote = WriteRoute::Remote {
        database: "shop".into(),
    };
    assert_eq!(remote.label(), "remote (production)");
    assert!(remote.describe().contains("remote (production)"));
    let busy = WriteRoute::Busy {
        holders: vec!["workerd (pid 7)".into()],
    };
    assert!(
        busy.describe()
            .starts_with("workerd (pid 7) has this database open")
    );
}
