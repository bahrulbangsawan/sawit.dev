//! The catalog is well formed, and the named sources install the way their own docs say.

use std::path::Path;

use super::*;
use crate::package_manager::PackageManager;

#[test]
fn the_catalog_is_well_formed() {
    let mut ids: Vec<&str> = PACKS.iter().map(|p| p.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), PACKS.len());
    for pack in PACKS {
        assert!(
            !pack.skills.is_empty() && !pack.methods.is_empty(),
            "{}",
            pack.id
        );
        assert!(pack.home.starts_with("https://"), "{}", pack.id);
        assert!(pack.repo.starts_with("https://github.com/"), "{}", pack.id);
        // Skill names go into commands and paths.
        for skill in pack.skills {
            assert!(
                skill
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{skill}"
            );
        }
        assert_eq!(get(pack.id), Some(pack));
        // A built-in pack's skills are each one Sawit ships: the folders it copies. Their names
        // are Sawit's own (another publisher's folder of a name is theirs), and each SKILL.md
        // gives its name, by which an agent's copy counts as Sawit's.
        if pack.methods.contains(&Method::Bundled) {
            assert_eq!(pack.bundled(), Some(pack.skills), "{}", pack.id);
            for name in pack.skills {
                assert!(bundled::files(name).is_some(), "{name} isn't built in");
                assert!(*name == "sawit" || name.starts_with("sawit-"), "{name}");
                let skill = bundled::text(name, "SKILL.md").unwrap();
                assert_eq!(bundled::skill_name(skill), Some(*name));
            }
        } else {
            assert_eq!(pack.bundled(), None, "{}", pack.id);
        }
    }
}

/// Canvas Design, Sawit's own design standard, is the first Design pack: built in, by Sawit, from
/// its folder in Sawit's repository, and it says it installs this Sawit's version, offline. Its
/// skill isn't `canvas-design`, the name of Anthropic's skill.
#[test]
fn canvas_design_ships_inside_sawit() {
    let pack = get("sawit-canvas-design").unwrap();
    assert_eq!(get("canvas-design"), None);
    let design: Vec<&str> = PACKS
        .iter()
        .filter(|p| p.category == Category::Design)
        .map(|p| p.id)
        .collect();
    assert_eq!(design[0], "sawit-canvas-design");
    assert_eq!((pack.name, pack.by), ("Canvas Design", "Sawit"));
    assert_eq!(pack.skills, ["sawit-canvas-design"]);
    assert_eq!(pack.methods, [Method::Bundled]);
    assert_eq!(pack.bundled(), Some(&["sawit-canvas-design"][..]));
    let folder =
        "https://github.com/bahrulbangsawan/sawit.dev/tree/main/skills/sawit-canvas-design";
    assert_eq!((pack.home, pack.repo), (folder, folder));
    let note = pack.note.unwrap();
    assert!(
        note.contains("inside Sawit") && note.contains("offline"),
        "{note}"
    );
    assert!(note.contains("installs this Sawit's version"), "{note}");
    assert!(pack.about.contains("checklist"), "{}", pack.about);
}

/// The seven sources the user named are there, each installing the way its own docs say.
#[test]
fn the_named_sources_install_their_documented_way() {
    let methods = |id: &str| get(id).unwrap().methods.to_vec();
    assert_eq!(
        methods("interfaces"),
        [Method::SkillsCli {
            source: "jakubkrehel/skills",
            all: true
        }]
    );
    assert!(
        matches!(methods("unslop")[0], Method::Copy { url, path: "skills" } if url == "https://github.com/heydamir/unslop")
    );
    assert_eq!(
        get("animate")
            .unwrap()
            .plugin()
            .unwrap()
            .plugin_key()
            .as_deref(),
        Some("animate@animate")
    );
    assert_eq!(
        get("superpowers")
            .unwrap()
            .plugin()
            .unwrap()
            .plugin_key()
            .as_deref(),
        Some("superpowers@claude-plugins-official")
    );
    assert!(get("mcp-apps").unwrap().skills.contains(&"create-mcp-app"));
    assert_eq!(get("cf-cli").unwrap().skills, ["cf-cli"]);
    assert!(get("cloudflare").unwrap().skills.contains(&"cf-cli"));
    // shadcn/improve: its README's `npx skills add shadcn/improve` first, then the plugin its
    // marketplace.json names.
    let improve = get("improve").unwrap();
    assert_eq!(improve.skills, ["improve"]);
    assert_eq!(improve.category, Category::Workflow);
    assert_eq!(
        improve.methods[0],
        Method::SkillsCli {
            source: "shadcn/improve",
            all: true
        }
    );
    assert_eq!(
        improve.plugin().unwrap().plugin_key().as_deref(),
        Some("improve@improve")
    );
}

