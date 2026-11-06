//! The agents' skills folders, what each has on disk, and finding them by name.

use std::collections::HashSet;
use std::path::Path;

use super::*;

/// Every Claude Code profile has its own folder; agents that read the same folder share one
/// target; clients without skills have none.
#[test]
fn targets_follow_the_agents_on_this_mac() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    std::fs::create_dir_all(h.join(".codex/skills")).unwrap();
    let clients = vec![
        client(Client::ClaudeCode, None),
        mcp_clients::Target {
            profile: "work".into(),
            profile_dir: Some(h.join(".claude-work")),
            ..client(Client::ClaudeCode, None)
        },
        client(Client::Codex, None),
        client(Client::Zed, None),
        client(Client::Cursor, None),
        client(Client::ClaudeDesktop, None),
    ];
    let targets = targets_for(h, clients);
    let labels: Vec<&str> = targets.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "Claude Code",
            "Claude Code · work",
            "Codex, Zed",
            "Cursor",
            "Codex (older folder)"
        ]
    );
    assert_eq!(targets[0].dir, h.join(".claude/skills"));
    assert_eq!(targets[1].dir, h.join(".claude-work/skills"));
    assert_eq!(
        targets[1].claude.as_ref().unwrap().profile_dir.as_deref(),
        Some(h.join(".claude-work").as_path())
    );
    assert_eq!(targets[2].dir, h.join(".agents/skills"));
    assert_eq!(targets[2].cli_agent, Some("codex"));
    assert_eq!(targets[3].cli_agent, Some("cursor"));
    // The skills CLI doesn't write Codex's older folder; a copy can.
    assert_eq!(targets[4].cli_agent, None);
}

/// A pack is installed in a folder that has all its skills (links followed), partly when it
/// has some, and in a Claude Code profile whose user plugins include it.
#[test]
fn what_a_folder_has_is_read_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let skill = |root: &Path, name: &str| {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("SKILL.md"), "---\nname: x\n---\n").unwrap();
    };
    let claude_dir = h.join(".claude");
    skill(&h.join(".agents/skills"), "cf-cli");
    std::fs::create_dir_all(claude_dir.join("skills")).unwrap();
    std::os::unix::fs::symlink(
        h.join(".agents/skills/cf-cli"),
        claude_dir.join("skills/cf-cli"),
    )
    .unwrap();
    // A folder without SKILL.md isn't a skill.
    std::fs::create_dir_all(claude_dir.join("skills/agents-sdk")).unwrap();
    let cache = h.join("cache/superpowers/7.0.0");
    skill(&cache.join("skills"), "brainstorming");
    std::fs::create_dir_all(claude_dir.join("plugins")).unwrap();
    std::fs::write(
        claude_dir.join("plugins/installed_plugins.json"),
        serde_json::json!({"version": 2, "plugins": {
            "superpowers@claude-plugins-official": [{"scope": "user", "installPath": cache}],
            "animate@animate": [{"scope": "project", "projectPath": "/p", "installPath": "/nope"}]
        }})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        claude_dir.join("plugins/known_marketplaces.json"),
        r#"{"claude-plugins-official": {}, "animate": {}}"#,
    )
    .unwrap();
    let target = Target {
        dir: claude_dir.join("skills"),
        claude: Some(ClaudeHome {
            dir: claude_dir.clone(),
            profile_dir: None,
        }),
        ..claude(None)
    };
    let installed = read_installed(&target);
    assert_eq!(installed.folder, HashSet::from(["cf-cli".to_string()]));
    assert!(installed.from_plugins.contains("brainstorming"));
    assert_eq!(
        installed.plugins,
        HashSet::from(["superpowers@claude-plugins-official".to_string()])
    );
    assert!(installed.marketplaces.contains("animate"));
    let status = |id| status(get(id).unwrap(), &installed);
    assert_eq!(status("cf-cli"), Status::Installed { plugin: false });
    assert_eq!(status("superpowers"), Status::Installed { plugin: true });
    // Installed for one project only.
    assert_eq!(status("animate"), Status::Missing);
    assert_eq!(status("cloudflare"), Status::Partial { have: 1, of: 19 });
}

