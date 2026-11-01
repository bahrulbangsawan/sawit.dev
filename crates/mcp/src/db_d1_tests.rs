//! The database tools on SQLite and Cloudflare D1 targets: a scratch Workers
//! project with a local D1 file where Miniflare would keep it, and a fake
//! `wrangler` in its `node_modules/.bin` for the remote one (never the real
//! one, and nothing remote).

use std::path::{Path, PathBuf};

use rmcp::model::CallToolResult;
use sawit_core::db::d1::{d1_unique_key, durable_object_id};
use sawit_core::instances::AppPaths;

use crate::Sawit;
use crate::db::{ExecuteArgs, SchemaArgs, TableArgs};
use crate::db_admin::{BackupArgs, ExportQueryArgs, ExportTableArgs, TruncateArgs};
use crate::diagrams::{SchemaDiagramArgs, SchemaFormat};

fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

fn failed(result: &CallToolResult) -> bool {
    result.is_error == Some(true)
}

/// A Workers project with D1 binding `DB` (database `shop`) whose local file holds
/// `products`, kept in WAL mode with its `-wal` as workerd leaves it, plus one file no binding
/// names.
fn workers_project(dir: &Path) -> (PathBuf, PathBuf) {
    let project = dir.join("api");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join("wrangler.toml"),
        "name = \"api\"\n[[d1_databases]]\nbinding = \"DB\"\ndatabase_name = \"shop\"\ndatabase_id = \"5d0c3fa0\"\n",
    )
    .unwrap();
    let folder = project.join(".wrangler/state/v3/d1").join(d1_unique_key());
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(format!(
        "{}.sqlite",
        durable_object_id(&d1_unique_key(), "5d0c3fa0")
    ));
    let conn = rusqlite::Connection::open(&file).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT NOT NULL, price REAL);
         CREATE TABLE orders (id INTEGER PRIMARY KEY, product_id INTEGER REFERENCES products (id));
         INSERT INTO products (name, price) VALUES ('Kopi', 3.5), ('Teh', 2.0);",
    )
    .unwrap();
    conn.set_db_config(
        rusqlite::config::DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE,
        true,
    )
    .unwrap();
    drop(conn);
    std::fs::write(folder.join(format!("{}.sqlite", "ab".repeat(32))), b"").unwrap();
    (project, file)
}

fn server(dir: &Path) -> Sawit {
    Sawit::new(AppPaths {
        root: dir.join("home"),
    })
}

fn local(project: &Path) -> String {
    format!("d1-local:DB@{}", project.display())
}

#[tokio::test]
async fn detect_lists_d1_with_kinds_local_first() {
    let dir = tempfile::tempdir().unwrap();
    let (project, file) = workers_project(dir.path());
    let server = server(dir.path());
    let text = said(
        &server
            .detect_databases_tool(Some(&project.display().to_string()))
            .await,
    );
    let local_at = text
        .find(&format!("- {} · kind: d1-local", local(&project)))
        .unwrap_or_else(|| panic!("{text}"));
    let remote_at = text
        .find(&format!(
            "- d1-remote:shop@{} · kind: d1-remote",
            project.display()
        ))
        .unwrap_or_else(|| panic!("{text}"));
    assert!(local_at < remote_at, "{text}");
    assert!(text.contains("binding DB · database shop"), "{text}");
    assert!(text.contains(&crate::tilde(&file)), "{text}");
    assert!(
        text.contains("Unmatched local D1 databases") && text.contains("kind: sqlite"),
        "{text}"
    );
}

