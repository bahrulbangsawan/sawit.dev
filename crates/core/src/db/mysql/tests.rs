//! MySQL's dialect, statement splitting, the read-only screen, the schema tree from
//! `information_schema` rows and MySQL connection strings, without a server (the live test is
//! in `live_tests.rs`).

use super::run::write_reason;
use super::schema::assemble;
use super::*;
use crate::db::query::{ConstraintKind, RelKind};
use crate::db::rows::{BulkChange, FilterOp, RowFilter, RowQuery, RowTarget, SortDir};
use crate::db::{DbKind, parse_database_url};

fn s(v: &str) -> Option<String> {
    Some(v.to_string())
}

#[test]
fn quotes_names_and_values() {
    assert_eq!(quote("a`b"), "`a``b`");
    assert_eq!(relation("shop", "order items"), "`shop`.`order items`");
    assert_eq!(literal("it's"), "'it''s'");
    // A backslash reads the same with or without NO_BACKSLASH_ESCAPES.
    assert_eq!(literal("a\\b"), "CONVERT(X'615C62' USING utf8mb4)");
}

#[test]
fn version_labels() {
    assert_eq!(version_label("8.4.13"), "MySQL 8.4");
    assert_eq!(version_label("11.4.2-MariaDB-log"), "MariaDB 11.4");
    assert_eq!(version_label("5.5.5-10.11.6-MariaDB"), "MariaDB 10.11");
    assert_eq!(version_label(""), "MySQL");
}

#[test]
fn splits_statements_outside_strings_and_comments() {
    let sql = "SELECT 'a;b', `c;d`, \"e;\\\"f\" FROM t; -- x;y\n# z;\n/* ; */ UPDATE t SET a = 1;;";
    let parts = split_statements(sql);
    assert_eq!(parts.len(), 2, "{parts:?}");
    assert!(parts[0].starts_with("SELECT 'a;b'"));
    assert!(parts[1].contains("UPDATE t SET a = 1"));
    assert!(split_statements("  -- only a comment\n").is_empty());
    assert_eq!(split_statements("SELECT 'it''s; fine'").len(), 1);
}

#[test]
fn read_only_screen() {
    for read in [
        "SELECT 1",
        "  (SELECT 1) UNION (SELECT 2)",
        "/* why */ SELECT * FROM t",
        "WITH x AS (SELECT 1) SELECT * FROM x",
        "SHOW TABLES",
        "DESCRIBE t",
        "EXPLAIN SELECT 1",
        "TABLE t",
    ] {
        assert_eq!(write_reason(read), None, "{read}");
    }
    for write in [
        "INSERT INTO t VALUES (1)",
        "update t set a = 1",
        "DROP TABLE t",
        "CREATE TABLE t (a int)",
        "SET SESSION TRANSACTION READ WRITE",
        "CALL p()",
        "LOAD DATA INFILE 'x' INTO TABLE t",
        "SELECT * FROM t INTO OUTFILE '/tmp/x'",
        "SELECT * INTO   DUMPFILE '/tmp/x' FROM t",
        "/*!40101 SET NAMES utf8 */",
        "EXPLAIN ANALYZE DELETE FROM t",
        "GRANT ALL ON *.* TO x",
    ] {
        assert!(write_reason(write).is_some(), "{write}");
    }
}

#[test]
fn filters_sort_and_pages() {
    let query = RowQuery {
        sort: Some(("name".into(), SortDir::Desc)),
        filters: vec![
            RowFilter {
                column: "id".into(),
                op: FilterOp::Ne,
                value: "7".into(),
            },
            RowFilter {
                column: "name".into(),
                op: FilterOp::Contains,
                value: "50%_off!".into(),
            },
        ],
        search: " ann ".into(),
    };
    let sql = select_sql(
        "shop",
        "users",
        &["id".into(), "name".into()],
        &["id".into()],
        &query,
        Some((100, 50)),
    );
    assert_eq!(
        sql,
        "SELECT * FROM `shop`.`users` WHERE NOT (`id` <=> '7') \
         AND CAST(`name` AS CHAR) LIKE '%50!%!_off!!%' ESCAPE '!' \
         AND (CAST(`id` AS CHAR) LIKE '%ann%' ESCAPE '!' OR CAST(`name` AS CHAR) LIKE '%ann%' ESCAPE '!') \
         ORDER BY `name` IS NULL, `name` DESC, `id` LIMIT 50 OFFSET 100"
    );
    assert_eq!(
        select_sql("shop", "users", &[], &[], &RowQuery::default(), None),
        "SELECT * FROM `shop`.`users`"
    );
}

