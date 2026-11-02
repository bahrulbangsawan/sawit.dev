//! Changes on a real server: exports and bulk changes that follow the browser's query, `pg_dump`
//! backups, CSV imports and drops.

use std::path::PathBuf;

use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::db::{self, RelKind};

use super::cluster;

#[tokio::test]
async fn exports_and_bulk_changes_follow_the_query() {
    use std::sync::atomic::{AtomicBool, AtomicU64};
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE TABLE items (id int PRIMARY KEY, name text, price numeric(8,2), ok boolean);
             INSERT INTO items SELECT g, 'item, ' || g, g * 1.5, g % 2 = 0 FROM generate_series(1, 10) g;
             CREATE TABLE pairs (a int, b text, n int, PRIMARY KEY (a, b));
             INSERT INTO pairs VALUES (1, 'x', 0), (1, 'y', 0), (2, 'x', 0);",
        )
        .await
        .unwrap();
    let schemas = db::load_schema(&client).await.unwrap();
    let types = schemas
        .iter()
        .flat_map(|s| &s.relations)
        .find(|r| r.name == "items")
        .unwrap()
        .columns
        .clone();
    let dir = tempfile::tempdir().unwrap();
    let query = db::RowQuery {
        filters: vec![db::RowFilter {
            column: "id".into(),
            op: db::FilterOp::Le,
            value: "3".into(),
        }],
        sort: Some(("id".into(), db::SortDir::Desc)),
        ..Default::default()
    };
    let key = vec!["id".to_string()];
    let (progress, stop) = (AtomicU64::new(0), AtomicBool::new(false));
    let csv = dir.path().join("items.csv");
    let summary = db::export_rows(
        &client,
        "public",
        "items",
        &key,
        &query,
        db::ExportFormat::Csv,
        &types,
        &csv,
        &progress,
        &stop,
    )
    .await
    .unwrap();
    assert_eq!(
        summary,
        db::ExportSummary {
            rows: 3,
            truncated: false
        }
    );
    assert_eq!(
        std::fs::read_to_string(&csv).unwrap(),
        "id,name,price,ok\r\n3,\"item, 3\",4.50,f\r\n2,\"item, 2\",3.00,t\r\n1,\"item, 1\",1.50,f\r\n"
    );
    let xlsx = dir.path().join("items.xlsx");
    let summary = db::export_rows(
        &client,
        "public",
        "items",
        &key,
        &db::RowQuery::default(),
        db::ExportFormat::Xlsx,
        &types,
        &xlsx,
        &progress,
        &stop,
    )
    .await
    .unwrap();
    assert_eq!(summary.rows, 10);
    assert_eq!(&std::fs::read(&xlsx).unwrap()[..2], b"PK");
    if let Some(out) = std::env::var_os("SAWIT_EXPORT_SAMPLE") {
        std::fs::copy(&xlsx, out).unwrap();
    }

    // Bulk changes: picked keys, then everything the query matches.
    let run = |target: db::RowTarget, change: db::BulkChange| {
        let client = &client;
        async move { db::run_bulk(client, &db::bulk_sql("public", "items", &target, &change)).await }
    };
    let picked = db::RowTarget::Keys {
        key: key.clone(),
        rows: vec![vec!["9".into()], vec!["10".into()]],
    };
    assert_eq!(run(picked, db::BulkChange::Delete).await.unwrap(), 2);
    let matching = db::RowTarget::Matching(query.clone());
    let set_name = db::BulkChange::Update {
        column: "name".into(),
        value: Some("cheap".into()),
    };
    assert_eq!(run(matching, set_name).await.unwrap(), 3);
    let set_price = db::BulkChange::Update {
        column: "price".into(),
        value: Some("not a number".into()),
    };
    assert!(
        run(db::RowTarget::Matching(query), set_price)
            .await
            .is_err()
    );
    let rows = client
        .query(
            "SELECT count(*) FILTER (WHERE name = 'cheap'), count(*) FROM items",
            &[],
        )
        .await
        .unwrap();
    assert_eq!((rows[0].get::<_, i64>(0), rows[0].get::<_, i64>(1)), (3, 8));

    // Composite keys, with untyped literals taking the key columns' types.
    let pairs = db::RowTarget::Keys {
        key: vec!["a".into(), "b".into()],
        rows: vec![vec!["1".into(), "y".into()], vec!["2".into(), "x".into()]],
    };
    let bump = db::BulkChange::Update {
        column: "n".into(),
        value: Some("7".into()),
    };
    let sql = db::bulk_sql("public", "pairs", &pairs, &bump);
    assert_eq!(db::run_bulk(&client, &sql).await.unwrap(), 2);
    let sql = db::bulk_sql(
        "public",
        "pairs",
        &db::RowTarget::Matching(Default::default()),
        &db::BulkChange::Delete,
    );
    assert_eq!(db::run_bulk(&client, &sql).await.unwrap(), 3);
}

