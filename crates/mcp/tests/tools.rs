//! Calls the MCP tools directly (no stdio) against a temp Sawit home.

use sawit_core::binaries::{SearchRoots, discover};
use sawit_core::control::{self, Preflight};
use sawit_core::instances::{AppPaths, Instance, ListenScope, Registry};
use sawit_mcp::Sawit;
use sawit_mcp::db::{CreateDbArgs, DropDbArgs, ExecuteArgs, TableArgs};
use sawit_mcp::diagrams::{SchemaDiagramArgs, SchemaFormat, Theme};

fn text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn list_instances_and_write_env() {
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: home.path().join("pd"),
    };
    let mut reg = Registry::default();
    reg.add(Instance {
        name: "shop".into(),
        bin_dir: "/nonexistent/bin".into(),
        data_dir: paths.cluster_dir("shop"),
        port: 5491,
        listen: ListenScope::Localhost,
        managed: true,
    })
    .unwrap();
    reg.save(&paths.instances_file()).unwrap();
    let server = Sawit::new(paths);

    let out = text(&server.list_instances_tool().await);
    assert!(out.contains("shop"), "{out}");
    assert!(
        out.contains("postgres://postgres@localhost:5491/postgres"),
        "{out}"
    );

    let project = home.path().join("tanstack-app");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".env"),
        "VITE_X=1\nDATABASE_URL=postgres://old\n",
    )
    .unwrap();
    let res = server
        .write_env_tool("shop", project.to_str().unwrap())
        .await;
    assert_ne!(res.is_error, Some(true), "{}", text(&res));
    assert_eq!(
        std::fs::read_to_string(project.join(".env")).unwrap(),
        "VITE_X=1\nDATABASE_URL=postgres://postgres@localhost:5491/postgres\n"
    );

    let missing = server
        .write_env_tool("nope", project.to_str().unwrap())
        .await;
    assert_eq!(missing.is_error, Some(true));
}

#[tokio::test]
async fn query_is_read_only() {
    let roots = SearchRoots::detect("/nonexistent".into()).await;
    let Some(bins) = discover(&roots).await.into_iter().next() else {
        eprintln!("skip: no PostgreSQL binaries");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: home.path().join("pd"),
    };
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let inst = Instance {
        name: "mcp".into(),
        bin_dir: bins.bin_dir,
        data_dir: paths.cluster_dir("mcp"),
        port,
        listen: ListenScope::Localhost,
        managed: true,
    };
    let mut reg = Registry::default();
    reg.add(inst.clone()).unwrap();
    reg.save(&paths.instances_file()).unwrap();
    control::init_cluster(&inst).await.unwrap();
    control::start(&inst, &paths, vec![], Preflight::Force)
        .await
        .unwrap();

    let server = Sawit::new(paths);
    let ok = server
        .query_tool("mcp", "select 41 + 1 as answer", None, None)
        .await;
    assert!(text(&ok).contains("42"), "{}", text(&ok));
    let denied = server
        .query_tool("mcp", "create table t(x int)", None, None)
        .await;
    assert_eq!(denied.is_error, Some(true), "{}", text(&denied));
    // Even single statements a superuser could run must not reach the shell.
    let marker = home.path().join("pwned");
    let sql = format!(
        "DO $$ BEGIN EXECUTE 'COPY (SELECT 1) TO PROGRAM ''touch {}'''; END $$",
        marker.display()
    );
    let attack = server.query_tool("mcp", &sql, None, None).await;
    assert_eq!(attack.is_error, Some(true), "{}", text(&attack));
    assert!(!marker.exists(), "shell command ran via MCP");
    let schema = server.describe_schema_tool("mcp", None, None).await;
    assert!(text(&schema).contains("public"), "{}", text(&schema));

    // execute_sql writes; query then sees the rows.
    let wrote = server
        .execute_sql_tool(ExecuteArgs {
            target: "mcp".into(),
            sql: "create table t(id int primary key, x text not null default 'a'); insert into t(id) values (1), (2)".into(),
            database: None,
            max_rows: None,
        })
        .await;
    assert_ne!(wrote.is_error, Some(true), "{}", text(&wrote));
    assert!(text(&wrote).contains("2 rows affected"), "{}", text(&wrote));
    let count = server
        .query_tool("mcp", "select count(*) as n from t", None, None)
        .await;
    assert!(text(&count).contains("2"), "{}", text(&count));
    let table = server
        .describe_table_tool(TableArgs {
            target: "mcp".into(),
            schema: None,
            table: "t".into(),
            database: None,
        })
        .await;
    let table = text(&table);
    assert!(table.contains("id integer NOT NULL"), "{table}");
    assert!(table.contains("PRIMARY KEY"), "{table}");
    let diagram = server
        .schema_diagram_tool(SchemaDiagramArgs {
            target: "mcp".into(),
            schema: None,
            database: None,
            format: SchemaFormat::Mermaid,
            output_path: None,
            theme: Theme::Light,
        })
        .await;
    assert!(text(&diagram).contains("erDiagram"), "{}", text(&diagram));
    let svg_path = home.path().join("schema.svg");
    let svg = server
        .schema_diagram_tool(SchemaDiagramArgs {
            target: "mcp".into(),
            schema: Some("public".into()),
            database: None,
            format: SchemaFormat::Svg,
            output_path: Some(svg_path.display().to_string()),
            theme: Theme::Dark,
        })
        .await;
    assert_ne!(svg.is_error, Some(true), "{}", text(&svg));
    assert!(std::fs::read_to_string(&svg_path).unwrap().contains("<svg"));

    // Databases: create, address by `database`, drop only when confirmed.
    let created = server
        .create_database_tool(CreateDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            owner: None,
            template: None,
        })
        .await;
    assert_ne!(created.is_error, Some(true), "{}", text(&created));
    let listed = text(&server.list_databases_tool("mcp").await);
    assert!(listed.contains("- other db"), "{listed}");
    let current = server
        .query_tool("mcp", "select current_database()", Some("other db"), None)
        .await;
    assert!(text(&current).contains("other db"), "{}", text(&current));
    let refused = server
        .drop_database_tool(DropDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            confirm: "other".into(),
        })
        .await;
    assert_eq!(refused.is_error, Some(true));
    let dropped = server
        .drop_database_tool(DropDbArgs {
            target: "mcp".into(),
            name: "other db".into(),
            confirm: "other db".into(),
        })
        .await;
    assert_ne!(dropped.is_error, Some(true), "{}", text(&dropped));
    assert!(!text(&server.list_databases_tool("mcp").await).contains("other db"));

    control::stop(&inst).await.unwrap();
}