#[test]
fn bulk_changes() {
    let keys = RowTarget::Keys {
        key: vec!["a".into(), "b".into()],
        rows: vec![vec!["1".into(), "x".into()], vec!["2".into(), "y".into()]],
    };
    assert_eq!(
        bulk_sql("db", "t", &[], &keys, &BulkChange::Delete),
        "DELETE FROM `db`.`t` WHERE (`a`, `b`) IN (('1', 'x'), ('2', 'y'))"
    );
    let one = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![vec!["3".into()]],
    };
    assert_eq!(
        bulk_sql(
            "db",
            "t",
            &[],
            &one,
            &BulkChange::Update {
                column: "note".into(),
                value: None
            }
        ),
        "UPDATE `db`.`t` SET `note` = NULL WHERE `id` IN ('3')"
    );
    let none = RowTarget::Keys {
        key: vec!["id".into()],
        rows: vec![],
    };
    assert_eq!(
        bulk_sql("db", "t", &[], &none, &BulkChange::Delete),
        "DELETE FROM `db`.`t` WHERE FALSE"
    );
}

#[test]
fn schema_from_information_schema() {
    let names = vec!["shop".to_string()];
    let tables = vec![
        vec![s("shop"), s("users"), s("BASE TABLE"), s("12")],
        vec![s("shop"), s("orders"), s("BASE TABLE"), None],
        vec![s("shop"), s("big_orders"), s("VIEW"), None],
    ];
    let columns = vec![
        vec![s("shop"), s("orders"), s("id"), s("int unsigned"), s("NO")],
        vec![s("shop"), s("orders"), s("user_id"), s("int"), s("YES")],
        vec![s("shop"), s("users"), s("id"), s("int"), s("NO")],
        vec![
            s("shop"),
            s("users"),
            s("email"),
            s("varchar(255)"),
            s("NO"),
        ],
    ];
    let keys = vec![
        vec![
            s("shop"),
            s("orders"),
            s("PRIMARY"),
            s("PRIMARY KEY"),
            s("id"),
            None,
            None,
            None,
        ],
        vec![
            s("shop"),
            s("orders"),
            s("orders_user"),
            s("FOREIGN KEY"),
            s("user_id"),
            s("shop"),
            s("users"),
            s("id"),
        ],
        vec![
            s("shop"),
            s("users"),
            s("PRIMARY"),
            s("PRIMARY KEY"),
            s("id"),
            None,
            None,
            None,
        ],
        vec![
            s("shop"),
            s("users"),
            s("email"),
            s("UNIQUE"),
            s("email"),
            None,
            None,
            None,
        ],
    ];
    let checks = vec![vec![
        s("shop"),
        s("users"),
        s("email_ok"),
        s("(`email` like '%@%')"),
    ]];
    let indexes = vec![
        vec![
            s("shop"),
            s("users"),
            s("PRIMARY"),
            s("0"),
            s("BTREE"),
            s("id"),
            None,
        ],
        vec![
            s("shop"),
            s("users"),
            s("email"),
            s("0"),
            s("BTREE"),
            s("email"),
            s("20"),
        ],
    ];
    let tree = assemble(&names, tables, columns, keys, checks, indexes);
    assert_eq!(tree.len(), 1);
    let rels = &tree[0].relations;
    assert_eq!(
        rels.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["big_orders", "orders", "users"]
    );
    assert_eq!(rels[0].kind, RelKind::View);
    let orders = &rels[1];
    assert_eq!(orders.primary_key, ["id"]);
    assert_eq!(orders.foreign_keys[0].ref_table, "users");
    assert_eq!(orders.foreign_keys[0].ref_columns, ["id"]);
    assert!(orders.columns[1].nullable);
    let users = &rels[2];
    assert_eq!(users.estimated_rows, Some(12));
    assert!(
        users
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Check)
    );
    assert!(
        users
            .constraints
            .iter()
            .any(|c| c.kind == ConstraintKind::Unique)
    );
    let email = users.indexes.iter().find(|i| i.name == "email").unwrap();
    assert!(email.unique && !email.primary);
    assert_eq!(
        email.definition,
        "CREATE UNIQUE INDEX `email` ON `users` (`email`(20)) USING BTREE"
    );
}

