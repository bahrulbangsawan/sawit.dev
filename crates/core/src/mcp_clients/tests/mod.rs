//! Tests for installing MCP servers into AI clients, one file per topic, and the fake home
//! folder they share.

use std::fs;
use std::path::Path;

use serde_json::Value;

use super::*;

mod cli;
mod discovery;
mod files;
mod github;
mod shapes;
mod updates;

fn home() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    let h = d.path();
    fs::create_dir_all(h.join(".claude")).unwrap();
    fs::write(
        h.join(".claude.json"),
        r#"{"numStartups": 3, "projects": {}}"#,
    )
    .unwrap();
    for p in ["work", "personal"] {
        fs::create_dir_all(h.join(format!(".claude-{p}"))).unwrap();
        fs::write(h.join(format!(".claude-{p}/.claude.json")), "{}").unwrap();
    }
    // A helper folder, not a profile.
    fs::create_dir_all(h.join(".claude-pool")).unwrap();
    fs::create_dir_all(h.join(".codex")).unwrap();
    fs::create_dir_all(h.join("Library/Application Support/Claude")).unwrap();
    fs::create_dir_all(h.join(".config/zed")).unwrap();
    d
}

fn find<'a>(targets: &'a [Target], client: Client, profile: &str) -> &'a Target {
    targets
        .iter()
        .find(|t| t.client == client && t.profile == profile)
        .unwrap_or_else(|| panic!("{client:?}/{profile} not found in {targets:#?}"))
}

fn json_at(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
}
