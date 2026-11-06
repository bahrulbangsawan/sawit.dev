//! HyperFrames: the CLI from npm, Studio from HeyGen's disk image.

use crate::dev_tools::removal::{installed_outside_at, uninstall_command_in};
use crate::package_manager::PackageManager;

use super::*;

/// The CLI installs from npm with the package manager picked, needs Node.js and FFmpeg, and
/// updates itself with its own `hyperframes update --yes`, which doesn't ask, never installing
/// skills into the agents (the Skills page does that).
#[test]
fn the_hyperframes_cli_installs_from_npm() {
    let hf = find("hyperframes").unwrap();
    assert_eq!(
        (hf.name, hf.bin, hf.category),
        ("HyperFrames CLI", "hyperframes", Category::Other)
    );
    assert_eq!(hf.needs, ["node", "ffmpeg"]);
    assert_eq!(hf.latest, Latest::Npm("hyperframes"));
    assert_eq!(npm_package(hf), Some("hyperframes"));
    assert_eq!(install_command(hf, None), "npm install -g hyperframes");
    assert_eq!(
        install_command(hf, Some(PackageManager::Bun)),
        "bun add -g hyperframes"
    );
    for pm in [None, Some(PackageManager::Pnpm)] {
        assert_eq!(
            upgrade_command(hf, pm),
            "HYPERFRAMES_SKIP_SKILLS=1 hyperframes update --yes"
        );
    }
    let ids = |tools: Vec<&Tool>| tools.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(
        ids(plan(&["hyperframes"], &|id| id == "homebrew")),
        ["nvm", "node", "ffmpeg", "hyperframes"]
    );
    // npm's copy is removed with the manager that put it there; another is left alone.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let npm =
        home.join(".nvm/versions/node/v26.13.0/lib/node_modules/hyperframes/bin/hyperframes.mjs");
    let link = home.join(".nvm/versions/node/v26.13.0/bin/hyperframes");
    assert_eq!(
        uninstall_command_in(hf, &link, &npm, home).as_deref(),
        Some("npm uninstall -g hyperframes")
    );
    let bun = home.join(".bun/install/global/node_modules/hyperframes/bin/hyperframes.mjs");
    assert_eq!(
        uninstall_command_in(hf, &bun, &bun, home).as_deref(),
        Some("bun remove -g hyperframes")
    );
    let project = home.join("video/node_modules/.bin/hyperframes");
    assert_eq!(uninstall_command_in(hf, &project, &project, home), None);
}

/// Studio is found at /Applications or ~/Applications, where HeyGen's installer copies it;
/// installing downloads that installer's disk image and opens it, and uninstalling quits the
/// app and removes it, nothing else.
#[test]
fn hyperframes_studio_installs_from_heygens_disk_image() {
    let studio = find("hyperframes-studio").unwrap();
    assert_eq!(studio.bin, "");
    assert!(studio.version);
    assert_eq!(
        studio.also,
        [
            "/Applications/HyperFrames.app",
            "~/Applications/HyperFrames.app"
        ]
    );
    assert!(studio.install.contains(
        r#"curl -fL --progress-bar -o "$tmp/Install-HyperFrames.dmg" "https://static.heygen.ai/hyperframes-oss/desktop/Install-HyperFrames.dmg""#
    ));
    assert!(
        studio
            .install
            .ends_with(r#"open "$tmp/Install-HyperFrames.dmg""#)
    );
    assert!(
        !studio.apple_silicon,
        "one image for Apple silicon and Intel"
    );
    let uninstall = studio.uninstall.unwrap();
    assert!(uninstall.contains(r#"application id "dev.hyperframes.desktop" is running"#));
    assert!(uninstall.contains(
        r#"rm -rf "/Applications/HyperFrames.app" "$HOME/Applications/HyperFrames.app""#
    ));
    assert_eq!(uninstall.matches("rm ").count(), 1);
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let app = home.join("Applications/HyperFrames.app");
    std::fs::create_dir_all(&app).unwrap();
    let real = std::fs::canonicalize(&app).unwrap();
    assert_eq!(
        uninstall_command_in(studio, &app, &real, home).as_deref(),
        Some(uninstall)
    );
    assert_eq!(installed_outside_at(studio, &app, &real, home, &[]), None);
    // A copy elsewhere (a download left in Downloads) is not Sawit's to remove.
    let other = home.join("Downloads/HyperFrames.app");
    std::fs::create_dir_all(&other).unwrap();
    assert_eq!(uninstall_command_in(studio, &other, &other, home), None);
}
