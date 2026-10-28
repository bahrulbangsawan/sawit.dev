//! Calls the MCP tools directly (no stdio) against a temp PG Desk home.

use pgdesk_core::binaries::{SearchRoots, discover};
use pgdesk_core::control::{self, Preflight};
use pgdesk_core::instances::{AppPaths, Instance, ListenScope, Registry};
use pgdesk_mcp::PgDesk;

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
    let server = PgDesk::new(paths);

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

    let server = PgDesk::new(paths);
    let ok = server
        .query_tool("mcp", "select 41 + 1 as answer", None)
        .await;
    assert!(text(&ok).contains("42"), "{}", text(&ok));
    let denied = server
        .query_tool("mcp", "create table t(x int)", None)
        .await;
    assert_eq!(denied.is_error, Some(true), "{}", text(&denied));
    // Even single statements a superuser could run must not reach the shell.
    let marker = home.path().join("pwned");
    let sql = format!(
        "DO $$ BEGIN EXECUTE 'COPY (SELECT 1) TO PROGRAM ''touch {}'''; END $$",
        marker.display()
    );
    let attack = server.query_tool("mcp", &sql, None).await;
    assert_eq!(attack.is_error, Some(true), "{}", text(&attack));
    assert!(!marker.exists(), "shell command ran via MCP");
    let schema = server.describe_schema_tool("mcp").await;
    assert!(text(&schema).contains("public"), "{}", text(&schema));

    control::stop(&inst).await.unwrap();
}
