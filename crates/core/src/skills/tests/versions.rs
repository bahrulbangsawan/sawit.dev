//! An agent's copy of a skill Sawit ships, beside this Sawit's version: the same, differing
//! (edited, or another Sawit's), or not Sawit's at all; what an install keeps of a copy that
//! differs, and what Sawit never touches.

use std::path::Path;

use super::built_in::{NAME, agents_in, canvas_design, guard, run_in};
use super::*;
use crate::package_manager::PackageManager;
use bundled::Held;

/// A SKILL.md's frontmatter name, as agents read it: plain or quoted, with either line ending,
/// and only in the frontmatter.
#[test]
fn the_skill_name_is_the_frontmatters() {
    for (text, name) in [
        (
            "---\nname: sawit-canvas-design\ndescription: x\n---\n# Hi\n",
            Some(NAME),
        ),
        (
            "---\r\ndescription: x\r\nname: 'sawit-canvas-design'\r\n---\r\n",
            Some(NAME),
        ),
        ("---\nname: \"canvas-design\"\n---\n", Some("canvas-design")),
        ("# No frontmatter\nname: sawit-canvas-design\n", None),
        (
            "---\ndescription: x\n---\nname: sawit-canvas-design\n",
            None,
        ),
        ("", None),
    ] {
        assert_eq!(bundled::skill_name(text), name, "{text:?}");
    }
}

/// A copy counts as this Sawit's version only file for file (Finder's .DS_Store aside): edited,
/// with a file more or less, or a link inside, it differs. A link to a copy is that copy, a link
/// to nothing is nothing, and a folder whose SKILL.md names another skill, or has none, isn't
/// Sawit's.
#[test]
fn an_agents_copy_is_compared_with_this_sawits_version() {
    let dir = tempfile::tempdir().unwrap();
    let skills = dir.path().join("skills");
    let at = skills.join(NAME);
    let held = || bundled::held(NAME, &at);
    assert_eq!(held(), None);
    bundled::write_out(NAME, &skills).unwrap();
    assert_eq!(held(), Some(Held::Same));
    std::fs::write(at.join(".DS_Store"), "x").unwrap();
    assert_eq!(held(), Some(Held::Same));

    let skill = bundled::text(NAME, "SKILL.md").unwrap();
    let outside = dir.path().join("outside.md");
    std::fs::write(&outside, "x").unwrap();
    let changes: [&dyn Fn(&Path); 5] = [
        &|at| std::fs::write(at.join("SKILL.md"), format!("{skill}\nMine.\n")).unwrap(),
        &|at| std::fs::write(at.join("notes.md"), "x").unwrap(),
        &|at| std::fs::remove_file(at.join("CREDITS.md")).unwrap(),
        &|at| std::fs::create_dir_all(at.join("extras")).unwrap(),
        &|at| std::os::unix::fs::symlink(&outside, at.join("linked.md")).unwrap(),
    ];
    for (n, change) in changes.iter().enumerate() {
        change(&at);
        assert_eq!(held(), Some(Held::Differs), "change {n}");
        bundled::write_out(NAME, &skills).unwrap();
        assert_eq!(held(), Some(Held::Same), "change {n} undone");
    }

    let linked = dir.path().join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    std::os::unix::fs::symlink(&at, linked.join(NAME)).unwrap();
    assert_eq!(bundled::held(NAME, &linked.join(NAME)), Some(Held::Same));
    let gone = dir.path().join("gone");
    std::fs::create_dir_all(&gone).unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), gone.join(NAME)).unwrap();
    assert_eq!(bundled::held(NAME, &gone.join(NAME)), None);

    for theirs in [
        Some("---\nname: canvas-design\n---\n"),
        Some("# Notes\n"),
        None,
    ] {
        let other = dir.path().join("other").join(NAME);
        let _ = std::fs::remove_dir_all(&other);
        std::fs::create_dir_all(&other).unwrap();
        if let Some(text) = theirs {
            std::fs::write(other.join("SKILL.md"), text).unwrap();
        }
        assert_eq!(bundled::held(NAME, &other), Some(Held::Other), "{theirs:?}");
    }
}

/// The pack's state follows the copy: installed when it's this Sawit's version, differing (an
/// install there is an update) when not, and missing where only another skill of its name is,
/// in a folder or from a plugin.
#[test]
fn the_state_says_when_a_copy_differs_and_ignores_other_skills() {
    let pack = canvas_design();
    let with = |held: Held| Installed {
        folder: [NAME.to_string()].into(),
        bundled: [(NAME.to_string(), held)].into(),
        ..Default::default()
    };
    assert_eq!(
        status(pack, &with(Held::Same)),
        Status::Installed { plugin: false }
    );
    let differs = status(pack, &with(Held::Differs));
    assert_eq!(differs, Status::Differs);
    assert!(differs.any() && differs.full());
    assert_eq!(status(pack, &with(Held::Other)), Status::Missing);
    let from_a_plugin = Installed {
        from_plugins: [NAME.to_string()].into(),
        ..Default::default()
    };
    assert_eq!(status(pack, &from_a_plugin), Status::Missing);
}