#[tokio::test]
async fn reads_describes_and_exports_a_local_d1_database() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _) = workers_project(dir.path());
    let server = server(dir.path());
    let target = local(&project);

    let schema = said(&server.describe_schema_tool(&target, None, None).await);
    assert!(schema.contains("schema main"), "{schema}");
    assert!(schema.contains("Table main.products"), "{schema}");
    assert!(
        schema.contains("FK (product_id) → main.products(id)"),
        "{schema}"
    );

    let table = said(
        &server
            .describe_table_tool(TableArgs {
                target: target.clone(),
                schema: None,
                table: "products".into(),
                database: None,
            })
            .await,
    );
    assert!(
        table.starts_with("table products · 2 rows (Cloudflare D1 (local))"),
        "{table}"
    );
    assert!(table.contains("name TEXT NOT NULL"), "{table}");
    assert!(
        table.contains("orders (product_id) → products(id)"),
        "{table}"
    );

    let rows = said(
        &server
            .query_tool(
                &target,
                "SELECT name FROM products ORDER BY price",
                None,
                None,
            )
            .await,
    );
    assert!(rows.contains("Teh") && rows.contains("(2 rows)"), "{rows}");
    let refused = server
        .query_tool(&target, "DELETE FROM products", None, None)
        .await;
    assert!(failed(&refused), "{}", said(&refused));
    assert!(said(&refused).contains("read-only"), "{}", said(&refused));

    let csv = dir.path().join("products.csv");
    let exported = server
        .export_table_tool(ExportTableArgs {
            target: target.clone(),
            database: None,
            schema: None,
            table: "products".into(),
            path: csv.display().to_string(),
            overwrite: None,
        })
        .await;
    assert!(!failed(&exported), "{}", said(&exported));
    assert_eq!(
        std::fs::read_to_string(&csv).unwrap(),
        "id,name,price\r\n1,Kopi,3.5\r\n2,Teh,2.0\r\n"
    );
    let xlsx = dir.path().join("cheap.xlsx");
    let exported = server
        .export_query_tool(ExportQueryArgs {
            target: target.clone(),
            database: None,
            sql: "SELECT * FROM products WHERE price < 3".into(),
            path: xlsx.display().to_string(),
            overwrite: None,
        })
        .await;
    assert!(
        said(&exported).starts_with("Exported 1 row"),
        "{}",
        said(&exported)
    );

    let diagram = said(
        &server
            .schema_diagram_tool(SchemaDiagramArgs {
                target: target.clone(),
                schema: None,
                database: None,
                format: SchemaFormat::Mermaid,
                output_path: None,
                theme: Default::default(),
            })
            .await,
    );
    assert!(
        diagram.contains("erDiagram") && diagram.contains("products"),
        "{diagram}"
    );

    let listed = said(&server.list_databases_tool(&target).await);
    assert!(listed.contains(&local(&project)), "{listed}");
}

#[tokio::test]
async fn execute_sql_writes_to_the_file_when_no_dev_server_has_it() {
    let dir = tempfile::tempdir().unwrap();
    let (project, file) = workers_project(dir.path());
    let server = server(dir.path());
    let target = local(&project);
    let run = |sql: &str| ExecuteArgs {
        target: target.clone(),
        sql: sql.into(),
        database: None,
        max_rows: None,
        confirm: false,
    };
    let wrote = server
        .execute_sql_tool(run("UPDATE products SET price = 4 WHERE name = 'Kopi'"))
        .await;
    assert!(!failed(&wrote), "{}", said(&wrote));
    assert!(said(&wrote).contains("1 row affected"), "{}", said(&wrote));
    assert!(said(&wrote).contains("straight to"), "{}", said(&wrote));
    // workerd's -wal stays, as it was found.
    assert!(PathBuf::from(format!("{}-wal", file.display())).exists());
    let read = said(
        &server
            .execute_sql_tool(run("SELECT price FROM products WHERE id = 1"))
            .await,
    );
    assert!(read.contains("4.0"), "{read}");
}

