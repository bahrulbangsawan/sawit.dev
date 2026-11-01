//! The database tools on a scratch Sawit home: write_env and saved connections ask before they
//! replace or delete, and the SQL and text they make.

use std::time::Duration;

use sawit_core::db::{QueryOutcome, ResultSet, parse_database_url};

use super::detect::english_via;
use super::sql::{create_database_sql, format_outcome};
use super::*;

/// MCP-7: write_env reads `project_dir` as list_scripts and save_command read theirs:
/// trimmed, `~` for the home folder, and absolute. A space at its end made it "not a folder",
/// and a relative one was written in whatever folder sawit-mcp ran in.
#[tokio::test]
async fn write_env_reads_its_folder_as_every_tool_does() {
    use sawit_core::instances::{AppPaths, ListenScope};
    let dir = tempfile::tempdir().unwrap();
    let server = Sawit::new(AppPaths {
        root: dir.path().join("home"),
    });
    let mut registry = Registry::default();
    let shop = Instance {
        name: "shop".into(),
        bin_dir: dir.path().join("bin"),
        data_dir: dir.path().join("data"),
        port: 54329,
        listen: ListenScope::Localhost,
        managed: true,
    };
    registry.add(shop).unwrap();
    registry.save(&server.paths.instances_file()).unwrap();
    let said = |result: &CallToolResult| {
        let texts = result.content.iter().filter_map(|c| c.as_text());
        texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
    };

    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let written = server
        .write_env_tool("shop", &format!(" {} ", project.display()), false)
        .await;
    assert_ne!(written.is_error, Some(true), "{}", said(&written));
    let env = std::fs::read_to_string(project.join(".env")).unwrap();
    assert!(
        env.contains("DATABASE_URL=postgres://postgres@localhost:54329/postgres"),
        "{env}"
    );

    let refused = server.write_env_tool("shop", "no-such-folder", false).await;
    assert_eq!(refused.is_error, Some(true));
    assert!(
        said(&refused).contains("must be an absolute path"),
        "{}",
        said(&refused)
    );
}

/// A server for a scratch data folder with one instance, `shop` on port
/// 54329.
fn shop_server(dir: &std::path::Path) -> Sawit {
    use sawit_core::instances::{AppPaths, ListenScope};
    let server = Sawit::new(AppPaths {
        root: dir.join("home"),
    });
    let mut registry = Registry::default();
    registry
        .add(Instance {
            name: "shop".into(),
            bin_dir: dir.join("bin"),
            data_dir: dir.join("data"),
            port: 54329,
            listen: ListenScope::Localhost,
            managed: true,
        })
        .unwrap();
    registry.save(&server.paths.instances_file()).unwrap();
    server
}

fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

/// write_env never replaces another database's URL unasked, and says
/// which one it would replace without the password in it.
#[tokio::test]
async fn write_env_asks_before_replacing_another_database_url() {
    let dir = tempfile::tempdir().unwrap();
    let server = shop_server(dir.path());
    let project = dir.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    let env = project.join(".env");
    let theirs = "API=1\nDATABASE_URL=postgres://app:hunter2@db.example.com:5432/app\n";
    std::fs::write(&env, theirs).unwrap();
    let folder = project.display().to_string();

    let refused = server.write_env_tool("shop", &folder, false).await;
    assert_eq!(refused.is_error, Some(true));
    let text = said(&refused);
    assert!(
        text.contains("postgres://app@db.example.com:5432/app"),
        "{text}"
    );
    assert!(text.contains("confirm=true"), "{text}");
    assert!(!text.contains("hunter2"), "{text}");
    assert_eq!(std::fs::read_to_string(&env).unwrap(), theirs);

    let replaced = server.write_env_tool("shop", &folder, true).await;
    assert_ne!(replaced.is_error, Some(true), "{}", said(&replaced));
    let now = std::fs::read_to_string(&env).unwrap();
    assert_eq!(
        now,
        "API=1\nDATABASE_URL=postgres://postgres@localhost:54329/postgres\n"
    );
    // The same URL again replaces nothing, so it needs no yes.
    let again = server.write_env_tool("shop", &folder, false).await;
    assert_ne!(again.is_error, Some(true), "{}", said(&again));
    // Nor does a value that doesn't read as a URL show up in the refusal.
    std::fs::write(&env, "DATABASE_URL=mysql://root:s3cret@db/app\n").unwrap();
    let refused = server.write_env_tool("shop", &folder, false).await;
    assert!(!said(&refused).contains("s3cret"), "{}", said(&refused));
    // A .env that can't be read is never written, even with confirm: read as empty, it was
    // replaced by the one DATABASE_URL line.
    let unreadable = b"API=1\nNAME=caf\xe9\n";
    std::fs::write(&env, unreadable).unwrap();
    for confirm in [false, true] {
        let refused = server.write_env_tool("shop", &folder, confirm).await;
        assert_eq!(refused.is_error, Some(true), "{}", said(&refused));
        assert!(
            said(&refused).contains("can't be read"),
            "{}",
            said(&refused)
        );
        assert_eq!(std::fs::read(&env).unwrap(), unreadable);
    }
}

