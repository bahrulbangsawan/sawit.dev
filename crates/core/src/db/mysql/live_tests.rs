//! The MySQL backend against a live server, when `SAWIT_TEST_MYSQL_URL` names one
//! (`mysql://root@127.0.0.1:3307/`); skipped without it.

use crate::db::rows::{BulkChange, RowQuery, RowTarget};
use crate::db::{DbKind, parse_database_url};

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

/// A live server for the tests below, when `SAWIT_TEST_MYSQL_URL` names one (its user must
/// create and drop the database `sawit_test`).
fn live() -> Option<(crate::db::ConnProfile, Option<String>)> {
    let url = std::env::var("SAWIT_TEST_MYSQL_URL").ok()?;
    Some(parse_database_url(&url).expect("SAWIT_TEST_MYSQL_URL parses"))
}

#[tokio::test]
async fn live_round_trip() {
    use crate::db::Conn;
    use crate::db::query::ReadOnlyTry;
    let Some((mut profile, password)) = live() else {
        return;
    };
    profile.database.clear();
    let admin = Conn::connect(&profile, password.as_deref()).await.unwrap();
    admin
        .run_sql(
            "DROP DATABASE IF EXISTS sawit_test; CREATE DATABASE sawit_test; \
             CREATE TABLE sawit_test.users (id INT PRIMARY KEY, email VARCHAR(100) UNIQUE, \
               photo BLOB, flag BIT(3), born DATE); \
             CREATE TABLE sawit_test.orders (id INT AUTO_INCREMENT PRIMARY KEY, \
               user_id INT, total DECIMAL(8,2), CONSTRAINT orders_user FOREIGN KEY (user_id) \
               REFERENCES sawit_test.users (id)); \
             INSERT INTO sawit_test.users VALUES (1, 'ann@x', X'00FF', b'101', '2000-01-02'), \
               (2, 'bob\\\\y', NULL, NULL, NULL); \
             INSERT INTO sawit_test.orders (user_id, total) VALUES (1, 9.50), (2, 3)",
            10,
        )
        .await
        .unwrap();

    profile.database = "sawit_test".into();
    let (rw, ro, schema, version) = Conn::connect_server(&profile, password.as_deref())
        .await
        .unwrap();
    assert!(version.unwrap().starts_with("M"));
    assert_eq!(schema.len(), 1);
    let orders = schema[0]
        .relations
        .iter()
        .find(|r| r.name == "orders")
        .unwrap();
    assert_eq!(orders.foreign_keys[0].ref_table, "users");
    assert_eq!(ro.primary_key("sawit_test", "users").await.unwrap(), ["id"]);

    let page = ro
        .fetch_rows(
            "sawit_test",
            "users",
            &["id".into()],
            &RowQuery::default(),
            0,
            10,
        )
        .await
        .unwrap();
    assert_eq!(page.columns, ["id", "email", "photo", "flag", "born"]);
    assert_eq!(
        page.rows[0],
        [s("1"), s("ann@x"), s("\\x00ff"), s("5"), s("2000-01-02")]
    );
    assert_eq!(page.rows[1][1], s("bob\\y"));

    let searched = RowQuery {
        search: "BOB\\".into(),
        ..RowQuery::default()
    };
    let n = ro
        .row_count_matching("sawit_test", "users", &searched)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(n.rows, 1);

    // Reads run read-only; writes are refused there, before anything runs.
    match ro
        .run_if_read_only("SELECT COUNT(*) AS n FROM orders", 10)
        .await
        .unwrap()
    {
        ReadOnlyTry::Read(out) => assert_eq!(out.results[0].rows[0][0], s("2")),
        other => panic!("{other:?}"),
    }
    for write in [
        "DELETE FROM orders",
        "DROP TABLE orders",
        "SET SESSION TRANSACTION READ WRITE",
    ] {
        assert!(matches!(
            ro.run_if_read_only(write, 10).await.unwrap(),
            ReadOnlyTry::Writes(_)
        ));
        assert!(ro.run_sql(write, 10).await.is_err(), "{write}");
    }
    // The read-only session refuses a write even if the screen let one through.
    assert!(ro.run_bulk("DELETE FROM orders").await.is_err());

    let changed = rw
        .run_bulk(&crate::db::bulk_sql_for(
            DbKind::Mysql,
            "sawit_test",
            "orders",
            &[],
            &RowTarget::Keys {
                key: vec!["id".into()],
                rows: vec![vec!["1".into()]],
            },
            &BulkChange::Update {
                column: "total".into(),
                value: Some("10".into()),
            },
        ))
        .await
        .unwrap();
    assert_eq!(changed, 1);

    let out = rw
        .run_sql(
            "SELECT 1 AS a; UPDATE orders SET total = total + 1; SELECT 2 AS b",
            10,
        )
        .await
        .unwrap();
    assert_eq!(out.results.len(), 2);
    assert_eq!(out.affected, [2]);

    let dir = std::env::temp_dir().join(format!("sawit-mysql-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("users.csv");
    let summary = ro
        .export_rows(
            "sawit_test",
            "users",
            &["id".into()],
            &RowQuery::default(),
            crate::db::ExportFormat::Csv,
            &[],
            &path,
            Default::default(),
            Default::default(),
        )
        .await
        .unwrap();
    assert_eq!(summary.rows, 2);
    assert!(std::fs::read_to_string(&path).unwrap().contains("ann@x"));
    let _ = std::fs::remove_dir_all(&dir);

    admin.run_sql("DROP DATABASE sawit_test", 1).await.unwrap();
}

/// A read-only connection reads SQL as the statement check does, whatever the server's
/// `sql_mode`: the session drops `NO_BACKSLASH_ESCAPES` and `ANSI_QUOTES`. The global mode is
/// put back afterwards, whatever happens in between.
#[tokio::test]
async fn live_read_only_session_drops_quoting_modes() {
    use crate::db::Conn;
    let Some((mut profile, password)) = live() else {
        return;
    };
    profile.database.clear();
    let admin = Conn::connect(&profile, password.as_deref()).await.unwrap();
    let cell = |out: crate::db::query::QueryOutcome| out.results[0].rows[0][0].clone().unwrap_or_default();
    let before = cell(admin.run_sql("SELECT @@GLOBAL.sql_mode", 1).await.unwrap());
    let set = "SET GLOBAL sql_mode = CONCAT(@@GLOBAL.sql_mode, ',NO_BACKSLASH_ESCAPES,ANSI_QUOTES')";
    let seen = match admin.run_sql(set, 1).await {
        Ok(_) => match Conn::connect_server(&profile, password.as_deref()).await {
            Ok((_rw, ro, _schema, _version)) => ro
                .run_sql("SELECT @@SESSION.sql_mode", 1)
                .await
                .map(cell)
                .unwrap_or_else(|e| format!("error: {e}")),
            Err(e) => format!("error: {e}"),
        },
        Err(e) => format!("error: {e}"),
    };
    let restore = format!("SET GLOBAL sql_mode = '{}'", before.replace('\'', "''"));
    admin.run_sql(&restore, 1).await.unwrap();
    assert!(!seen.starts_with("error") && !seen.contains("NO_BACKSLASH_ESCAPES"), "{seen}");
    assert!(!seen.contains("ANSI_QUOTES"), "{seen}");
}
