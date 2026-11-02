//! What the table browser reads from a real server: stable pages (by key, or in physical order on
//! big tables), counts within a time limit, filters, search and sort.

use sawit_core::db;

use super::cluster;

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
    assert_eq!(
        run(by_age).await,
        (["3", "1", "5", "4", "2"].map(String::from).to_vec(), 5)
    );
    // Typed comparison: '30' is compared as an int.
    let q = db::RowQuery {
        filters: vec![filter("age", db::FilterOp::Ge, "30")],
        sort: Some(("age".into(), db::SortDir::Desc)),
        ..Default::default()
    };
    assert_eq!(
        run(q).await,
        (["4", "1", "5"].map(String::from).to_vec(), 3)
    );
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
