//! Importing from Claude Pool and removing it, against a made-up Claude Pool in a temp home.

use std::path::Path;

use serde_json::json;

use crate::agent_accounts::import::PoolPaths;
use crate::agent_accounts::{Accounts, AgentKind, import, read_active, shell, statusline, usage};

const A: &str = "11111111-1111-4111-8111-111111111111";
const B: &str = "22222222-2222-4222-8222-222222222222";

/// A Claude Pool as this Mac has it: two profiles (tracked, the status line on its collector),
/// usage, a layout, the shell block and an active pointer.
fn pool(home: &Path) {
    let support = home.join("Library/Application Support/ClaudePool");
    std::fs::create_dir_all(&support).unwrap();
    std::fs::write(
        support.join("profiles.json"),
        json!([
            {"id": A, "name": "1", "slug": "1", "configDir": home.join(".claude"), "colorHex": "D97757", "isDefault": true, "trackingEnabled": true, "sortIndex": 0, "createdAt": "2026-10-01T10:00:00Z"},
            {"id": B, "name": "Work", "slug": "work", "configDir": home.join(".claude-2"), "colorHex": "#4F8EF7", "isDefault": false, "trackingEnabled": true, "originalStatusLine": "{\"type\":\"command\",\"command\":\"printf old\"}", "sortIndex": 1, "createdAt": "2026-10-02T10:00:00Z"}
        ])
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        support.join("settings.json"),
        json!({"shareExtensions": true, "limitAlertThreshold": 85, "collectorOutput": "summary"})
            .to_string(),
    )
    .unwrap();
    let ph = home.join(".claude-pool");
    std::fs::create_dir_all(ph.join("usage")).unwrap();
    std::fs::create_dir_all(ph.join("statusline")).unwrap();
    std::fs::write(ph.join(format!("usage/{B}.json")), json!({"schema": 1, "profileId": B, "lastSeenAt": 1.0, "observedAt": 1.0, "fiveHour": {"usedPercentage": 23.0, "resetsAt": 9e9}}).to_string()).unwrap();
    std::fs::write(
        ph.join(format!("usage/{B}.history.jsonl")),
        "{\"t\":1,\"fh\":23,\"fhr\":9000000000}\n",
    )
    .unwrap();
    std::fs::write(ph.join("statusline/config.json"), json!({"schema": 1, "theme": "nord", "lines": [[{"id": "x", "kind": "model", "bold": true, "showLabel": false}]]}).to_string()).unwrap();
    std::fs::write(
        ph.join(format!("statusline/profile-{B}.json")),
        json!({"managed": true, "profileName": "Work"}).to_string(),
    )
    .unwrap();
    std::fs::write(
        ph.join("active"),
        home.join(".claude-2").to_string_lossy().as_bytes(),
    )
    .unwrap();
    for (dir, id) in [(".claude", A), (".claude-2", B)] {
        std::fs::create_dir_all(home.join(dir)).unwrap();
        std::fs::write(
            home.join(dir).join("settings.json"),
            json!({"model": "opus", "statusLine": {"type": "command", "command": format!("'{}/bin/claude-pool-collector' --profile {id}", ph.display()), "padding": 0}}).to_string(),
        )
        .unwrap();
    }
    std::fs::write(
        home.join(".zshrc"),
        "export A=1\n# >>> claude-pool >>>\nclaude() { command claude; }\n# <<< claude-pool <<<\n",
    )
    .unwrap();
}

#[test]
fn claude_pool_is_imported_copied_and_repointed() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    pool(&home);
    assert!(import::found(&home).is_some());
    let plan = import::plan(&home, &root, "/bin/zsh").unwrap();
    assert_eq!(plan.accounts.len(), 2);
    assert_eq!(plan.accounts[1].name, "work");
    assert_eq!(
        plan.accounts[1].original_status_line.as_ref().unwrap()["command"],
        "printf old"
    );
    assert_eq!(plan.repoints.len(), 2);
    assert!(plan.summary().iter().any(|l| l.contains(".zshrc")));
    // Planning writes nothing.
    assert!(!root.exists());

    let done = import::apply(&plan, &root, 1_793_961_302).unwrap();
    assert!(!done.is_empty());
    let all = Accounts::load(&root).unwrap();
    assert_eq!(all.accounts.len(), 2);
    assert!(all.settings.share_extensions);
    assert_eq!(all.settings.limit_threshold, 85);
    assert_eq!(
        read_active(&root, AgentKind::ClaudeCode),
        Some(home.join(".claude-2"))
    );
    assert_eq!(
        usage::load_snapshot(&root, B)
            .unwrap()
            .five_hour
            .unwrap()
            .used_percentage,
        23.0
    );
    assert_eq!(usage::load_history(&root, B).len(), 1);
    assert!(statusline::load_account(&root, B).unwrap().managed);
    assert_eq!(statusline::load_config(&root).unwrap().theme, "nord");
    let s: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(home.join(".claude-2/settings.json")).unwrap(),
    )
    .unwrap();
    assert!(
        s["statusLine"]["command"]
            .as_str()
            .unwrap()
            .contains("sawit-statusline --account 2222")
    );
    assert_eq!(s["statusLine"]["padding"], 0);
    assert_eq!(s["model"], "opus");
    let zshrc = std::fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(zshrc.contains(shell::BEGIN) && !zshrc.contains("claude-pool"));
    // Claude Pool's own files are untouched (copied, not moved).
    assert!(home.join(format!(".claude-pool/usage/{B}.json")).is_file());
    // Backups of what changed.
    assert!(root.join(format!("accounts/backups/{B}")).is_dir());
    assert!(root.join("accounts/backups/shell").is_dir());
}

