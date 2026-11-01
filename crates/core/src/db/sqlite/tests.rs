//! SQLite files as workerd leaves them: the schema tree, pages, exports, read-only runs, and
//! writes that leave the folder as it was.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};

use rusqlite::Connection;
use rusqlite::config::DbConfig;

use super::*;
use crate::db::export::ExportFormat;
use crate::db::query::{ConstraintKind, ReadOnlyTry, RelKind, ResultSet, RowCount};
use crate::db::rows::{BulkChange, FilterOp, RowFilter, RowQuery, RowTarget, SortDir};

/// A WAL database like the ones workerd leaves: data in the `-wal`, `-wal` and `-shm`
/// kept after it closes.
fn shop(dir: &Path, keep_wal: bool) -> PathBuf {
    let path = dir.join("shop.sqlite");
    let conn = Connection::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch(
            "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT UNIQUE);
             CREATE TABLE orders (id INTEGER PRIMARY KEY, customer_id INTEGER REFERENCES customers, total REAL, note TEXT);
             CREATE INDEX orders_customer ON orders (customer_id);
             CREATE VIEW big_orders AS SELECT * FROM orders WHERE total > 10;
             CREATE TABLE _cf_KV (key TEXT PRIMARY KEY, value BLOB);
             INSERT INTO customers (name, email) VALUES ('Ana', 'ana@x.id'), ('Budi', NULL), ('Citra', 'c@x.id');
             INSERT INTO orders (customer_id, total, note) VALUES (1, 12.5, 'first'), (1, 3, NULL), (2, 40, 'Rush 100%'), (3, 7.25, 'gift');",
        )
        .unwrap();
    if keep_wal {
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
    }
    drop(conn);
    assert_eq!(sidecar(&path, "-wal").exists(), keep_wal);
    path
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn schema_lists_tables_views_keys_and_indexes() {
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), true);
    let schemas = with_read(&path, load_schema).unwrap();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name, "main");
    let names: Vec<_> = schemas[0]
        .relations
        .iter()
        .map(|r| r.name.as_str())
        .collect();
    // D1's own tables stay hidden.
    assert_eq!(names, ["big_orders", "customers", "orders"]);
    let orders = &schemas[0].relations[2];
    assert_eq!(orders.primary_key, ["id"]);
    assert_eq!(orders.columns[2].data_type, "REAL");
    assert!(orders.columns[2].nullable);
    assert_eq!(orders.foreign_keys[0].ref_table, "customers");
    // No column named: the parent's primary key.
    assert_eq!(orders.foreign_keys[0].ref_columns, ["id"]);
    assert!(
        orders
            .indexes
            .iter()
            .any(|i| i.name == "orders_customer" && i.definition.starts_with("CREATE INDEX"))
    );
    let customers = &schemas[0].relations[1];
    assert!(!customers.columns[1].nullable);
    assert!(
        customers
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Unique)
    );
    assert_eq!(schemas[0].relations[0].kind, RelKind::View);
}

#[test]
fn rows_sort_filter_search_and_page() {
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), true);
    let key = vec!["id".to_string()];
    let page = |q: &RowQuery, offset, limit| {
        with_read(&path, |c| fetch_rows(c, "orders", &key, q, offset, limit)).unwrap()
    };
    let all = page(&RowQuery::default(), 0, 50);
    assert_eq!(all.columns, ["id", "customer_id", "total", "note"]);
    assert_eq!(all.rows.len(), 4);
    // A whole REAL keeps its point; NULL stays NULL.
    assert_eq!(all.rows[1][2].as_deref(), Some("3.0"));
    assert_eq!(all.rows[1][3], None);
    let by_total = RowQuery {
        sort: Some(("total".into(), SortDir::Desc)),
        ..RowQuery::default()
    };
    let ids = |set: &ResultSet| -> Vec<String> {
        set.rows.iter().map(|r| r[0].clone().unwrap()).collect()
    };
    assert_eq!(ids(&page(&by_total, 0, 2)), ["3", "1"]);
    assert_eq!(ids(&page(&by_total, 2, 2)), ["4", "2"]);
    let filtered = RowQuery {
        filters: vec![RowFilter {
            column: "customer_id".into(),
            op: FilterOp::Eq,
            value: "1".into(),
        }],
        ..RowQuery::default()
    };
    assert_eq!(ids(&page(&filtered, 0, 50)), ["1", "2"]);
    // LIKE's own characters are matched literally, in any case.
    let search = RowQuery {
        search: "RUSH 100%".into(),
        ..RowQuery::default()
    };
    assert_eq!(ids(&page(&search, 0, 50)), ["3"]);
    let not_null = RowQuery {
        filters: vec![RowFilter {
            column: "note".into(),
            op: FilterOp::NotContains,
            value: "i".into(),
        }],
        ..RowQuery::default()
    };
    assert_eq!(ids(&page(&not_null, 0, 50)), ["2", "3"]);
    let count = with_read(&path, |c| row_count(c, "orders", &filtered)).unwrap();
    assert_eq!(
        count,
        RowCount {
            rows: 2,
            exact: true
        }
    );
}

