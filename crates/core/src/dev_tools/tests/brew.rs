//! The Homebrew formulas: the GitHub CLI, MinIO and the MinIO Client. Homebrew's copy
//! is upgraded and removed by the formula it came from; another copy is found and left alone;
//! Midnight Commander's `mc` isn't the MinIO Client.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dev_tools::detect::{another_program, short_version, version_text};
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

/// MinIO and its client install from MinIO's tap. A copy from Homebrew's core (`minio`,
/// `minio-mc`, deprecated) is Homebrew's too: `brew upgrade` names the formula its receipt
/// gives, so neither a tap's nor core's copy is upgraded by the other's name.
#[test]
fn minio_and_its_client_upgrade_by_the_formula_they_came_from() {
    for (id, formula) in [("minio", "minio/stable/minio"), ("mc", "minio/stable/mc")] {
        let tool = find(id).unwrap();
        assert_eq!(tool.formula, Some(formula));
        assert_eq!(tool.install, format!("brew install {formula}"));
        assert_eq!(tool.needs, ["homebrew"]);
        assert_eq!(tool.signature, Some("minio"));
        // The repositories are archived: no release to compare with.
        assert_eq!(tool.latest, Latest::None);
    }
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    let tap = keg(
        &dir.path().join("a"),
        "mc",
        "RELEASE.2025-08-13T08-35-41Z",
        "mc",
        Some("minio/stable"),
    );
    assert_eq!(
        plan_for("mc", &tap, &home),
        (
            Some("brew uninstall mc".into()),
            "brew upgrade minio/stable/mc".into(),
            None
        )
    );
    let core = keg(
        &dir.path().join("b"),
        "minio-mc",
        "2025-08-13T08-35-41Z",
        "mc",
        Some("homebrew/core"),
    );
    assert_eq!(
        plan_for("mc", &core, &home),
        (
            Some("brew uninstall minio-mc".into()),
            "brew upgrade minio-mc".into(),
            None
        )
    );
    // An old receipt without a tap is core's.
    let server = keg(
        &dir.path().join("c"),
        "minio",
        "2025-10-15T17-29-55Z",
        "minio",
        None,
    );
    assert_eq!(plan_for("minio", &server, &home).1, "brew upgrade minio");
    // Their keeping: the client's aliases and keys stay.
    assert_eq!(find("mc").unwrap().keeps, ["~/.mc"]);
    assert!(kept(find("mc").unwrap(), &home).is_empty());
    std::fs::create_dir_all(home.join(".mc")).unwrap();
    assert_eq!(kept(find("mc").unwrap(), &home), [home.join(".mc")]);
}

/// MinIO names its releases by date: their versions are read, and compared by date and time.
#[test]
fn minio_versions_are_dates() {
    assert_eq!(
        short_version("mc version RELEASE.2025-08-13T08-35-41Z (commit-id=7394ce0)\nRuntime: go1.27.1 darwin/arm64").as_deref(),
        Some("2025-08-13T08-35-41Z")
    );
    assert_eq!(
        short_version("minio version RELEASE.2025-10-15T17-29-55Z (commit-id=9e49d5e)").as_deref(),
        Some("2025-10-15T17-29-55Z")
    );
    assert!(is_newer("2025-10-15T17-29-55Z", "2025-09-06T17-38-46Z"));
    assert!(is_newer(
        "RELEASE.2025-08-13T08-35-42Z",
        "2025-08-13T08-35-41Z"
    ));
    assert!(!is_newer("2025-08-13T08-35-41Z", "2025-08-13T08-35-41Z"));
    assert!(!is_newer("2024-12-31T23-59-59Z", "2025-01-01T00-00-00Z"));
    // Others compare as before.
    assert!(is_newer("2.10.0", "2.9.9"));
}

/// An `mc` is the MinIO Client only when its version says MinIO: Midnight Commander's isn't,
/// nor one that can't say. A tool without a signature is whatever its command is.
#[tokio::test]
async fn midnight_commander_is_not_the_minio_client() {
    let mc = find("mc").unwrap();
    assert!(another_program(
        mc,
        Some("GNU Midnight Commander 4.8.33\nBuilt with GLib 2.84")
    ));
    assert!(another_program(mc, None));
    assert!(!another_program(
        mc,
        Some("mc version RELEASE.2025-08-13T08-35-41Z\nCopyright (c) 2015-2025 MinIO, Inc.")
    ));
    assert!(!another_program(find("gh").unwrap(), Some("anything")));
    // Read from the command itself.
    let dir = tempfile::tempdir().unwrap();
    let script = |name: &str, text: &str| {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.path().join(name);
        std::fs::write(&path, format!("#!/bin/sh\nprintf '{text}\\n'\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    };
    let gnu = script("gnu-mc", "GNU Midnight Commander 4.8.33");
    let minio = script(
        "minio-mc",
        "mc version RELEASE.2025-08-13T08-35-41Z (commit-id=1)\\nCopyright (c) 2015-2025 MinIO, Inc.",
    );
    let text = version_text(mc, &gnu).await;
    assert!(another_program(mc, text.as_deref()));
    let text = version_text(mc, &minio).await;
    assert!(!another_program(mc, text.as_deref()));
    assert_eq!(
        crate::dev_tools::detect::version_of(mc, &minio)
            .await
            .as_deref(),
        Some("2025-08-13T08-35-41Z")
    );
}