/// Removing a saved connection needs its name again; a refusal returns
/// before the Keychain is touched.
#[tokio::test]
async fn removing_a_connection_needs_its_name_again() {
    let dir = tempfile::tempdir().unwrap();
    let server = shop_server(dir.path());
    let (mut profile, _) = parse_database_url("postgres://app@db.example.com:5432/app").unwrap();
    profile.name = "prod".into();
    let mut profiles = Profiles::default();
    profiles.upsert(profile);
    profiles.save(&server.profiles_file()).unwrap();
    for confirm in ["", "Prod"] {
        let refused = server.remove_connection_tool("prod", confirm).await;
        assert_eq!(refused.is_error, Some(true));
        assert!(
            said(&refused).contains("confirm=\"prod\""),
            "{}",
            said(&refused)
        );
    }
    assert_eq!(server.profiles().unwrap().items.len(), 1);
}

#[test]
fn confirm_must_match_exactly() {
    assert!(check_confirm("shop", "shop", "drop database").is_ok());
    let e = check_confirm("shop", "Shop", "drop database").unwrap_err();
    assert!(e.contains("confirm=\"shop\""), "{e}");
    assert!(check_confirm("shop", "", "drop database").is_err());
}

#[test]
fn create_database_quotes_identifiers() {
    assert_eq!(
        create_database_sql("shop", None, None),
        "CREATE DATABASE \"shop\""
    );
    assert_eq!(
        create_database_sql("a\"b", Some("app"), Some("template0")),
        "CREATE DATABASE \"a\"\"b\" OWNER \"app\" TEMPLATE \"template0\""
    );
    assert_eq!(
        create_database_sql("x", Some(" "), None),
        "CREATE DATABASE \"x\""
    );
}

#[test]
fn outcome_lists_tables_then_affected_rows() {
    let outcome = QueryOutcome {
        results: vec![ResultSet {
            columns: vec!["id".into()],
            rows: vec![vec![Some("1".into())]],
            truncated: false,
        }],
        affected: vec![3],
        duration: Duration::from_millis(7),
    };
    let text = format_outcome(&outcome);
    assert!(text.contains("id\n--\n1\n(1 row)"), "{text}");
    assert!(text.contains("3 rows affected"));
    assert!(text.ends_with("(7 ms)"));
    let many = QueryOutcome {
        affected: vec![1, 0],
        ..QueryOutcome::default()
    };
    let text = format_outcome(&many);
    assert!(text.contains("statement 1: 1 row affected\nstatement 2: 0 rows affected"));
}

#[test]
fn via_labels_in_english() {
    assert_eq!(
        english_via("koneksi tersimpan prod"),
        "saved connection prod"
    );
    assert_eq!(english_via(".env di ~/code/app"), ".env in ~/code/app");
    assert_eq!(english_via("instance shop"), "instance shop");
}
