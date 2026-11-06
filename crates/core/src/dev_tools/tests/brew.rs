//! The Homebrew formulas: the GitHub CLI. Homebrew's copy is upgraded and removed by the
//! formula it came from; another copy is found and left alone.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dev_tools::detect::short_version;
use crate::dev_tools::removal::{brew_formula, installed_outside_at, uninstall_command_in};
use crate::package_manager::PackageManager;

use super::*;

/// A keg in a fake Cellar under `root`: `<root>/Cellar/<keg>/<version>/bin/<bin>`, with an
/// install receipt naming `tap` (none for an old receipt), linked from `<root>/bin/<bin>` as
/// Homebrew links it. Returns the link.
fn keg(root: &Path, keg: &str, version: &str, bin: &str, tap: Option<&str>) -> PathBuf {
    let dir = root.join("Cellar").join(keg).join(version);
    std::fs::create_dir_all(dir.join("bin")).unwrap();
    std::fs::write(dir.join("bin").join(bin), "#!/bin/sh\n").unwrap();
    let source = match tap {
        Some(tap) => format!(r#"{{"source":{{"tap":"{tap}","spec":"stable"}}}}"#),
        None => "{}".to_string(),
    };
    std::fs::write(dir.join("INSTALL_RECEIPT.json"), source).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let link = root.join("bin").join(bin);
    std::os::unix::fs::symlink(dir.join("bin").join(bin), &link).unwrap();
    link
}

/// What the page and the agents' tools would do with the copy at `found`: remove it with,
/// upgrade it with, and whether it's outside Homebrew.
fn plan_for(id: &str, found: &Path, home: &Path) -> (Option<String>, String, Option<Outside>) {
    let tool = find(id).unwrap();
    let real = std::fs::canonicalize(found).unwrap();
    (
        uninstall_command_in(tool, found, &real, home),
        upgrade_for(tool, Some(found)),
        installed_outside_at(tool, found, &real, home, &[]),
    )
}

/// The GitHub CLI installs with Homebrew (Homebrew first), whatever package manager is
/// picked, and its latest is GitHub's cli/cli release.
#[test]
fn the_github_cli_installs_with_homebrew() {
    let gh = find("gh").unwrap();
    assert_eq!(
        (gh.name, gh.bin, gh.category),
        ("GitHub CLI", "gh", Category::Other)
    );
    assert_eq!(gh.formula, Some("gh"));
    assert_eq!(gh.latest, Latest::GitHub("cli/cli"));
    assert_eq!(gh.needs, ["homebrew"]);
    assert!(!gh.interactive && !gh.long && gh.unsupported().is_none());
    for pm in [None, Some(PackageManager::Bun)] {
        assert_eq!(install_command(gh, pm), "brew install gh");
        assert_eq!(upgrade_command(gh, pm), "brew upgrade gh");
    }
    assert_eq!(gh.uninstall, Some("brew uninstall gh"));
    let ids = |tools: Vec<&Tool>| tools.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(plan(&["gh"], &|_| false)), ["homebrew", "gh"]);
    assert_eq!(
        short_version(
            "gh version 2.103.3 (2026-10-26)\nhttps://github.com/cli/cli/releases/tag/v2.103.3"
        )
        .as_deref(),
        Some("2.103.3")
    );
}

/// Homebrew's `gh` is upgraded and removed with brew; the `.pkg`'s copy in /usr/local/bin (or
/// MacPorts', conda's) is found, but installed outside Homebrew: never upgraded or removed.
#[test]
fn only_homebrews_github_cli_is_upgraded_or_removed() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    let brew = keg(
        &dir.path().join("homebrew"),
        "gh",
        "2.103.3",
        "gh",
        Some("homebrew/core"),
    );
    assert_eq!(brew_formula(&brew).as_deref(), Some("gh"));
    assert_eq!(
        plan_for("gh", &brew, &home),
        (
            Some("brew uninstall gh".into()),
            "brew upgrade gh".into(),
            None
        )
    );
    let pkg = dir.path().join("usr/local/bin/gh");
    std::fs::create_dir_all(pkg.parent().unwrap()).unwrap();
    std::fs::write(&pkg, "").unwrap();
    let (uninstall, _, outside) = plan_for("gh", &pkg, &home);
    assert_eq!((uninstall, outside), (None, Some(Outside::Homebrew)));
    assert_eq!(brew_formula(&pkg), None);
    // Neither the page, upgrade_dev_tools nor `sawit pupuk` upgrades that copy.
    let found = HashMap::from([(
        "gh",
        Found {
            path: pkg.clone(),
            version: Some("2.0.0".into()),
            outside: Some(Outside::Homebrew),
        },
    )]);
    let latest = HashMap::from([("gh", "2.103.3".to_string())]);
    assert!(newer("gh", &found, &latest).is_none());
    assert!(upgrade_plan(&["gh"], &found, &latest).is_empty());
}
