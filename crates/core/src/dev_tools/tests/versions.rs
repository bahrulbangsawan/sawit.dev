//! Versions: release tags, Homebrew casks, comparing, and what an upgrade runs.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::dev_tools::versions::{cask_version, tag_version};

use super::*;

/// Homebrew's API names the version `brew upgrade --cask` installs.
#[test]
fn cask_versions() {
    assert_eq!(
        cask_version(r#"{"token":"puremac","version":"3.2.0","auto_updates":null}"#).as_deref(),
        Some("3.2.0")
    );
    assert_eq!(
        cask_version(r#"{"version":"1.2.3,456"}"#).as_deref(),
        Some("1.2.3")
    );
    assert_eq!(cask_version(r#"{"version":"latest"}"#), None);
    assert_eq!(cask_version(r#"{"token":"x"}"#), None);
    assert_eq!(cask_version("not json"), None);
    assert!(is_newer("3.3.0", "3.2.0"));
}

/// An app installed outside what upgrades it shows its version, but never an upgrade: not in
/// `newer` (the page's Update badge and Upgrade All, list_dev_tools, `sawit list`), and not
/// in `upgrade_plan`, whether every tool is upgraded or it's named (upgrade_dev_tools,
/// `sawit pupuk puremac`), even when its latest release is unknown.
#[test]
fn an_app_installed_outside_is_never_upgraded() {
    let at = |path: &str, v: &str, outside: Option<Outside>| Found {
        path: PathBuf::from(path),
        version: Some(v.into()),
        outside,
    };
    let dmg = Some(Outside::Homebrew);
    let script = Some(Outside::InstallScript);
    let mut found = HashMap::from([
        ("puremac", at("/Applications/PureMac.app", "3.1.0", dmg)),
        ("zed", at("/Applications/Zed.app", "0.200.0", script)),
        ("cap", at("/Applications/Cap.app", "1.0.0", script)),
        ("bun", at("/Users/me/.bun/bin/bun", "1.0.0", None)),
    ]);
    let latest = HashMap::from([
        ("puremac", "3.2.0".to_string()),
        ("zed", "0.210.0".to_string()),
        ("bun", "1.1.0".to_string()),
    ]);
    let ids = |p: Vec<&Tool>| p.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(super::newer("puremac", &found, &latest), None);
    assert_eq!(super::newer("zed", &found, &latest), None);
    assert_eq!(ids(upgrade_plan(&[], &found, &latest)), ["bun"]);
    assert_eq!(
        ids(upgrade_plan(
            &["puremac", "zed", "cap", "bun"],
            &found,
            &latest
        )),
        ["bun"]
    );
    // Homebrew's copy, and Zed's script's: upgraded.
    found.get_mut("puremac").unwrap().outside = None;
    found.get_mut("zed").unwrap().outside = None;
    assert_eq!(super::newer("puremac", &found, &latest), Some("3.2.0"));
    assert_eq!(
        ids(upgrade_plan(&[], &found, &latest)),
        ["bun", "zed", "puremac"]
    );
    // Cap publishes no latest release: named, it runs its script again, as before.
    found.get_mut("cap").unwrap().outside = None;
    assert_eq!(ids(upgrade_plan(&["cap"], &found, &latest)), ["cap"]);
}

#[test]
fn tags() {
    assert_eq!(tag_version("v2.9.8").as_deref(), Some("2.9.8"));
    assert_eq!(tag_version("bun-v1.4.4").as_deref(), Some("1.4.4"));
    assert_eq!(tag_version("7.0.11").as_deref(), Some("7.0.11"));
    assert_eq!(tag_version("nightly"), None);
}

#[test]
fn upgrades() {
    let at = |v: &str| Found {
        path: PathBuf::from("/x"),
        version: Some(v.into()),
        outside: None,
    };
    // bun is behind, uv is current, deno publishes nothing readable.
    let found = HashMap::from([
        ("bun", at("1.0.0")),
        ("uv", at("0.5.0")),
        ("deno", at("2.0")),
    ]);
    let latest = HashMap::from([("bun", "1.1.0".to_string()), ("uv", "0.5.0".to_string())]);
    assert_eq!(super::newer("bun", &found, &latest), Some("1.1.0"));
    assert_eq!(super::newer("uv", &found, &latest), None);
    assert_eq!(super::newer("pnpm", &found, &latest), None);
    let ids = |p: Vec<&Tool>| p.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(upgrade_plan(&[], &found, &latest)), ["bun"]);
    // Named: current ones are skipped, unknown latest still runs, missing ones never.
    assert_eq!(
        ids(upgrade_plan(
            &["uv", "deno", "pnpm", "bun"],
            &found,
            &latest
        )),
        ["bun", "deno"]
    );
}

#[test]
fn newer() {
    assert!(is_newer("1.3.0", "1.2.9"));
    assert!(is_newer("1.10.0", "1.9.0"));
    assert!(is_newer("2.0", "1.99.1"));
    assert!(is_newer("1.2.3.1", "1.2.3"));
    assert!(!is_newer("1.2.3", "1.2.3"));
    assert!(!is_newer("1.2", "1.2.0"));
    assert!(!is_newer("1.2.3", "1.3.0"));
    assert!(!is_newer("1.3.0-beta.1", "1.3.0"));
    assert!(is_newer("v1.3.0-rc.1", "1.2.0"));
    assert!(!is_newer(
        "0.0.1793260836-gdb4c2d",
        "0.0.1793260836-gdb4c2d"
    ));
    assert!(!is_newer("nightly", "1.0.0"));
}

/// Hits the network: `cargo test -p sawit-core dev_tools -- --ignored`.
#[tokio::test]
#[ignore]
async fn latest_lookups() {
    let latest = latest_versions().await;
    for id in ["bun", "deno", "pnpm", "claude-code", "puremac"] {
        let v = latest
            .get(id)
            .unwrap_or_else(|| panic!("no latest for {id}"));
        assert!(v.starts_with(|c: char| c.is_ascii_digit()), "{id}: {v}");
    }
}
