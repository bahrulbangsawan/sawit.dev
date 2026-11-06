//! The added packs installed and removed for real, from their sources over the network, in a
//! throwaway home: the exact lines the Skills page and `install_skill` run, for Claude Code and
//! the other agents. Ignored by default (they download); run them with
//! `cargo test -p sawit-core live_ -- --ignored`.

use std::path::Path;

use super::*;
use crate::package_manager::PackageManager;

/// Runs `lines` in `sh` with `home` as `$HOME`, and without `CLAUDE_CONFIG_DIR`: never the real
/// home's agents. Claude Code is found where its installer puts it in the real home.
fn run_in(home: &Path, lines: &[String]) {
    let real = std::env::var("HOME").unwrap_or_default();
    let path = format!(
        "{real}/.local/bin:{}",
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(format!("set -e\n{}", lines.join("\n")))
        .env("HOME", home)
        .env("PATH", path)
        .env("npm_config_cache", home.join(".npm"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The agents of a throwaway home: Claude Code, Codex's shared folder and Factory Droid's.
fn agents(home: &Path) -> (Target, Target, Target) {
    let claude_dir = home.join(".claude");
    let main = Target {
        dir: claude_dir.join("skills"),
        claude: Some(ClaudeHome {
            dir: claude_dir,
            profile_dir: None,
        }),
        ..claude(None)
    };
    let shared = Target {
        dir: home.join(".agents/skills"),
        ..folder(".agents/skills", Some("codex"))
    };
    let droid = Target {
        dir: home.join(".factory/skills"),
        ..folder(".factory/skills", Some("droid"))
    };
    for t in [&main, &shared, &droid] {
        std::fs::create_dir_all(&t.dir).unwrap();
    }
    (main, shared, droid)
}

fn env_in(home: &Path) -> Env {
    Env {
        home: home.to_path_buf(),
        claude_config_dir_set: false,
        bundled: home.join("bundled"),
    }
}

/// Installs `pack` with `method` into `targets`, checks each has it, removes it and checks
/// each is clean again.
fn round_trip(pack: &Pack, method: Method, home: &Path, targets: &[&Target]) {
    let env = env_in(home);
    let none = Installed::default();
    let places: Vec<Place> = targets.iter().map(|t| (*t, &none)).collect();
    let install = script(
        pack,
        method,
        Action::Install,
        &places,
        PackageManager::Npm,
        &env,
    )
    .unwrap();
    run_in(home, &install);
    let read: Vec<Installed> = targets.iter().map(|t| read_installed(t)).collect();
    for (t, installed) in targets.iter().zip(&read) {
        assert!(
            matches!(status(pack, installed), Status::Installed { .. }),
            "{} in {}: {:?}",
            pack.id,
            t.dir.display(),
            status(pack, installed)
        );
    }
    let places: Vec<Place> = targets.iter().copied().zip(&read).collect();
    let remove = script(
        pack,
        method,
        Action::Remove,
        &places,
        PackageManager::Npm,
        &env,
    )
    .unwrap();
    run_in(home, &remove);
    for t in targets {
        assert_eq!(
            status(pack, &read_installed(t)),
            Status::Missing,
            "{}",
            t.dir.display()
        );
    }
}

/// shadcn/ui's skills with the skills CLI, into Claude Code and Codex.
#[test]
#[ignore = "downloads from GitHub and npm"]
fn live_shadcn_ui_installs_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let (main, shared, _) = agents(dir.path());
    let pack = get("shadcn-ui").unwrap();
    round_trip(pack, pack.methods[0], dir.path(), &[&main, &shared]);
}

/// HyperFrames: its Core Skills with the skills CLI into Codex, and its plugin into a Claude
/// Code profile of the throwaway home.
#[test]
#[ignore = "downloads from GitHub and npm, and needs Claude Code"]
fn live_hyperframes_installs_and_removes() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let (_, shared, _) = agents(home);
    let pack = get("hyperframes").unwrap();
    round_trip(pack, pack.methods[1], home, &[&shared]);
    // The plugin: a Claude Code profile in the throwaway home (its CLAUDE_CONFIG_DIR).
    let profile = home.join(".claude-test");
    std::fs::create_dir_all(profile.join("skills")).unwrap();
    let target = Target {
        dir: profile.join("skills"),
        claude: Some(ClaudeHome {
            dir: profile.clone(),
            profile_dir: Some(profile.clone()),
        }),
        ..claude(None)
    };
    round_trip(pack, pack.methods[0], home, &[&target]);
}
