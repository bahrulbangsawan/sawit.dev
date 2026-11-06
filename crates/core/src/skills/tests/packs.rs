//! The packs added from their sources' docs: HyperFrames' Core Skills (its plugin in Claude
//! Code).

use super::*;
use crate::package_manager::PackageManager;

fn lines(pack: &Pack, method: Method, action: Action, places: &[Place]) -> Vec<String> {
    script(pack, method, action, places, PackageManager::Npm, &env()).unwrap()
}

/// HyperFrames: its plugin in Claude Code (`claude plugin marketplace add
/// heygen-com/hyperframes`, `hyperframes@hyperframes`), and its Core Skills by name with the
/// skills CLI into the other agents (Codex here), as its guides say.
#[test]
fn hyperframes_installs_its_plugin_or_its_core_skills() {
    let pack = get("hyperframes").unwrap();
    assert_eq!(
        (pack.name, pack.by, pack.category),
        ("HyperFrames", "HeyGen", Category::Media)
    );
    assert_eq!(pack.home, "https://hyperframes.heygen.com");
    assert_eq!(pack.repo, "https://github.com/heygen-com/hyperframes");
    assert_eq!(
        pack.skills,
        [
            "hyperframes",
            "hyperframes-cli",
            "hyperframes-registry",
            "media-use"
        ]
    );
    assert_eq!(
        pack.plugin().unwrap().plugin_key().as_deref(),
        Some("hyperframes@hyperframes")
    );
    let none = Installed::default();
    let main = claude(None);
    assert_eq!(
        lines(pack, pack.methods[0], Action::Install, &[(&main, &none)]),
        [
            "claude plugin marketplace add heygen-com/hyperframes",
            "claude plugin install hyperframes@hyperframes"
        ]
    );
    let codex = folder(".agents/skills", Some("codex"));
    assert_eq!(
        lines(pack, pack.methods[1], Action::Install, &[(&codex, &none)]),
        [
            "npx -y skills add heygen-com/hyperframes --skill hyperframes --skill hyperframes-cli --skill hyperframes-registry --skill media-use -g -a codex -y"
        ]
    );
    // Removing the plugin from Claude Code, and the folders from Codex's.
    let plugin = Installed {
        plugins: ["hyperframes@hyperframes".to_string()].into(),
        ..Installed::default()
    };
    assert_eq!(
        lines(pack, pack.methods[0], Action::Remove, &[(&main, &plugin)]),
        ["claude plugin uninstall hyperframes@hyperframes"]
    );
    let folders = Installed {
        folder: pack.skills.iter().map(|s| s.to_string()).collect(),
        ..Installed::default()
    };
    assert_eq!(
        lines(pack, pack.methods[1], Action::Remove, &[(&codex, &folders)]).len(),
        4
    );
    assert!(pack.note.unwrap().contains("/hyperframes:hyperframes"));
}
