//! The packs added from their sources' docs: shadcn/ui's official skills, HyperFrames' Core
//! Skills (its plugin in Claude Code), and Design to Code, whose repository's example sites
//! never land on disk.

use super::*;
use crate::package_manager::PackageManager;

fn lines(pack: &Pack, method: Method, action: Action, places: &[Place]) -> Vec<String> {
    script(pack, method, action, places, PackageManager::Npm, &env()).unwrap()
}

/// shadcn/ui's skills install the way ui.shadcn.com/docs/skills says (`npx skills add
/// shadcn/ui`), both of them, into Claude Code and the other agents picked; removing deletes
/// their two folders.
#[test]
fn shadcns_official_skills_install_with_the_skills_cli() {
    let pack = get("shadcn-ui").unwrap();
    assert_eq!((pack.name, pack.by), ("shadcn/ui", "shadcn"));
    assert_eq!(pack.category, Category::Design);
    assert_eq!(pack.home, "https://ui.shadcn.com/docs/skills");
    assert_eq!(pack.repo, "https://github.com/shadcn-ui/ui");
    assert_eq!(pack.skills, ["shadcn", "migrate-radix-to-base"]);
    // Not the other shadcn pack.
    assert_eq!(get("improve").unwrap().skills, ["improve"]);
    let none = Installed::default();
    let (main, codex) = (claude(None), folder(".agents/skills", Some("codex")));
    assert_eq!(
        lines(
            pack,
            pack.methods[0],
            Action::Install,
            &[(&main, &none), (&codex, &none)]
        ),
        ["npx -y skills add shadcn/ui --skill '*' -g -a claude-code -a codex -y"]
    );
    let there = Installed {
        folder: ["shadcn".to_string(), "migrate-radix-to-base".to_string()].into(),
        ..Installed::default()
    };
    assert_eq!(
        lines(pack, pack.methods[0], Action::Remove, &[(&main, &there)]),
        [
            "rm -rf \"$HOME/.claude/skills/shadcn\"",
            "rm -rf \"$HOME/.claude/skills/migrate-radix-to-base\""
        ]
    );
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

/// Design to Code installs its one skill, and only that folder: the skills CLI by name, or a
/// clone whose `design-to-code` folder (at the repository's top, beside `websites/`) is copied
/// into Claude Code's, Codex's and Factory Droid's skills folders. Its description says it
/// overlaps with build-design.
#[test]
fn design_to_code_lands_only_its_skill_folder() {
    let pack = get("design-to-code").unwrap();
    assert_eq!(
        (pack.name, pack.by, pack.category),
        ("Design to Code", "Atharv Jadav", Category::Design)
    );
    assert_eq!(pack.home, "https://github.com/Atharvsinh-codez/skills");
    assert_eq!(pack.repo, "https://github.com/Atharvsinh-codez/skills");
    assert_eq!(pack.skills, ["design-to-code"]);
    assert!(pack.about.contains("build-design"), "{}", pack.about);
    assert!(pack.note.unwrap().contains("Restart"));
    let none = Installed::default();
    let main = claude(None);
    let codex = folder(".agents/skills", Some("codex"));
    // Droid's own folder: the skills CLI installs Droid into the shared one.
    let droid = folder(".factory/skills", None);
    let places = [(&main, &none), (&codex, &none), (&droid, &none)];
    assert_eq!(
        lines(pack, pack.methods[0], Action::Install, &places[..2]),
        [
            "npx -y skills add Atharvsinh-codez/skills --skill design-to-code -g -a claude-code -a codex -y"
        ]
    );
    let copy = lines(pack, pack.methods[1], Action::Install, &places);
    assert_eq!(
        copy[2],
        "git clone --depth 1 https://github.com/Atharvsinh-codez/skills \"$tmp/skills\""
    );
    let copies: Vec<&String> = copy.iter().filter(|l| l.starts_with("cp ")).collect();
    assert_eq!(
        copies,
        [
            "cp -R \"$tmp/skills/design-to-code\" \"$HOME/.claude/skills\"/",
            "cp -R \"$tmp/skills/design-to-code\" \"$HOME/.agents/skills\"/",
            "cp -R \"$tmp/skills/design-to-code\" \"$HOME/.factory/skills\"/",
        ]
    );
    assert!(!copy.iter().any(|l| l.contains("websites")));
    assert_eq!(
        sample(pack, pack.methods[1], PackageManager::Npm, &env()),
        "git clone --depth 1 https://github.com/Atharvsinh-codez/skills && cp -R skills/<skill> <agent's skills folder>/"
    );
    let there = Installed {
        folder: ["design-to-code".to_string()].into(),
        ..Installed::default()
    };
    assert_eq!(
        lines(pack, pack.methods[0], Action::Remove, &[(&droid, &there)]),
        ["rm -rf \"$HOME/.factory/skills/design-to-code\""]
    );
}
