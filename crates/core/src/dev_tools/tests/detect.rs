//! Detection: apps at their places, versions from `--version` and `Info.plist`, a failing login
//! shell.

use std::path::{Path, PathBuf};

use crate::dev_tools::detect::{app_version, at_known_place, detect_as, short_version, version_of};

use super::*;

/// An app without a command is found where its cask (or a drag from the disk image) puts it:
/// /Applications first, else ~/Applications.
#[test]
fn an_app_is_found_at_its_place() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let puremac = find("puremac").unwrap();
    let user_app = home.join("Applications/PureMac.app");
    std::fs::create_dir_all(user_app.join("Contents")).unwrap();
    // /Applications/PureMac.app isn't on this Mac when the test runs, unless it is.
    let expected = if Path::new("/Applications/PureMac.app").exists() {
        PathBuf::from("/Applications/PureMac.app")
    } else {
        user_app.clone()
    };
    assert_eq!(at_known_place(puremac, home), Some(expected));
    let other = tempfile::tempdir().unwrap();
    if !Path::new("/Applications/PureMac.app").exists() {
        assert_eq!(at_known_place(puremac, other.path()), None);
    }
}

/// An app's version is its Info.plist's CFBundleShortVersionString, in an XML or a binary
/// plist; an app without one has none, and a tool that says its version is no use has none.
#[tokio::test]
async fn an_apps_version_comes_from_its_info_plist() {
    let dir = tempfile::tempdir().unwrap();
    let app = dir.path().join("PureMac.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(&contents).unwrap();
    let plist = contents.join("Info.plist");
    // As PureMac 3.2.0's Info.plist has them.
    std::fs::write(
            &plist,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>com.puremac.app</string>
	<key>CFBundleShortVersionString</key>
	<string>3.2.0</string>
	<key>CFBundleVersion</key>
	<string>31</string>
</dict>
</plist>
"#,
        )
        .unwrap();
    let puremac = find("puremac").unwrap();
    assert_eq!(app_version(&app).await.as_deref(), Some("3.2.0"));
    assert_eq!(version_of(puremac, &app).await.as_deref(), Some("3.2.0"));
    // Cap says nothing useful about its version.
    assert_eq!(version_of(find("cap").unwrap(), &app).await, None);
    let binary = std::process::Command::new("/usr/bin/plutil")
        .args(["-convert", "binary1"])
        .arg(&plist)
        .status()
        .unwrap();
    assert!(binary.success());
    assert!(!std::fs::read(&plist).unwrap().starts_with(b"<?xml"));
    assert_eq!(app_version(&app).await.as_deref(), Some("3.2.0"));
    // No version in it, and no Info.plist at all.
    std::fs::write(
        &plist,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict><key>CFBundleVersion</key><string>31</string></dict></plist>
"#,
    )
    .unwrap();
    assert_eq!(app_version(&app).await, None);
    std::fs::remove_file(&plist).unwrap();
    assert_eq!(app_version(&app).await, None);
}

/// A login shell that fails is reported, not read as "nothing installed".
#[tokio::test]
async fn a_failing_login_shell_is_reported() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    // Named `sh`, so it's taken for a POSIX shell and run.
    let sh = dir.path().join("sh");
    std::fs::write(&sh, "#!/bin/sh\necho 'broken profile' >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
    let found = detect_as(sh.to_str()).await;
    let problem = found.problem.expect("no problem reported");
    assert!(problem.contains("broken profile"), "{problem}");
    assert!(problem.contains("exit 3"), "{problem}");
}

#[test]
fn versions() {
    assert_eq!(short_version("Python 3.13.2\n").as_deref(), Some("3.13.2"));
    assert_eq!(short_version("v22.11.0").as_deref(), Some("22.11.0"));
    assert_eq!(
        short_version("2.0.14 (Claude Code)").as_deref(),
        Some("2.0.14")
    );
    assert_eq!(
        short_version("rustc 1.90.0 (1159e78c4 2025-09-14)").as_deref(),
        Some("1.90.0")
    );
    assert_eq!(
        short_version("uv 0.12.1 (Homebrew)").as_deref(),
        Some("0.12.1")
    );
    assert_eq!(short_version("nothing here"), None);
}
