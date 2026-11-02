//! Finding the files: by agent, highest priority first, and each profile's own.

use super::*;

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "{}\n").unwrap();
}

#[test]
fn lists_existing_files_by_agent_highest_priority_first() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("root");
    let project = dir.path().join("app");
    touch(&home.join(".claude/settings.json"));
    touch(&home.join(".claude.json"));
    touch(&home.join(".claude-work/settings.json"));
    touch(&home.join(".claude-work/.claude.json"));
    touch(&home.join(".codex/config.toml"));
    touch(&root.join("Library/Application Support/ClaudeCode/managed-settings.json"));
    touch(&project.join(".claude/settings.local.json"));
    touch(&project.join("CLAUDE.md"));
    touch(&project.join("AGENTS.md"));
    // Gemini isn't installed, so its project file is left out.
    touch(&project.join("GEMINI.md"));

    let found = detect_in(&home, &root, std::slice::from_ref(&project));
    let got: Vec<_> = found
        .iter()
        .map(|c| (c.client, c.layer, c.path.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (
                Client::ClaudeCode,
                Layer::Managed,
                root.join("Library/Application Support/ClaudeCode/managed-settings.json")
            ),
            (
                Client::ClaudeCode,
                Layer::ProjectLocal,
                project.join(".claude/settings.local.json")
            ),
            (
                Client::ClaudeCode,
                Layer::Project,
                project.join("CLAUDE.md")
            ),
            (
                Client::ClaudeCode,
                Layer::User,
                home.join(".claude/settings.json")
            ),
            (Client::ClaudeCode, Layer::Global, home.join(".claude.json")),
            // The work profile's own files, after the default's.
            (
                Client::ClaudeCode,
                Layer::User,
                home.join(".claude-work/settings.json")
            ),
            (
                Client::ClaudeCode,
                Layer::Global,
                home.join(".claude-work/.claude.json")
            ),
            (Client::Codex, Layer::Project, project.join("AGENTS.md")),
            (Client::Codex, Layer::User, home.join(".codex/config.toml")),
        ]
    );
    assert_eq!(found[1].project.as_ref(), Some(&project));
}

/// Profiles are the folders Claude Code or Codex ran in (they hold its state file), each
/// its own set of files, not one more layer of the default's; a profile outside the home
/// folder (`CLAUDE_CONFIG_DIR`) is one too.
#[test]
fn profiles_are_their_own_and_only_real_ones() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let root = dir.path().join("root");
    touch(&home.join(".claude/settings.json"));
    touch(&home.join(".claude.json"));
    touch(&home.join(".claude-work/settings.json"));
    touch(&home.join(".claude-work/.claude.json"));
    // A helper folder that happens to share the prefix: not a profile.
    touch(&home.join(".claude-pool/settings.json"));
    let elsewhere = dir.path().join("team/claude");
    touch(&elsewhere.join("settings.json"));
    touch(&elsewhere.join(".claude.json"));

    let found = detect_with(
        &home,
        &root,
        &[],
        &[(Client::ClaudeCode, elsewhere.clone())],
    );
    let got: Vec<_> = found
        .iter()
        .map(|c| (c.profile.as_deref(), c.path.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            (None, home.join(".claude/settings.json")),
            (None, home.join(".claude.json")),
            (Some("work"), home.join(".claude-work/settings.json")),
            (Some("work"), home.join(".claude-work/.claude.json")),
            (Some("claude"), elsewhere.join("settings.json")),
            (Some("claude"), elsewhere.join(".claude.json")),
        ]
    );
    // The panel's groups, as `list_config_files` and `sawit list config` show them too.
    let titles: Vec<(String, usize)> = groups(&found)
        .into_iter()
        .map(|(client, profile, files)| (group_title(client, profile), files.len()))
        .collect();
    assert_eq!(
        titles,
        [
            ("Claude Code".to_string(), 2),
            ("Claude Code · work".to_string(), 2),
            ("Claude Code · claude".to_string(), 2),
        ]
    );
}
