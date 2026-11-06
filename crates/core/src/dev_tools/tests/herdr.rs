//! Herdr: the app (Apple silicon only, found by its bundle, versioned from its Info.plist) and
//! the TUI (a curl install Sawit removes; Homebrew's, mise's or Nix's left alone). Neither
//! uninstall touches the user's settings.

use std::path::Path;

use crate::dev_tools::detect::{app_version, at_known_place};
use crate::dev_tools::removal::{installed_outside_at, uninstall_command_in};

use super::*;

/// The app: an agent tool with no command, found at ~/Applications/Herdr.app where its
/// installer puts it, its latest from Herdr's release manifest, and Apple silicon only.
#[test]
fn the_herdr_app_is_an_apple_silicon_app() {
    let app = find("herdr-app").unwrap();
    assert_eq!((app.name, app.category), ("Herdr", Category::Agent));
    assert_eq!(app.bin, "");
    assert_eq!(app.also[0], "~/Applications/Herdr.app");
    assert_eq!(
        app.install,
        "curl -fsSL https://releases.herdr.dev/app/install.sh | sh"
    );
    assert_eq!(
        app.latest,
        Latest::Manifest("https://releases.herdr.dev/app/latest.json")
    );
    assert!(app.apple_silicon && app.version);
    // This Mac decides what the row says.
    assert_eq!(
        app.unsupported(),
        (!apple_silicon()).then_some("Needs an Apple Silicon Mac")
    );
    assert_eq!(find("herdr").unwrap().unsupported(), None);
    // No `command -v` line: the bundle is what's looked for.
    let script = crate::dev_tools::detect::detect_script();
    assert!(!script.contains(&format!("{}herdr-app", crate::dev_tools::detect::MARK)));
    assert!(script.contains(&format!("{}herdr\\t", crate::dev_tools::detect::MARK)));
}

/// The app's copy is its installer's while ~/.local/bin/herdr-app leads into it: Sawit's
/// uninstall runs on it. One dragged into /Applications from Herdr.dmg is left alone.
#[tokio::test]
async fn the_herdr_apps_copy_is_its_installers() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let app = find("herdr-app").unwrap();
    let bundle = home.join("Applications/Herdr.app");
    let contents = bundle.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).unwrap();
    std::fs::write(contents.join("MacOS/Herdr"), "").unwrap();
    // As Herdr 0.1.13's release has it.
    std::fs::write(
        contents.join("Info.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.herdr.app</string>
<key>CFBundleName</key><string>Herdr</string>
<key>CFBundleShortVersionString</key><string>0.1.13</string>
</dict></plist>"#,
    )
    .unwrap();
    assert_eq!(app_version(&bundle).await.as_deref(), Some("0.1.13"));
    if !Path::new("/Applications/Herdr.app").exists() {
        assert_eq!(at_known_place(app, home), Some(bundle.clone()));
    }
    let real = std::fs::canonicalize(&bundle).unwrap();
    // Not linked yet: a copy Sawit doesn't remove.
    assert_eq!(uninstall_command_in(app, &bundle, &real, home), None);
    assert_eq!(
        installed_outside_at(app, &bundle, &real, home, &[]),
        Some(Outside::InstallScript)
    );
    std::fs::create_dir_all(home.join(".local/bin")).unwrap();
    std::os::unix::fs::symlink(
        contents.join("MacOS/Herdr"),
        home.join(".local/bin/herdr-app"),
    )
    .unwrap();
    assert_eq!(
        uninstall_command_in(app, &bundle, &real, home).as_deref(),
        app.uninstall
    );
    assert_eq!(installed_outside_at(app, &bundle, &real, home, &[]), None);
    // Its upgrade is the installer again, which replaces the bundle in place.
    assert_eq!(upgrade_for(app, Some(&bundle)), app.install);
}

/// The app's uninstaller closes the window, stops herdr-core (the agents in it stop, as the
/// page and the tools say first) and removes what the installers put in place, never the
/// settings folders, which are kept for a choice of their own.
#[test]
fn the_herdr_apps_uninstall_keeps_the_settings() {
    let app = find("herdr-app").unwrap();
    let script = app.uninstall.unwrap();
    let stop = script.find("server stop").unwrap();
    let remove = script.find("rm ").unwrap();
    assert!(stop < remove, "herdr-core stops before its files go");
    assert!(script.contains(r#"is running then tell application id "dev.herdr.app" to quit"#));
    assert!(script.contains(r#"rm -rf "$HOME/Applications/Herdr.app""#));
    assert!(script.contains(r#"rm -rf "$HOME/.local/share/herdr-core""#));
    let removals: Vec<&str> = script.lines().filter(|l| l.contains("rm ")).collect();
    for kept in [".config", ".local/state", "\"/Applications/Herdr.app\""] {
        assert!(
            !removals.iter().any(|l| l.contains(kept)),
            "{kept} in {script}"
        );
    }
    assert_eq!(
        app.keeps,
        [
            "~/.config/herdr-app",
            "~/.config/herdr-core",
            "~/.local/state/herdr-core"
        ]
    );
    assert!(
        app.warning
            .unwrap()
            .contains("Agents running in herdr-core stop")
    );
}

/// The TUI: its curl installer's copy in ~/.local/bin is updated with `herdr update` and
/// removed; Homebrew's (`brew install herdr`, from core) is upgraded and removed with brew;
/// mise's or Nix's is found and left alone.
#[test]
fn the_herdr_tui_removes_only_its_curl_install() {
    let tui = find("herdr").unwrap();
    assert_eq!(
        (tui.name, tui.bin, tui.category),
        ("Herdr TUI", "herdr", Category::Agent)
    );
    assert_eq!(tui.install, "curl -fsSL https://herdr.dev/install.sh | sh");
    assert_eq!(tui.upgrade, Some("herdr update"));
    assert_eq!(tui.latest, Latest::GitHub("herdrdev/herdr"));
    assert_eq!(tui.owns, ["~/.local/bin/herdr"]);
    assert_eq!(tui.keeps, ["~/.config/herdr", "~/.local/state/herdr"]);
    assert!(!tui.apple_silicon);
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let at = |p: &Path| uninstall_command_in(tui, p, p, home);
    let curl = home.join(".local/bin/herdr");
    assert_eq!(at(&curl).as_deref(), tui.uninstall);
    assert_eq!(upgrade_for(tui, Some(&curl)), "herdr update");
    let mise = home.join(".local/share/mise/installs/herdr/0.9.5/herdr");
    assert_eq!(at(&mise), None);
    let nix = Path::new("/nix/store/abc-herdr-0.9.5/bin/herdr");
    assert_eq!(at(nix), None);
    let brew = Path::new("/opt/homebrew/Cellar/herdr/0.9.5/bin/herdr");
    assert_eq!(at(brew).as_deref(), Some("brew uninstall herdr"));
    let script = tui.uninstall.unwrap();
    assert!(script.starts_with(r#""$HOME/.local/bin/herdr" server stop"#));
    assert!(!script.contains(".config"));
}
