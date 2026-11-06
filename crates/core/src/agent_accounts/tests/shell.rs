//! The shell block, the cp1–cp9 shortcuts and `sawit account`: in temp folders only.

use std::path::Path;

use super::account;
use crate::agent_accounts::shell::{self, BlockState, LinkState};
use crate::agent_accounts::{Accounts, AgentKind, cli, read_active};

const POOL_BLOCK: &str = "# >>> claude-pool >>>\n# Managed by Claude Pool.\nclaude() { command claude \"$@\"; }\n# <<< claude-pool <<<\n";

/// The block goes where Claude Pool's was (replacing it), else at the end; it is updated in
/// place, taken out cleanly, and a half-edited block is refused rather than guessed at.
#[test]
fn the_shell_block_replaces_claude_pools_and_comes_out_cleanly() {
    let root = Path::new("/tmp/sawit data");
    let before = format!("export A=1\n{POOL_BLOCK}alias g=git\n");
    let with = shell::with_block(&before, root).unwrap();
    assert!(!with.contains("claude-pool"));
    assert!(with.starts_with("export A=1\n# >>> sawit-accounts >>>"));
    assert!(with.ends_with("# <<< sawit-accounts <<<\nalias g=git\n"));
    assert!(with.contains("'/tmp/sawit data/accounts/active-claude_code'"));
    assert_eq!(shell::state(&with, root), BlockState::Installed);
    assert_eq!(
        shell::with_block(&with, root).unwrap(),
        with,
        "installing again changes nothing"
    );
    let other_root = Path::new("/elsewhere");
    assert_eq!(shell::state(&with, other_root), BlockState::Outdated);
    assert_eq!(
        shell::without_block(&with).unwrap(),
        "export A=1\nalias g=git\n"
    );

    let plain = shell::with_block("export A=1", root).unwrap();
    assert!(plain.starts_with("export A=1\n\n# >>> sawit-accounts >>>"));
    assert_eq!(shell::state("x", root), BlockState::Absent);
    let broken = "# >>> sawit-accounts >>>\nclaude() {}\n";
    assert_eq!(shell::state(broken, root), BlockState::Unbalanced);
    assert!(shell::with_block(broken, root).is_err());
    assert!(shell::with_block("# >>> claude-pool >>>\n", root).is_err());
}

/// The block really runs `claude` with the active folder, and leaves an explicit one alone.
#[test]
fn the_shell_block_starts_claude_as_the_active_account() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("sawit");
    let bin = tmp.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("claude"),
        "#!/bin/sh\necho \"dir=${CLAUDE_CONFIG_DIR-unset}\"\n",
    )
    .unwrap();
    std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let work = tmp.path().join(".claude-work");
    std::fs::create_dir_all(&work).unwrap();
    crate::agent_accounts::write_active(&root, &account("work", &work, false)).unwrap();
    let script = format!(
        "{}\nclaude\nCLAUDE_CONFIG_DIR=/x claude\n",
        shell::snippet(&root)
    );
    let out = std::process::Command::new("/bin/zsh")
        .args(["-f", "-c", &script])
        .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text, format!("dir={}\ndir=/x\n", work.display()));
}

/// The cp1–cp9 links: free names and Claude Pool's links become links to Sawit (Claude Pool's
/// targets written down first); anything else of that name stays; removal takes only Sawit's.
#[test]
fn shortcuts_replace_only_claude_pools_links() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, dir) = (tmp.path().join("sawit"), tmp.path().join("bin"));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = tmp.path().join("Sawit.app/Contents/MacOS/sawit");
    std::os::unix::fs::symlink(
        "/Applications/ClaudePool.app/Contents/MacOS/ClaudePool",
        dir.join("cp1"),
    )
    .unwrap();
    std::fs::write(dir.join("cp2"), "#!/bin/sh\n").unwrap();
    std::os::unix::fs::symlink("/usr/bin/true", dir.join("cp3")).unwrap();
    let (linked, skipped) = shell::install_shortcuts(&root, &dir, &exe, 1_793_961_302).unwrap();
    assert_eq!(skipped, ["cp2", "cp3"]);
    assert_eq!(linked.len(), 7);
    assert_eq!(shell::link_state(&dir.join("cp1"), &exe), LinkState::Ours);
    let noted = std::fs::read_to_string(root.join("accounts/backups/links-2026-11-06T103502Z.txt"))
        .unwrap();
    assert!(noted.contains("ClaudePool.app"));
    assert_eq!(shell::remove_shortcuts(&dir, &exe).unwrap(), 7);
    assert!(dir.join("cp2").exists() && dir.join("cp3").symlink_metadata().is_ok());
    assert_eq!(shell::shortcut_number("cp4"), Some(4));
    assert_eq!(shell::shortcut_number("cp0"), None);
    assert_eq!(shell::shortcut_number("cp10"), None);
}

#[test]
fn the_command_line_adds_uses_renames_and_removes() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let mut all = Accounts::default();
    all.add(account("default", &home.join(".claude"), true))
        .unwrap();
    all.save(&root).unwrap();
    assert_eq!(
        cli::run_for_test(&["add", "Side Project"], &root, &home),
        Ok(0)
    );
    assert!(home.join(".claude-side-project").is_dir());
    assert_eq!(cli::run_for_test(&["use", "side"], &root, &home), Ok(0));
    assert_eq!(
        read_active(&root, AgentKind::ClaudeCode),
        Some(home.join(".claude-side-project"))
    );
    assert_eq!(cli::run_for_test(&["use", "1"], &root, &home), Ok(0));
    assert_eq!(read_active(&root, AgentKind::ClaudeCode), None);
    assert_eq!(
        cli::run_for_test(&["rename", "2", "Client"], &root, &home),
        Ok(0)
    );
    assert!(cli::run_for_test(&["use", "nobody"], &root, &home).is_err());
    assert_eq!(
        cli::run_for_test(&["remove", "client", "--yes"], &root, &home),
        Ok(0)
    );
    assert_eq!(Accounts::load(&root).unwrap().accounts.len(), 1);
    assert!(
        home.join(".claude-side-project").is_dir(),
        "the folder stays"
    );
    assert!(cli::run_for_test(&["frobnicate"], &root, &home).is_err());
}
