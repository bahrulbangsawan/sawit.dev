//! The commands each way of installing writes, removing, and that every script is valid shell.

use std::collections::HashSet;
use std::path::Path;

use super::*;
use crate::package_manager::PackageManager;

/// The skills CLI runs with the package manager picked, globally, for the agents picked, without
/// questions; each other Claude Code profile gets its own run with its CLAUDE_CONFIG_DIR.
#[test]
fn the_skills_cli_runs_with_the_package_manager_picked() {
    let none = Installed::default();
    let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
    let (shared, cursor) = (
        folder(".agents/skills", Some("codex")),
        folder(".cursor/skills", Some("cursor")),
    );
    let pack = get("interfaces").unwrap();
    let method = pack.methods[0];
    let lines =
        |pm, places: &[Place]| script(pack, method, Action::Install, places, pm, &env()).unwrap();
    assert_eq!(
        lines(PackageManager::Npm, &[(&main, &none), (&shared, &none)]),
        ["npx -y skills add jakubkrehel/skills --skill '*' -g -a claude-code -a codex -y"]
    );
    assert_eq!(
        lines(
            PackageManager::Bun,
            &[(&main, &none), (&work, &none), (&cursor, &none)]
        ),
        [
            "bunx skills add jakubkrehel/skills --skill '*' -g -a claude-code -a cursor -y",
            "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" bunx skills add jakubkrehel/skills --skill '*' -g -a claude-code -y",
        ]
    );
    assert_eq!(
        lines(PackageManager::Pnpm, &[(&shared, &none)]),
        ["pnpm dlx skills add jakubkrehel/skills --skill '*' -g -a codex -y"]
    );
    // One skill of a bigger source, by name.
    let cf = get("cf-cli").unwrap();
    assert_eq!(
        script(
            cf,
            cf.methods[0],
            Action::Install,
            &[(&shared, &none)],
            PackageManager::Yarn,
            &env()
        )
        .unwrap(),
        ["yarn dlx skills add cloudflare/skills --skill cf-cli -g -a codex -y"]
    );
    // The app started with CLAUDE_CONFIG_DIR: the default profile's run unsets it.
    let set = Env {
        claude_config_dir_set: true,
        ..env()
    };
    assert_eq!(
        script(
            pack,
            method,
            Action::Install,
            &[(&main, &none)],
            PackageManager::Npm,
            &set
        )
        .unwrap(),
        [
            "env -u CLAUDE_CONFIG_DIR npx -y skills add jakubkrehel/skills --skill '*' -g -a claude-code -y"
        ]
    );
}

/// A plugin's marketplace is added once per profile, under its own name, and read again
/// before an update; a plugin installed already is updated; a plugin can't go into another
/// agent's folder.
#[test]
fn a_plugin_installs_into_each_claude_code_profile() {
    let none = Installed::default();
    let known = Installed {
        marketplaces: HashSet::from(["mcp-apps".to_string()]),
        plugins: HashSet::from(["mcp-apps@mcp-apps".to_string()]),
        ..Default::default()
    };
    let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
    let pack = get("mcp-apps").unwrap();
    let plugin = pack.plugin().unwrap();
    let run = |action, places: &[Place]| {
        script(pack, plugin, action, places, PackageManager::Npm, &env())
    };
    assert_eq!(
        run(Action::Install, &[(&main, &none)]).unwrap(),
        [
            "claude plugin marketplace add modelcontextprotocol/ext-apps",
            "claude plugin install mcp-apps@mcp-apps",
        ]
    );
    assert_eq!(
        run(Action::Update, &[(&work, &known)]).unwrap(),
        [
            "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" claude plugin marketplace update mcp-apps",
            "CLAUDE_CONFIG_DIR=\"$HOME/.claude-work\" claude plugin update mcp-apps@mcp-apps",
        ]
    );
    let cursor = folder(".cursor/skills", Some("cursor"));
    let refused = run(Action::Install, &[(&cursor, &none)]).unwrap_err();
    assert!(refused.contains("only into Claude Code"), "{refused}");
    assert!(run(Action::Install, &[]).is_err());
    // A method the pack doesn't have.
    let other = get("superpowers").unwrap().methods[0];
    assert!(
        script(
            pack,
            other,
            Action::Install,
            &[(&main, &none)],
            PackageManager::Npm,
            &env()
        )
        .is_err()
    );
}

/// The documented clone and copy, into every folder picked, replacing what was there.
#[test]
fn a_copy_clones_once_and_copies_into_each_folder() {
    let none = Installed::default();
    let pack = get("unslop").unwrap();
    let shared = folder(".agents/skills", Some("codex"));
    let lines = script(
        pack,
        pack.methods[0],
        Action::Install,
        &[(&claude(None), &none), (&shared, &none)],
        PackageManager::Npm,
        &env(),
    )
    .unwrap();
    assert_eq!(
        lines,
        [
            "tmp=\"$(mktemp -d)\"",
            "trap 'rm -rf \"$tmp\"' EXIT",
            "git clone --depth 1 https://github.com/heydamir/unslop \"$tmp/unslop\"",
            "mkdir -p \"$HOME/.claude/skills\"",
            "rm -rf \"$HOME/.claude/skills/unslop\"",
            "cp -R \"$tmp/unslop/skills/unslop\" \"$HOME/.claude/skills\"/",
            "mkdir -p \"$HOME/.agents/skills\"",
            "rm -rf \"$HOME/.agents/skills/unslop\"",
            "cp -R \"$tmp/unslop/skills/unslop\" \"$HOME/.agents/skills\"/",
        ]
    );
}