#[test]
fn parses_mysql_urls() {
    let (p, pw) = parse_database_url("mysql://app:s%40cret@db.example.com:3307/shop").unwrap();
    assert_eq!(p.kind, DbKind::Mysql);
    assert_eq!(
        (
            p.host.as_str(),
            p.port,
            p.database.as_str(),
            p.user.as_str()
        ),
        ("db.example.com", 3307, "shop", "app")
    );
    assert_eq!(pw.as_deref(), Some("s@cret"));
    assert_eq!(p.url(), "mysql://app@db.example.com:3307/shop");
    // The connection string reads back as the same profile.
    let (again, _) = parse_database_url(&p.connection_string()).unwrap();
    assert_eq!(again, p);

    let (p, _) = parse_database_url("mysql://root@localhost").unwrap();
    assert_eq!((p.port, p.database.as_str()), (3306, ""));

    for url in [
        "jdbc:mysql://u:p@h:3306/d?useSSL=false",
        "mysql+pymysql://u:p@h/d?charset=utf8mb4",
        "mariadb://u:p@h/d",
        "DATABASE_URL=\"mysql://u:p@h/d\"",
    ] {
        let (p, _) = parse_database_url(url).unwrap();
        assert_eq!(
            (p.kind, p.host.as_str(), p.database.as_str()),
            (DbKind::Mysql, "h", "d"),
            "{url}"
        );
    }
    let (p, _) = parse_database_url("jdbc:mysql://u:p@h:3306/d?useSSL=false").unwrap();
    assert_eq!(p.ssl, SslMode::Disable);

    // PlanetScale and Prisma.
    let (p, _) = parse_database_url(
        "mysql://u:p@aws.connect.psdb.cloud/d?ssl={\"rejectUnauthorized\":true}",
    )
    .unwrap();
    assert_eq!(p.ssl, SslMode::VerifyFull);
    let (p, _) = parse_database_url("mysql://u:p@h/d?sslaccept=strict").unwrap();
    assert_eq!(p.ssl, SslMode::VerifyFull);
    let (p, _) = parse_database_url("mysql://u:p@h/d?ssl-mode=REQUIRED").unwrap();
    assert_eq!(p.ssl, SslMode::Require);
    assert!(parse_database_url("mysql://u:p@h/d?ssl-mode=SOMETIMES").is_err());

    // Go's DSN.
    let (p, pw) =
        parse_database_url("app:pw@tcp(127.0.0.1:3307)/shop?parseTime=true&tls=skip-verify")
            .unwrap();
    assert_eq!(
        (p.kind, p.host.as_str(), p.port, p.database.as_str(), p.ssl),
        (DbKind::Mysql, "127.0.0.1", 3307, "shop", SslMode::Require)
    );
    assert_eq!(pw.as_deref(), Some("pw"));
    let (p, _) = parse_database_url("root@unix(/tmp/mysql.sock)/shop").unwrap();
    assert!(p.is_socket());
    assert_eq!(
        p.url(),
        "mysql://root@localhost/shop?socket=%2Ftmp%2Fmysql.sock"
    );

    // PostgreSQL still parses as PostgreSQL.
    let (p, _) = parse_database_url("postgres://u@h/d").unwrap();
    assert_eq!(p.kind, DbKind::Postgres);
}

#[test]
fn mysql_types_export_as_numbers() {
    use crate::db::{CellKind, cell_kind};
    assert_eq!(cell_kind("int unsigned"), CellKind::Number);
    assert_eq!(
        cell_kind("decimal(10,2) unsigned zerofill"),
        CellKind::Number
    );
    assert_eq!(cell_kind("bigint"), CellKind::Number);
    assert_eq!(cell_kind("varchar(20)"), CellKind::Text);
}