#[test]
fn claude_pool_goes_only_after_a_verified_backup_and_never_while_needed() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    pool(&home);
    let plan = import::plan(&home, &root, "/bin/zsh").unwrap();
    let accounts: Vec<_> = plan.accounts.iter().collect();
    // Before the import: the status lines and the shell block still need Claude Pool.
    // Only the test's own folders: never /Applications or /opt/homebrew/bin.
    let paths = PoolPaths {
        app: home.join("Applications/ClaudePool.app"),
        ..PoolPaths::of(&home)
    };
    let bin = home.join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(
        "/Applications/ClaudePool.app/Contents/MacOS/ClaudePool",
        bin.join("claude-pool"),
    )
    .unwrap();
    std::fs::write(
        bin.join("cp3"),
        "#!/bin/sh
",
    )
    .unwrap();
    let dirs = vec![bin.clone()];
    let r = import::removal(
        &home,
        &paths,
        &dirs,
        &root,
        &accounts,
        "/bin/zsh",
        true,
        1_793_961_302,
    );
    assert_eq!(r.blockers.len(), 4, "{:?}", r.blockers);
    assert!(import::remove(&r, &home).is_err());
    assert!(home.join(".claude-pool").is_dir());

    import::apply(&plan, &root, 1_793_961_302).unwrap();
    let r = import::removal(
        &home,
        &paths,
        &dirs,
        &root,
        &accounts,
        "/bin/zsh",
        false,
        1_793_961_400,
    );
    assert!(r.blockers.is_empty(), "{:?}", r.blockers);
    assert_eq!(r.folders.len(), 2);
    assert_eq!(
        r.links,
        [bin.join("claude-pool")],
        "only links to Claude Pool's app"
    );
    import::remove(&r, &home).unwrap();
    assert!(!home.join(".claude-pool").exists());
    assert!(!home.join("Library/Application Support/ClaudePool").exists());
    let list = std::process::Command::new("/usr/bin/tar")
        .arg("-tzf")
        .arg(&r.archive)
        .output()
        .unwrap();
    let names = String::from_utf8_lossy(&list.stdout);
    assert!(names.contains(".claude-pool/usage/") && names.contains("ClaudePool/profiles.json"));
    use std::os::unix::fs::PermissionsExt as _;
    assert_eq!(
        std::fs::metadata(&r.archive).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(bin.join("cp3").is_file(), "anything else stays");
    // The accounts' folders are untouched.
    assert!(home.join(".claude-2/settings.json").is_file());
}

/// A home that isn't this user's own (a test's, an e2e run's) sees no Mac-wide Claude Pool: not
/// /Applications/ClaudePool.app, not /opt/homebrew/bin.
#[test]
fn another_home_never_sees_the_macs_claude_pool() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    assert!(!import::is_users_home(&home));
    assert_eq!(
        PoolPaths::of(&home).app,
        home.join("Applications/ClaudePool.app")
    );
    assert_eq!(import::link_dirs(&home), [home.join(".local/bin")]);
    assert!(import::found(&home).is_none());
}

/// An accounts file that can't be read is left as it is, not replaced by the imported accounts.
#[test]
fn import_never_saves_over_an_unreadable_accounts_file() {
    let tmp = tempfile::tempdir().unwrap();
    let file = Accounts::path(tmp.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{ not json").unwrap();
    let plan = import::Plan {
        accounts: vec![],
        settings: Default::default(),
        copies: vec![],
        repoints: vec![],
        active: None,
        shell_file: None,
    };
    assert!(import::apply(&plan, tmp.path(), 1_793_961_302).is_err());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ not json");
}
