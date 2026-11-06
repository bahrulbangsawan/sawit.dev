//! The extras some rows show: the GitHub CLI's sign-in, HyperFrames' Chrome, Herdr's release manifest.

use std::collections::HashMap;

use crate::dev_tools::extras::{GhAuth, hyperframes_browser, parse_gh_auth};
use crate::dev_tools::versions::manifest_version;

/// `gh auth status` exits 0 when signed in, with the login in its new and old wording, and 1
/// when not.
#[test]
fn the_github_clis_sign_in_is_read_from_gh_auth_status() {
    let new = "github.com\n  ✓ Logged in to github.com account octo-cat (keyring)\n  - Active account: true\n  - Token: gho_************************************\n";
    assert_eq!(
        parse_gh_auth(Some(0), new),
        GhAuth::SignedIn(Some("octo-cat".into()))
    );
    let old = "github.com\n  ✓ Logged in to github.com as hubot (oauth_token)\n";
    assert_eq!(
        parse_gh_auth(Some(0), old),
        GhAuth::SignedIn(Some("hubot".into()))
    );
    assert_eq!(
        parse_gh_auth(Some(0), "github.com\n"),
        GhAuth::SignedIn(None)
    );
    let out = "You are not logged into any GitHub hosts. To log in, run: gh auth login\n";
    assert_eq!(parse_gh_auth(Some(1), out), GhAuth::SignedOut);
    assert_eq!(parse_gh_auth(None, new), GhAuth::SignedOut);
}

/// HyperFrames renders with the Chrome `HYPERFRAMES_BROWSER_PATH` names, its own download, or
/// Google Chrome, in that order; none found is said, not fixed by installing one.
#[test]
fn hyperframes_finds_a_chrome_where_its_cli_looks() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let vars: HashMap<&str, String> = HashMap::new();
    let none = |name: &str| vars.get(name).cloned();
    // On this Mac Google Chrome may be installed; the test reads only the home given.
    let system = [
        "/Applications/Google Chrome.app",
        "/Applications/Chromium.app",
    ]
    .into_iter()
    .find(|p| std::path::Path::new(p).exists())
    .map(std::path::PathBuf::from);
    assert_eq!(hyperframes_browser(home, &none), system);
    // An empty download folder doesn't count; one with a browser does.
    let chrome = home.join(".cache/hyperframes/chrome");
    std::fs::create_dir_all(&chrome).unwrap();
    assert_eq!(hyperframes_browser(home, &none), system);
    std::fs::create_dir_all(chrome.join("chrome-headless-shell")).unwrap();
    assert_eq!(hyperframes_browser(home, &none), Some(chrome.clone()));
    // HYPERFRAMES_CACHE_DIR moves the download.
    let cache = home.join("elsewhere");
    std::fs::create_dir_all(cache.join("chrome/x")).unwrap();
    let moved = |name: &str| (name == "HYPERFRAMES_CACHE_DIR").then(|| cache.display().to_string());
    assert_eq!(
        hyperframes_browser(home, &moved),
        Some(cache.join("chrome"))
    );
    // The variable wins, when its browser is there.
    let pick = home.join("my-chrome");
    std::fs::write(&pick, "").unwrap();
    let named =
        |name: &str| (name == "HYPERFRAMES_BROWSER_PATH").then(|| pick.display().to_string());
    assert_eq!(hyperframes_browser(home, &named), Some(pick.clone()));
    let missing =
        |name: &str| (name == "HYPERFRAMES_BROWSER_PATH").then(|| "/no/such/chrome".to_string());
    assert_eq!(hyperframes_browser(home, &missing), None);
}

/// Herdr's release manifest gives the app's latest at its top level.
#[test]
fn herdr_apps_latest_comes_from_its_manifest() {
    let json = r#"{"version": "0.1.13", "core_min": "0.1.11", "targets": {"macos-aarch64": {"url": "x"}}}"#;
    assert_eq!(manifest_version(json).as_deref(), Some("0.1.13"));
    assert_eq!(manifest_version(r#"{"targets": {}}"#), None);
    assert_eq!(manifest_version("<html>"), None);
}
