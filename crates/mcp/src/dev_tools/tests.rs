//! The developer tool tools refuse before anything runs, and say how each tool is upgraded and
//! removed.

use std::collections::HashMap;

use rmcp::model::CallToolResult;
use sawit_core::dev_tools::{Removal, TOOLS};

use super::install::not_upgraded;
use super::list::{uninstall_line, upgrade_line};
use super::*;

#[test]
fn unknown_ids_are_reported() {
    let first = TOOLS[0].id.to_string();
    assert!(unknown(std::slice::from_ref(&first)).is_empty());
    assert_eq!(unknown(&[first, "nope".into()]), ["nope"]);
}

#[test]
fn upgrade_args() {
    let a: UpgradeToolsArgs = serde_json::from_str(r#"{"confirm":true}"#).unwrap();
    assert!(a.tools.is_empty() && a.confirm);
    let a: UpgradeToolsArgs = serde_json::from_str(r#"{"tools":["bun"],"confirm":false}"#).unwrap();
    assert_eq!(a.tools, ["bun"]);
}

#[tokio::test]
async fn upgrade_needs_confirm() {
    let r = Sawit::new(sawit_core::instances::AppPaths {
        root: std::env::temp_dir(),
    })
    .upgrade_dev_tools_tool(UpgradeToolsArgs {
        tools: vec![],
        confirm: false,
    })
    .await;
    assert_eq!(r.is_error, Some(true));
}

fn said(result: &CallToolResult) -> String {
    let texts = result.content.iter().filter_map(|c| c.as_text());
    texts.map(|t| t.text.clone()).collect::<Vec<_>>().join("\n")
}

#[test]
fn uninstall_args_need_confirm() {
    let a: UninstallToolsArgs =
        serde_json::from_str(r#"{"tools":["bun"],"confirm":true}"#).unwrap();
    assert_eq!(a.tools, ["bun"]);
    assert!(a.confirm);
    assert!(serde_json::from_str::<UninstallToolsArgs>(r#"{"tools":["bun"]}"#).is_err());
}

/// Nothing is detected, let alone run, until the agent confirms and names
/// tools of the catalog.
#[tokio::test]
async fn uninstall_refuses_before_anything_runs() {
    let sawit = Sawit::new(sawit_core::instances::AppPaths {
        root: std::env::temp_dir(),
    });
    let refused = |tools: &[&str], confirm: bool| {
        let tools = tools.iter().map(|t| t.to_string()).collect();
        sawit.uninstall_dev_tools_tool(UninstallToolsArgs { tools, confirm })
    };
    let r = refused(&["bun"], false).await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("confirm=true"), "{}", said(&r));
    let r = refused(&[], true).await;
    assert_eq!(r.is_error, Some(true));
    assert!(said(&r).contains("Name the tools"), "{}", said(&r));
    let r = refused(&["bun", "sawit-palsu"], true).await;
    assert_eq!(r.is_error, Some(true));
    assert!(
        said(&r).contains("Unknown tool sawit-palsu"),
        "{}",
        said(&r)
    );
}

/// list_dev_tools tells an agent what uninstall_dev_tools would run for an
/// installed tool, and what would stop working, before it asks the user.
#[test]
fn the_list_says_how_each_tool_is_removed() {
    let tool = |id: &str| dev_tools::find(id).unwrap();
    let line = |id: &str, removal: Removal, needed_by: Vec<&'static Tool>| {
        uninstall_line(&dev_tools::Uninstall {
            tool: tool(id),
            found: Some("/nonexistent-sawit-test/bin/x".into()),
            removal,
            needed_by,
        })
    };
    assert_eq!(
        line(
            "nvm",
            Removal::Run(r#"rm -rf "$NVM_DIR""#.into()),
            vec![tool("node")]
        ),
        "\n    uninstall: rm -rf \"$NVM_DIR\" (uninstall_dev_tools); Node.js needs it and will stop working"
    );
    // Each line of a longer uninstaller under the first.
    assert_eq!(
        line(
            "uv",
            Removal::Run("uv cache clean\nrm -f uv".into()),
            vec![]
        ),
        "\n    uninstall: uv cache clean\n      rm -f uv (uninstall_dev_tools)"
    );
    assert!(
        line("homebrew", Removal::Terminal("uninstall.sh".into()), vec![])
            .contains("asks for a password: run it in a terminal")
    );
    assert!(
        line("python", Removal::LeftAlone, vec![]).contains("Sawit leaves it alone"),
        "{}",
        line("python", Removal::LeftAlone, vec![])
    );
    assert_eq!(
        line("npm", Removal::Bundled(vec!["Node.js"]), vec![]),
        "\n    uninstall: none: it comes with Node.js, and goes when that is uninstalled"
    );
    assert_eq!(line("bun", Removal::Missing, vec![]), "");
    assert_eq!(
        needs_it(&[tool("npm"), tool("gemini")]),
        "npm (latest), Gemini CLI need it and will stop working"
    );
    // Removing Homebrew: its cask's app keeps working, without Homebrew's updates.
    assert_eq!(
        line(
            "homebrew",
            Removal::Terminal("uninstall.sh".into()),
            vec![tool("puremac")]
        ),
        "\n    uninstall: uninstall.sh (asks for a password: run it in a terminal); PureMac won't get updates through Homebrew any more"
    );
}

/// An app installed outside what upgrades it gets no upgrade from list_dev_tools or
/// upgrade_dev_tools, which say why, as the page does; Homebrew's copy upgrades with brew.
#[test]
fn an_app_installed_outside_homebrew_is_never_upgraded() {
    use dev_tools::{Found, Outside};
    let puremac = dev_tools::find("puremac").unwrap();
    let mut found = HashMap::from([(
        "puremac",
        Found {
            path: "/Applications/PureMac.app".into(),
            version: Some("3.1.0".into()),
            outside: Some(Outside::Homebrew),
        },
    )]);
    let latest = HashMap::from([("puremac", "3.2.0".to_string())]);
    assert!(dev_tools::upgrade_plan(&["puremac"], &found, &latest).is_empty());
    assert_eq!(
        upgrade_line(puremac, &found["puremac"]),
        "none: installed outside Homebrew, so Sawit leaves this copy alone"
    );
    assert_eq!(
        not_upgraded("puremac", &found),
        "- puremac: installed outside Homebrew (/Applications/PureMac.app), so Sawit leaves it alone: not upgraded"
    );
    assert_eq!(
        not_upgraded("bun", &found),
        "- bun: not installed (install_dev_tools)"
    );
    found.get_mut("puremac").unwrap().outside = None;
    assert_eq!(
        upgrade_line(puremac, &found["puremac"]),
        "brew upgrade --cask puremac"
    );
    assert_eq!(
        not_upgraded("puremac", &found),
        "- puremac: already the latest"
    );
    assert_eq!(
        dev_tools::upgrade_plan(&["puremac"], &found, &latest)
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        ["puremac"]
    );
}