#[tokio::test]
async fn postgres_only_tools_refuse_a_d1_target() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _) = workers_project(dir.path());
    let server = server(dir.path());
    let target = local(&project);
    let backup = server
        .backup_tool(BackupArgs {
            target: target.clone(),
            database: None,
            schema: None,
            table: None,
            path: dir.path().join("b.sql").display().to_string(),
            overwrite: None,
        })
        .await;
    let truncate = server
        .truncate_table_tool(TruncateArgs {
            target: target.clone(),
            database: None,
            schema: None,
            table: "products".into(),
            confirm: "products".into(),
        })
        .await;
    for refused in [backup, truncate] {
        assert!(failed(&refused));
        let text = said(&refused);
        assert!(
            text.contains("Cloudflare D1 (local)") && text.contains("PostgreSQL only"),
            "{text}"
        );
    }
    let missing = server
        .describe_schema_tool(&format!("d1-local:NOPE@{}", project.display()), None, None)
        .await;
    assert!(
        said(&missing).contains("no D1 binding “NOPE”"),
        "{}",
        said(&missing)
    );
}

/// A fake `wrangler` in the project's `node_modules/.bin`, logging each call; it answers the
/// schema query with one table and anything else with one changed row.
#[cfg(unix)]
fn fake_wrangler(project: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin = project.join("node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let log = project.join("wrangler.log");
    let script = format!(
        "#!/bin/sh\necho \"$@\" >> '{}'\ncase \"$*\" in\n  *sqlite_master*) echo '[{{\"results\":[{{\"type\":\"table\",\"name\":\"products\",\"sql\":\"CREATE TABLE products (id INTEGER PRIMARY KEY, name TEXT)\"}}],\"success\":true,\"meta\":{{}}}}]' ;;\n  *) echo '[{{\"results\":[],\"success\":true,\"meta\":{{\"changes\":1}}}}]' ;;\nesac\n",
        log.display()
    );
    let path = bin.join("wrangler");
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    log
}

#[cfg(unix)]
#[tokio::test]
async fn remote_writes_need_confirm_and_say_production() {
    let dir = tempfile::tempdir().unwrap();
    let (project, _) = workers_project(dir.path());
    let log = fake_wrangler(&project);
    let server = server(dir.path());
    let target = format!("d1-remote:shop@{}", project.display());
    let run = |confirm| ExecuteArgs {
        target: target.clone(),
        sql: "DELETE FROM products WHERE id = 1".into(),
        database: None,
        max_rows: None,
        confirm,
    };
    let refused = server.execute_sql_tool(run(false)).await;
    assert!(failed(&refused));
    assert!(
        said(&refused).contains("remote (production)"),
        "{}",
        said(&refused)
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        !calls.contains("DELETE"),
        "only the schema was read: {calls}"
    );
    let done = server.execute_sql_tool(run(true)).await;
    assert!(!failed(&done), "{}", said(&done));
    assert!(
        said(&done).contains("remote (production)"),
        "{}",
        said(&done)
    );
    let calls = std::fs::read_to_string(&log).unwrap();
    assert!(
        calls.contains("d1 execute shop --remote --json --yes --command DELETE FROM products"),
        "{calls}"
    );
    // query never runs a write remotely, even unasked.
    let q = server
        .query_tool(&target, "DROP TABLE products", None, None)
        .await;
    assert!(failed(&q));
    assert!(!std::fs::read_to_string(&log).unwrap().contains("DROP"));
    let schema = said(&server.describe_schema_tool(&target, None, None).await);
    assert!(schema.contains("Table main.products"), "{schema}");
}

#[test]
fn schema_args_take_d1_ids() {
    // The arguments the tools above share stay as they were for PostgreSQL targets.
    let a: SchemaArgs =
        serde_json::from_str(r#"{"target": "d1-local:DB@/x", "schema": null}"#).unwrap();
    assert_eq!(a.target, "d1-local:DB@/x");
    let e: ExecuteArgs = serde_json::from_str(r#"{"target": "shop", "sql": "SELECT 1"}"#).unwrap();
    assert!(!e.confirm);
}