/// improve installs with the skills CLI into every agent picked, or as its plugin into a
/// Claude Code profile; it counts as installed from its folder, a link to one, or its user
/// plugin, and removing it takes out only its own folder and plugin.
#[test]
fn improve_installs_with_the_skills_cli_or_its_plugin() {
    let pack = get("improve").unwrap();
    let (cli, plugin) = (pack.methods[0], pack.methods[1]);
    let none = Installed::default();
    let (main, shared) = (claude(None), folder(".agents/skills", Some("codex")));
    assert_eq!(
        script(
            pack,
            cli,
            Action::Install,
            &[(&main, &none), (&shared, &none)],
            PackageManager::Npm,
            &env()
        )
        .unwrap(),
        ["npx -y skills add shadcn/improve --skill '*' -g -a claude-code -a codex -y"]
    );
    assert_eq!(
        sample(pack, cli, PackageManager::Pnpm, &env()),
        "pnpm dlx skills add shadcn/improve --skill '*' -g -a <agent> -y"
    );
    assert_eq!(
        script(
            pack,
            plugin,
            Action::Install,
            &[(&main, &none)],
            PackageManager::Npm,
            &env()
        )
        .unwrap(),
        [
            "claude plugin marketplace add shadcn/improve",
            "claude plugin install improve@improve",
        ]
    );
    let refused = script(
        pack,
        plugin,
        Action::Install,
        &[(&shared, &none)],
        PackageManager::Npm,
        &env(),
    )
    .unwrap_err();
    assert!(refused.contains("only into Claude Code"), "{refused}");

    // Installed: from the shared folder (the skills CLI's copy), through a link in Claude
    // Code's folder, and from the plugin's install path.
    let dir = tempfile::tempdir().unwrap();
    let h = dir.path();
    let skill_md = |root: &Path| {
        std::fs::create_dir_all(root.join("improve")).unwrap();
        std::fs::write(root.join("improve/SKILL.md"), "---\nname: improve\n---\n").unwrap();
    };
    skill_md(&h.join(".agents/skills"));
    let shared_dir = Target {
        dir: h.join(".agents/skills"),
        ..shared.clone()
    };
    assert_eq!(
        status(pack, &read_installed(&shared_dir)),
        Status::Installed { plugin: false }
    );
    let claude_dir = h.join(".claude");
    std::fs::create_dir_all(claude_dir.join("skills")).unwrap();
    std::os::unix::fs::symlink(
        h.join(".agents/skills/improve"),
        claude_dir.join("skills/improve"),
    )
    .unwrap();
    let claude_target = Target {
        dir: claude_dir.join("skills"),
        claude: Some(ClaudeHome {
            dir: claude_dir.clone(),
            profile_dir: None,
        }),
        ..claude(None)
    };
    assert_eq!(
        status(pack, &read_installed(&claude_target)),
        Status::Installed { plugin: false }
    );
    std::fs::remove_file(claude_dir.join("skills/improve")).unwrap();
    assert_eq!(
        status(pack, &read_installed(&claude_target)),
        Status::Missing
    );
    let cache = h.join(".claude/plugins/cache/improve/improve/1.0.0");
    skill_md(&cache.join("skills"));
    std::fs::write(
        claude_dir.join("plugins/installed_plugins.json"),
        serde_json::json!({"version": 2, "plugins": {
            "improve@improve": [{"scope": "user", "installPath": cache}]
        }})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        claude_dir.join("plugins/known_marketplaces.json"),
        r#"{"improve": {}}"#,
    )
    .unwrap();
    let installed = read_installed(&claude_target);
    assert!(installed.from_plugins.contains("improve"));
    assert_eq!(status(pack, &installed), Status::Installed { plugin: true });
    // An update reads its marketplace again and updates the plugin.
    assert_eq!(
        script(
            pack,
            plugin,
            Action::Update,
            &[(&claude_target, &installed)],
            PackageManager::Npm,
            &env()
        )
        .unwrap(),
        [
            "claude plugin marketplace update improve",
            "claude plugin update improve@improve",
        ]
    );
    // Removing: the plugin where it's installed, the folder where it's a folder.
    let env_h = Env {
        home: h.to_path_buf(),
        ..env()
    };
    assert_eq!(
        script(
            pack,
            cli,
            Action::Remove,
            &[(&claude_target, &installed)],
            PackageManager::Npm,
            &env_h
        )
        .unwrap(),
        ["claude plugin uninstall improve@improve"]
    );
    assert_eq!(
        script(
            pack,
            cli,
            Action::Remove,
            &[(&shared_dir, &read_installed(&shared_dir))],
            PackageManager::Npm,
            &env_h
        )
        .unwrap(),
        ["rm -rf \"$HOME/.agents/skills/improve\""]
    );
}