/// Removing deletes only the pack's own skill folders where they are, and uninstalls its plugin
/// where it's installed; with nothing there it says so instead.
#[test]
fn removing_deletes_only_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env {
        home: dir.path().to_path_buf(),
        claude_config_dir_set: false,
    };
    let mut main = claude(None);
    main.dir = dir.path().join(".claude/skills");
    let has = Installed {
        folder: HashSet::from(["create-mcp-app".to_string(), "unrelated".to_string()]),
        plugins: HashSet::from(["mcp-apps@mcp-apps".to_string()]),
        ..Default::default()
    };
    let pack = get("mcp-apps").unwrap();
    let lines = script(
        pack,
        pack.methods[0],
        Action::Remove,
        &[(&main, &has)],
        PackageManager::Npm,
        &env,
    )
    .unwrap();
    assert_eq!(
        lines,
        [
            "claude plugin uninstall mcp-apps@mcp-apps",
            "rm -rf \"$HOME/.claude/skills/create-mcp-app\"",
        ]
    );
    let nothing = script(
        pack,
        pack.methods[0],
        Action::Remove,
        &[(&main, &Installed::default())],
        PackageManager::Npm,
        &env,
    )
    .unwrap_err();
    assert!(nothing.contains("nothing to remove"), "{nothing}");
    // cf-cli there only through the Cloudflare plugin: removing cf-cli says to remove that.
    let through_plugin = Installed {
        from_plugins: HashSet::from(["cf-cli".to_string()]),
        plugins: HashSet::from(["cloudflare@cloudflare".to_string()]),
        ..Default::default()
    };
    let cf = get("cf-cli").unwrap();
    assert_eq!(
        status(cf, &through_plugin),
        Status::Installed { plugin: false }
    );
    let other = script(
        cf,
        cf.methods[0],
        Action::Remove,
        &[(&main, &through_plugin)],
        PackageManager::Npm,
        &env,
    )
    .unwrap_err();
    assert!(
        other.contains("another pack's Claude Code plugin"),
        "{other}"
    );
}

#[test]
fn a_sample_shows_each_way_with_the_package_manager() {
    let cf = get("cf-cli").unwrap();
    assert_eq!(
        sample(cf, cf.methods[0], PackageManager::Bun),
        "bunx skills add cloudflare/skills --skill cf-cli -g -a <agent> -y"
    );
    let animate = get("animate").unwrap();
    assert_eq!(
        sample(animate, animate.methods[0], PackageManager::Npm),
        "claude plugin marketplace add cth9191/animate && claude plugin install animate@animate"
    );
    assert_eq!(
        sample(animate, animate.methods[1], PackageManager::Npm),
        "git clone --depth 1 https://github.com/cth9191/animate && cp -R animate/plugins/animate/skills/<skill> <agent's skills folder>/"
    );
}

#[test]
fn paths_outside_home_are_quoted() {
    let e = env();
    assert_eq!(
        e.path(Path::new("/Users/me/.claude/skills")),
        "\"$HOME/.claude/skills\""
    );
    assert_eq!(e.path(Path::new("/opt/my skills")), "'/opt/my skills'");
    assert_eq!(e.path(Path::new("/Users/me/we$ird")), "'/Users/me/we$ird'");
}

/// Every script the catalog builds, each way, with each package manager, into every kind of
/// agent (another profile, a folder outside the home folder with a space in it), is one the
/// shell reads: checked with `sh -n`, which runs nothing.
#[test]
fn every_script_is_valid_shell() {
    let has_all = Installed {
        folder: PACKS
            .iter()
            .flat_map(|p| p.skills.iter().map(|s| s.to_string()))
            .collect(),
        plugins: PACKS
            .iter()
            .filter_map(|p| p.plugin()?.plugin_key())
            .collect(),
        ..Default::default()
    };
    let none = Installed::default();
    let (main, work) = (claude(None), claude(Some("/Users/me/.claude-work")));
    let odd = Target {
        dir: PathBuf::from("/opt/my skills"),
        ..folder(".x", Some("cursor"))
    };
    let set = Env {
        claude_config_dir_set: true,
        ..env()
    };
    let mut checked = 0;
    for pack in PACKS {
        for method in pack.methods {
            let places: Vec<Place> = [&main, &work, &odd]
                .into_iter()
                .filter(|t| method.reaches(t))
                .map(|t| (t, &none))
                .collect();
            for pm in PackageManager::ALL {
                for action in [Action::Install, Action::Update] {
                    let lines = script(pack, *method, action, &places, pm, &set).unwrap();
                    let ok = std::process::Command::new("/bin/sh")
                        .args(["-n", "-c", &lines.join("\n")])
                        .status()
                        .unwrap()
                        .success();
                    assert!(ok, "{} {}: {lines:?}", pack.id, method.id());
                    checked += 1;
                }
            }
        }
        let places: Vec<Place> = [&main, &work, &odd]
            .into_iter()
            .map(|t| (t, &has_all))
            .collect();
        let lines = script(
            pack,
            pack.methods[0],
            Action::Remove,
            &places,
            PackageManager::Npm,
            &set,
        )
        .unwrap();
        let ok = std::process::Command::new("/bin/sh")
            .args(["-n", "-c", &lines.join("\n")])
            .status()
            .unwrap()
            .success();
        assert!(ok, "remove {}: {lines:?}", pack.id);
    }
    assert!(checked > 50, "{checked}");
}
