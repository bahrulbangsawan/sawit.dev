//! Read-only access on a real server: the agents' role and connection can't be talked into
//! writing, and SQL is tried read-only before anything else.

use sawit_core::db::{self, ConnProfile};

use super::cluster;

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

/// `sawit <db> "sql"` tries SQL read-only first (`run_if_read_only`): a read runs, and a write
/// (a `SELECT` that does one too), what must run outside a transaction, `COPY` and several
/// statements come back as what they are, with nothing of them run. A typo or a missing table is
/// the statement's own error.
#[tokio::test]
async fn sql_is_tried_read_only_first() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let admin = db::connect(&profile, None).await.unwrap();
    admin
        .batch_execute(
            "CREATE TABLE t (x int); INSERT INTO t VALUES (1); CREATE SEQUENCE s;
             CREATE PROCEDURE add_one() LANGUAGE plpgsql AS $$
               BEGIN INSERT INTO t VALUES (2); COMMIT; END $$;",
        )
        .await
        .unwrap();
    let ro = db::connect_read_only(&profile, None).await.unwrap();
    match db::run_if_read_only(&ro, "SELECT x FROM t", 10).await {
        Ok(db::ReadOnlyTry::Read(read)) => {
            assert_eq!(read.results[0].rows[0][0].as_deref(), Some("1"));
        }
        other => panic!("a read: {other:?}"),
    }
    for sql in [
        "DELETE FROM t",
        "INSERT INTO t VALUES (2)",
        "UPDATE t SET x = 3",
        "SELECT nextval('s')",
        "TRUNCATE t",
        "DROP TABLE t",
        "CREATE TABLE u (y int)",
        "VACUUM t",
        "CREATE DATABASE other",
        "COPY t TO STDOUT",
        "CALL add_one()",
        // Ends the transaction it runs in before it writes anything.
        "DO $$ BEGIN COMMIT; INSERT INTO t VALUES (3); END $$",
    ] {
        let tried = db::run_if_read_only(&ro, sql, 10).await;
        assert!(
            matches!(tried, Ok(db::ReadOnlyTry::Writes(_))),
            "{sql}: {tried:?}"
        );
    }
    for sql in ["SELECT 1; SELECT 2", "COMMIT; DELETE FROM t"] {
        let tried = db::run_if_read_only(&ro, sql, 10).await;
        assert!(
            matches!(tried, Ok(db::ReadOnlyTry::Several(_))),
            "{sql}: {tried:?}"
        );
    }
    for sql in ["SELEC 1", "SELECT * FROM missing_table"] {
        assert!(db::run_if_read_only(&ro, sql, 10).await.is_err(), "{sql}");
    }
    // None of them ran.
    let left = db::run_sql(&admin, "SELECT count(*), max(x) FROM t", 1)
        .await
        .unwrap();
    assert_eq!(
        left.results[0].rows[0],
        [Some("1".to_string()), Some("1".to_string())]
    );
    let next = db::run_sql(&admin, "SELECT nextval('s')", 1).await.unwrap();
    assert_eq!(next.results[0].rows[0][0].as_deref(), Some("1"));
    let databases = db::list_databases(&admin).await.unwrap();
    assert!(!databases.contains(&"other".to_string()));
    // A read that only names the read-only switch is a read: `sawit shop
    // product_characteristics` asked about it as a write. A switch run alone is rolled back with
    // the try, so the session stays read-only.
    admin
        .batch_execute("CREATE TABLE product_characteristics (x int)")
        .await
        .unwrap();
    let switch = "SET SESSION CHARACTERISTICS AS TRANSACTION READ WRITE";
    for sql in [
        "SELECT * FROM product_characteristics",
        "SHOW transaction_read_only",
        switch,
    ] {
        let tried = db::run_if_read_only(&ro, sql, 10).await;
        assert!(
            matches!(tried, Ok(db::ReadOnlyTry::Read(_))),
            "{sql}: {tried:?}"
        );
    }
    let mode = db::run_sql(&ro, "SHOW default_transaction_read_only", 1)
        .await
        .unwrap();
    assert_eq!(mode.results[0].rows[0][0].as_deref(), Some("on"));
    // `run_read_only` refuses them all, as before, and a switch's words too.
    assert!(db::run_read_only(&ro, switch, 10).await.is_err());
    assert!(db::run_read_only(&ro, "DELETE FROM t", 10).await.is_err());
    assert!(
        db::run_read_only(&ro, "SELECT 1; SELECT 2", 10)
            .await
            .is_err()
    );
}