/// Installing over a copy that differs keeps it in Sawit's data folder and says so; a folder of the
/// skill's name that isn't Sawit's stops the install there and is never removed.
#[test]
fn an_install_keeps_a_copy_that_differs_and_leaves_other_skills_alone() {
    let pack = canvas_design();
    let (main, shared) = (claude(None), folder(".agents/skills", Some("codex")));
    let one = |held: Held| Installed {
        bundled: [(NAME.to_string(), held)].into(),
        ..Default::default()
    };
    let (edited, theirs) = (one(Held::Differs), one(Held::Other));
    let script_for = |action, places: &[Place]| {
        script(
            pack,
            pack.methods[0],
            action,
            places,
            PackageManager::Npm,
            &env(),
        )
    };
    let from = "\"$HOME/Library/Application Support/sawit/bundled-skills/sawit-canvas-design\"";
    let at = "\"$HOME/.claude/skills/sawit-canvas-design\"";
    let kept_in = "\"$HOME/Library/Application Support/sawit/skill-backups/claude-code\"";
    let bak =
        "\"$HOME/Library/Application Support/sawit/skill-backups/claude-code/sawit-canvas-design\"";
    assert_eq!(
        script_for(Action::Update, &[(&main, &edited)]).unwrap(),
        [
            guard(from),
            "mkdir -p \"$HOME/.claude/skills\"".to_string(),
            format!("rm -rf {bak}"),
            format!("mkdir -p {kept_in}"),
            format!("mv {at} {bak}"),
            format!("echo \"Kept the copy that differs from this Sawit's version as\" {bak}"),
            format!("cp -R {from} \"$HOME/.claude/skills\"/"),
        ]
    );
    assert_eq!(
        kept(pack, &[(&main, &edited), (&shared, &theirs)], &env()),
        [home().join(
            "Library/Application Support/sawit/skill-backups/claude-code/sawit-canvas-design"
        )]
    );

    let refused = script_for(Action::Install, &[(&main, &edited), (&shared, &theirs)]);
    let refused = refused.unwrap_err();
    assert!(
        refused.starts_with(
            ".agents/skills has a sawit-canvas-design folder that isn't Sawit's skill"
        ),
        "{refused}"
    );
    // Removing deletes Sawit's copy, edited or not, and never the other skill.
    assert_eq!(
        script_for(Action::Remove, &[(&main, &edited), (&shared, &theirs)]).unwrap(),
        [format!("rm -rf {at}")]
    );
    let nothing = script_for(Action::Remove, &[(&shared, &theirs)]).unwrap_err();
    assert!(
        nothing.contains("isn't Sawit's skill") && nothing.contains("nothing to remove"),
        "{nothing}"
    );
    // So a removal from both acts on Claude Code's alone.
    assert!(removes_from(pack, &(&main, &edited)));
    assert!(!removes_from(pack, &(&shared, &theirs)));
}

/// Run by the shell: an edited copy goes to its backup folder, replacing an older one, and the new copy is
/// this Sawit's version.
#[test]
fn an_older_bak_is_replaced_by_the_copy_that_differs() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let env = Env {
        home: home.clone(),
        claude_config_dir_set: false,
        bundled: home.join("sawit-data/bundled-skills"),
    };
    let (main, _) = agents_in(&home);
    let copy = bundled::write_out(NAME, &main.dir).unwrap();
    std::fs::write(copy.join("notes.md"), "mine").unwrap();
    let bak = backup(&env, &main, NAME);
    std::fs::create_dir_all(&bak).unwrap();
    std::fs::write(bak.join("older.md"), "older").unwrap();
    let pack = canvas_design();
    let installed = read_installed(&main);
    assert_eq!(status(pack, &installed), Status::Differs);
    let lines = script(
        pack,
        pack.methods[0],
        Action::Update,
        &[(&main, &installed)],
        PackageManager::Npm,
        &env,
    );
    bundled::write_out(NAME, &env.bundled).unwrap();
    let said = run_in(&home, &lines.unwrap());
    assert!(said.contains(&bak.display().to_string()), "{said}");
    assert_eq!(
        std::fs::read_to_string(bak.join("notes.md")).unwrap(),
        "mine"
    );
    assert!(!bak.join("older.md").exists());
    assert_eq!(bundled::held(NAME, &copy), Some(Held::Same));
    assert_eq!(
        status(pack, &read_installed(&main)),
        Status::Installed { plugin: false }
    );
}