#[tokio::test]
async fn backups_imports_and_drops() {
    let Some((_cluster, profile)) = cluster().await else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let roots = SearchRoots::detect(PathBuf::from("/nonexistent")).await;
    let bins = discover(&roots).await;
    let client = db::connect(&profile, None).await.unwrap();
    client
        .batch_execute(
            "CREATE SCHEMA \"Sales\";
             CREATE TABLE \"Sales\".orders (id int PRIMARY KEY, note text, qty int);
             INSERT INTO \"Sales\".orders VALUES (1, 'a', 1);
             CREATE TABLE other (id int);
             CREATE VIEW \"Sales\".big AS SELECT * FROM \"Sales\".orders WHERE qty > 1;",
        )
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();

    // Backups: the table alone, then the whole database in custom format.
    let sql = dir.path().join("orders.sql");
    let table = db::BackupScope::Table {
        schema: "Sales".into(),
        table: "orders".into(),
    };
    db::backup(&client, &profile, None, &bins, &table, &sql)
        .await
        .unwrap();
    let text = std::fs::read_to_string(&sql).unwrap();
    assert!(text.contains("CREATE TABLE \"Sales\".orders"), "{text}");
    assert!(!text.contains("CREATE TABLE public.other"));
    let dump = dir.path().join("all.dump");
    db::backup(
        &client,
        &profile,
        None,
        &bins,
        &db::BackupScope::Database,
        &dump,
    )
    .await
    .unwrap();
    assert_eq!(&std::fs::read(&dump).unwrap()[..5], b"PGDMP");
    // pg_dump errors come back as the error, and leave no file behind.
    let missing = db::BackupScope::Table {
        schema: "Sales".into(),
        table: "nope".into(),
    };
    let bad = dir.path().join("nope.sql");
    let err = db::backup(&client, &profile, None, &bins, &missing, &bad)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no matching tables"), "{err}");
    assert!(!bad.exists());

    // CSV import, columns by header (in any order), empty fields as NULL.
    let csv = dir.path().join("in.csv");
    std::fs::write(&csv, "qty,id,note\r\n5,2,\"x, y\"\r\n7,3,\r\n").unwrap();
    assert_eq!(
        db::import_csv(&client, "Sales", "orders", &csv)
            .await
            .unwrap(),
        2
    );
    let row = client
        .query_one("SELECT count(*), count(note) FROM \"Sales\".orders", &[])
        .await
        .unwrap();
    assert_eq!((row.get::<_, i64>(0), row.get::<_, i64>(1)), (3, 2));
    // A bad row rolls the whole file back.
    std::fs::write(&csv, "id,qty\n4,1\n5,lots\n").unwrap();
    assert!(
        db::import_csv(&client, "Sales", "orders", &csv)
            .await
            .is_err()
    );
    let n: i64 = client
        .query_one("SELECT count(*) FROM \"Sales\".orders", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 3);

    // Dropping a table a view depends on needs CASCADE.
    let plain = db::drop_sql("Sales", "orders", RelKind::Table, false);
    assert!(db::run_bulk(&client, &plain).await.is_err());
    db::run_bulk(&client, &db::truncate_sql("Sales", "orders"))
        .await
        .unwrap();
    let cascade = db::drop_sql("Sales", "orders", RelKind::Table, true);
    db::run_bulk(&client, &cascade).await.unwrap();
    let left: i64 = client
        .query_one(
            "SELECT count(*) FROM pg_class WHERE relname IN ('orders', 'big')",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(left, 0);
}
