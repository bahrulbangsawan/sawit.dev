//! Shared settings across accounts: links, three-way merges, removals and backups, in a temp
//! home with made-up accounts.

use std::path::Path;

use serde_json::{Value, json};

use super::account;
use crate::agent_accounts::shared;

fn write(path: &Path, v: Value) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Skills move to the default account and are linked into the others; an MCP server, a plugin,
/// a hook and a permission added in one account reach all; one removed goes everywhere; the
/// sign-in-related keys and statusLine stay per account; each changed file is backed up.
#[test]
fn shared_settings_follow_every_account() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    let (d, w) = (home.join(".claude"), home.join(".claude-work"));
    std::fs::create_dir_all(d.join("skills/review")).unwrap();
    std::fs::create_dir_all(w.join("skills/deploy")).unwrap();
    std::fs::create_dir_all(w.join("skills/synced")).unwrap();
    std::fs::write(w.join("CLAUDE.md"), "be brief").unwrap();
    write(
        &home.join(".claude.json"),
        json!({"oauthAccount": {"emailAddress": "a@x"}, "mcpServers": {"neon": {"url": "https://n"}}}),
    );
    write(
        &w.join(".claude.json"),
        json!({"oauthAccount": {"emailAddress": "b@x"}, "mcpServers": {"linear": {"url": "https://l"}}}),
    );
    write(
        &d.join("settings.json"),
        json!({"statusLine": {"command": "mine"}, "permissions": {"allow": ["Bash(ls)"]}}),
    );
    write(
        &w.join("settings.json"),
        json!({"theme": "light", "enabledPlugins": {"p@m": true}, "hooks": {"Stop": [{"hooks": [{"command": "say done"}]}]}}),
    );
    let (da, wa) = (account("default", &d, true), account("work", &w, false));

    let report = shared::sync(&root, &home, &[&wa, &da], 1_793_961_302).unwrap();
    assert!(!report.changes.is_empty());
    // Skills: work's moved to the default and linked back; the default's linked into work.
    assert!(d.join("skills/deploy").is_dir() && !d.join("skills/deploy").is_symlink());
    assert!(w.join("skills/deploy").is_symlink() && w.join("skills/review").is_symlink());
    assert!(
        !w.join("skills/synced").is_symlink(),
        "synced stays per account"
    );
    assert!(d.join("CLAUDE.md").is_file() && w.join("CLAUDE.md").is_symlink());
    // Keyed entries everywhere; per-account keys untouched.
    for g in [home.join(".claude.json"), w.join(".claude.json")] {
        let v = read(&g);
        assert!(v["mcpServers"]["neon"].is_object() && v["mcpServers"]["linear"].is_object());
    }
    assert_eq!(
        read(&w.join(".claude.json"))["oauthAccount"]["emailAddress"],
        "b@x"
    );
    let (ds, ws) = (
        read(&d.join("settings.json")),
        read(&w.join("settings.json")),
    );
    assert_eq!(ds["enabledPlugins"]["p@m"], true);
    assert_eq!(ws["permissions"]["allow"], json!(["Bash(ls)"]));
    assert_eq!(ds["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
    assert_eq!(ds["statusLine"]["command"], "mine");
    assert!(ws.get("statusLine").is_none() && ws["theme"] == "light");
    let backups = root.join("accounts/backups/shared-sync");
    assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 1);

    // A run that finds the copies on disk counts them as held (one a session wrote back over
    // the sync's is never read as a removal before then).
    shared::sync(&root, &home, &[&da, &wa], 1_793_961_350).unwrap();
    // Removed in one account that held it: gone from all.
    let mut g = read(&w.join(".claude.json"));
    g["mcpServers"].as_object_mut().unwrap().remove("neon");
    write(&w.join(".claude.json"), g);
    let report = shared::sync(&root, &home, &[&da, &wa], 1_793_961_400).unwrap();
    assert!(report.changes.iter().any(|c| c.contains("Removed neon")));
    assert!(
        read(&home.join(".claude.json"))["mcpServers"]
            .get("neon")
            .is_none()
    );
    // Nothing new: nothing written.
    assert!(
        shared::sync(&root, &home, &[&da, &wa], 1_793_961_500)
            .unwrap()
            .changes
            .is_empty()
    );
}

#[test]
fn a_broken_file_stops_the_merge_and_one_account_does_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    let (d, w) = (home.join(".claude"), home.join(".claude-work"));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::create_dir_all(&w).unwrap();
    std::fs::write(w.join(".claude.json"), "{not json").unwrap();
    let (da, wa) = (account("default", &d, true), account("work", &w, false));
    assert!(shared::sync(&root, &home, &[&da, &wa], 0).is_err());
    assert_eq!(
        std::fs::read_to_string(w.join(".claude.json")).unwrap(),
        "{not json"
    );
    assert!(
        shared::sync(&root, &home, &[&da], 0)
            .unwrap()
            .changes
            .is_empty()
    );
}
