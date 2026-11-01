//! How `sawit pupuk` and `sawit cabut` word an app installed outside Homebrew.

use std::path::PathBuf;

use crate::kebun::tools::{cabut_warnings, pupuk_skip};

/// `sawit pupuk puremac` on PureMac from its disk image says it's outside Homebrew and leaves
/// it, as the page and upgrade_dev_tools do; `sawit cabut homebrew` says a cask's app only
/// stops getting updates.
#[test]
fn pupuk_and_cabut_word_an_app_outside_homebrew() {
    use crate::dev_tools::{Found, Outside, find, upgrade_plan};
    let at = |path: &str, outside| Found {
        path: PathBuf::from(path),
        version: Some("3.1.0".into()),
        outside,
    };
    let found = std::collections::HashMap::from([
        (
            "puremac",
            at("/Applications/PureMac.app", Some(Outside::Homebrew)),
        ),
        (
            "zed",
            at("/Applications/Zed.app", Some(Outside::InstallScript)),
        ),
        ("bun", at("/nonexistent-sawit-test/.bun/bin/bun", None)),
    ]);
    let latest = std::collections::HashMap::from([("puremac", "3.2.0".to_string())]);
    assert!(upgrade_plan(&["puremac", "zed"], &found, &latest).is_empty());
    assert_eq!(
        pupuk_skip("puremac", &found),
        "puremac di /Applications/PureMac.app dipasang di luar Homebrew: tidak dipupuk Sawit"
    );
    assert_eq!(
        pupuk_skip("zed", &found),
        "zed di /Applications/Zed.app dipasang di luar skrip instalnya: tidak dipupuk Sawit"
    );
    assert_eq!(pupuk_skip("bun", &found), "bun sudah versi terbaru");
    assert!(pupuk_skip("deno", &found).contains("belum terpasang"));

    let tool = |id: &str| find(id).unwrap();
    assert_eq!(
        cabut_warnings(tool("homebrew"), &[tool("puremac")]),
        ["PureMac tetap jalan, tapi tak lagi diperbarui lewat Homebrew."]
    );
    assert_eq!(
        cabut_warnings(tool("node"), &[tool("npm"), tool("gemini")]),
        ["npm (latest), Gemini CLI butuh Node.js; ikut layu kalau dicabut."]
    );
    assert!(cabut_warnings(tool("bun"), &[]).is_empty());
}