/// Factory Droid shares ~/.agents/skills, where the skills CLI installs it (`-a droid` with
/// `-g`); its own ~/.factory/skills, which it reads too, is a target of its own only when it
/// exists, so skills there (copies, older installs) are still found and removed.
#[test]
fn factory_droid_shares_the_agents_folder_and_keeps_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let clients = || {
        vec![
            client(Client::ClaudeCode, None),
            client(Client::FactoryDroid, None),
            client(Client::Codex, None),
        ]
    };
    let targets = targets_for(h, clients());
    let labels: Vec<&str> = targets.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels, ["Claude Code", "Codex, Factory Droid"]);
    assert_eq!(targets[1].dir, h.join(".agents/skills"));
    // The CLI writes the shared folder for either name.
    assert_eq!(targets[1].cli_agent, Some("codex"));
    // Droid alone: the shared folder with Droid's own name for the CLI.
    let alone = targets_for(h, vec![client(Client::FactoryDroid, None)]);
    assert_eq!(
        (alone[0].label.as_str(), alone[0].cli_agent),
        ("Factory Droid", Some("droid"))
    );
    // Its own folder, once there.
    let skill = h.join(".factory/skills/design-to-code");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), "---\nname: design-to-code\n---\n").unwrap();
    let targets = targets_for(h, clients());
    let own = targets.last().unwrap();
    assert_eq!(own.label, "Factory Droid (own folder)");
    assert_eq!(
        (own.dir.clone(), own.cli_agent),
        (h.join(".factory/skills"), None)
    );
    let installed = read_installed(own);
    let pack = get("design-to-code").unwrap();
    assert_eq!(
        status(pack, &installed),
        Status::Installed { plugin: false }
    );
    // Removing takes it out of that folder; installing there copies (the CLI can't).
    let env = Env {
        home: h.to_path_buf(),
        ..env()
    };
    assert_eq!(
        script(
            pack,
            pack.methods[0],
            Action::Remove,
            &[(own, &installed)],
            crate::package_manager::PackageManager::Npm,
            &env
        )
        .unwrap(),
        ["rm -rf \"$HOME/.factory/skills/design-to-code\""]
    );
    assert!(
        script(
            pack,
            pack.methods[0],
            Action::Install,
            &[(own, &Installed::default())],
            crate::package_manager::PackageManager::Npm,
            &env
        )
        .is_err()
    );
    assert!(
        script(
            pack,
            pack.methods[1],
            Action::Install,
            &[(own, &Installed::default())],
            crate::package_manager::PackageManager::Npm,
            &env
        )
        .is_ok()
    );
    // Without Droid, the folder isn't offered.
    let without = targets_for(h, vec![client(Client::ClaudeCode, None)]);
    assert!(without.iter().all(|t| t.dir != h.join(".factory/skills")));
}

#[test]
fn agents_are_found_by_label_agent_or_folder() {
    let mut work = claude(Some("/Users/me/.claude-work"));
    work.label = "Claude Code · work".into();
    let mut shared = folder(".agents/skills", Some("codex"));
    shared.label = "Codex, Zed".into();
    shared.clients = vec![Client::Codex, Client::Zed];
    let targets = [claude(None), work, shared];
    let find = |names: &[&str]| {
        find_targets(
            &targets,
            &names.iter().map(|n| n.to_string()).collect::<Vec<_>>(),
        )
    };
    assert_eq!(find(&["Claude Code"]), Ok(vec![0]));
    assert_eq!(find(&["claude-code-work", "zed"]), Ok(vec![1, 2]));
    assert_eq!(find(&["Claude Code (profile work)"]), Ok(vec![1]));
    assert_eq!(find(&["/Users/me/.agents/skills", "codex"]), Ok(vec![2]));
    let unknown = find(&["cursor"]).unwrap_err();
    assert!(unknown.contains("Claude Code · work"), "{unknown}");
}