#[test]
fn exports_matching_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), true);
    let out = dir.path().join("orders.csv");
    let query = RowQuery {
        sort: Some(("total".into(), SortDir::Asc)),
        ..RowQuery::default()
    };
    let summary = with_read(&path, |c| {
        export_rows(
            c,
            "orders",
            &["id".into()],
            &query,
            ExportFormat::Csv,
            &[],
            &out,
            &AtomicU64::new(0),
            &AtomicBool::new(false),
        )
    })
    .unwrap();
    assert_eq!(summary.rows, 4);
    let text = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        text.lines().collect::<Vec<_>>(),
        [
            "id,customer_id,total,note",
            "2,1,3.0,",
            "4,3,7.25,gift",
            "1,1,12.5,first",
            "3,2,40.0,Rush 100%"
        ]
    );
}

#[test]
fn read_only_refuses_writes_and_leaves_the_folder_as_it_was() {
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), true);
    let before = files(dir.path());
    assert_eq!(
        before,
        ["shop.sqlite", "shop.sqlite-shm", "shop.sqlite-wal"]
    );
    let tried = with_read(&path, |c| run_if_read_only(c, "DELETE FROM orders", 10)).unwrap();
    assert!(matches!(tried, ReadOnlyTry::Writes(_)), "{tried:?}");
    let e = with_read(&path, |c| {
        run_read_only(c, "UPDATE orders SET total = 0", 10)
    })
    .unwrap_err();
    assert!(e.to_string().contains("read-only"), "{e}");
    // Even past the screening, the connection itself refuses.
    let e = with_read(&path, |c| {
        c.execute_batch("PRAGMA query_only = 0; DELETE FROM orders")
            .map_err(lite_error)
    })
    .unwrap_err();
    assert!(e.to_string().contains("readonly"), "{e}");
    assert!(
        with_read(&path, |c| run_read_only(c, "SELECT 1; SELECT 2", 10)).is_err(),
        "several statements"
    );
    let read = with_read(&path, |c| {
        run_read_only(c, "SELECT count(*) FROM orders", 10)
    })
    .unwrap();
    assert_eq!(read.results[0].rows[0][0].as_deref(), Some("4"));
    assert_eq!(files(dir.path()), before);
}

#[test]
fn a_checkpointed_wal_database_reads_without_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), false);
    assert_eq!(files(dir.path()), ["shop.sqlite"]);
    let n = with_read(&path, |c| row_count(c, "customers", &RowQuery::default())).unwrap();
    assert_eq!(n.rows, 3);
    assert_eq!(files(dir.path()), ["shop.sqlite"]);
}

#[test]
fn writes_keep_a_wal_that_was_there_and_remove_one_they_made() {
    let dir = tempfile::tempdir().unwrap();
    let kept = shop(dir.path(), true);
    let out = with_write(&kept, |c| {
        run_write(c, "UPDATE orders SET total = total + 1 WHERE id = 2", 10)
    })
    .unwrap();
    assert_eq!(out.affected, [1]);
    assert_eq!(
        files(dir.path()),
        ["shop.sqlite", "shop.sqlite-shm", "shop.sqlite-wal"]
    );
    let total = with_read(&kept, |c| {
        run_read_only(c, "SELECT total FROM orders WHERE id = 2", 1)
    })
    .unwrap();
    assert_eq!(total.results[0].rows[0][0].as_deref(), Some("4.0"));

    let other = tempfile::tempdir().unwrap();
    let clean = shop(other.path(), false);
    with_write(&clean, |c| {
        run_write(c, "DELETE FROM orders WHERE id = 4", 10)
    })
    .unwrap();
    assert_eq!(files(other.path()), ["shop.sqlite"]);
    // A failing batch changes nothing.
    let e = with_write(&clean, |c| {
        run_write(c, "DELETE FROM orders; INSERT INTO nowhere VALUES (1)", 10)
    })
    .unwrap_err();
    assert!(e.to_string().contains("nowhere"), "{e}");
    let n = with_read(&clean, |c| row_count(c, "orders", &RowQuery::default())).unwrap();
    assert_eq!(n.rows, 3);
    assert_eq!(files(other.path()), ["shop.sqlite"]);
}

#[test]
fn bulk_changes_use_sqlite_literals() {
    let target = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![vec!["1".into()], vec!["3".into()]],
    };
    let change = BulkChange::Update {
        column: "note".into(),
        value: Some("it's \\ fine".into()),
    };
    assert_eq!(
        bulk_sql("orders", &[], &target, &change),
        r#"UPDATE "orders" SET "note" = 'it''s \ fine' WHERE ("id" = '1') OR ("id" = '3')"#
    );
    let dir = tempfile::tempdir().unwrap();
    let path = shop(dir.path(), true);
    let cols = vec!["id".into(), "note".into()];
    let sql = bulk_sql("orders", &cols, &target, &BulkChange::Delete);
    let out = with_write(&path, |c| run_write(c, &sql, 1)).unwrap();
    assert_eq!(out.affected, [2]);
    let none = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![],
    };
    assert!(bulk_sql("orders", &cols, &none, &BulkChange::Delete).ends_with("WHERE 0"));
}

#[test]
fn missing_files_are_never_created() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("none.sqlite");
    assert!(with_read(&path, load_schema).is_err());
    assert!(with_write(&path, |c| run_write(c, "CREATE TABLE t (x)", 1)).is_err());
    assert!(files(dir.path()).is_empty());
}
