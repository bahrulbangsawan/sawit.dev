//! A skill Sawit ships installs Built In: its folder, written out into Sawit's data folder, copied
//! into every agent's skills folder picked once the lines find it there, found there when its
//! SKILL.md is Sawit's, and removed as a copy is.

use std::path::Path;

use super::*;
use crate::package_manager::PackageManager;

pub(super) const NAME: &str = "sawit-canvas-design";

pub(super) fn canvas_design() -> &'static Pack {
    get(NAME).unwrap()
}

/// The first line of a Built In script copying from `from`: it stops there, before anything is
/// removed, when Sawit hasn't written the skill out.
pub(super) fn guard(from: &str) -> String {
    format!(
        "[ -d {from} ] || {{ echo {from} \"is missing: Sawit writes the skill out there as it installs it, so install it from Sawit (its Skills page, or install_skill).\" >&2; exit 1; }}"
    )
}

/// Built In reaches every kind of agent, as a copy does: a Claude Code profile, a folder the
/// skills CLI knows, and one it doesn't. Its sample names the folder in Sawit's data folder.
#[test]
fn built_in_reaches_every_agent() {
    let method = canvas_design().methods[0];
    assert_eq!(method, Method::Bundled);
    assert_eq!((method.id(), method.label()), ("built-in", "Built In"));
    assert!(!method.uses_package_manager());
    assert_eq!(method.plugin_key(), None);
    for target in [
        claude(None),
        claude(Some("/Users/me/.claude-work")),
        folder(".agents/skills", Some("codex")),
        folder(".codex/skills", None),
    ] {
        assert!(method.reaches(&target), "{}", target.label);
    }
    assert_eq!(
        sample(canvas_design(), method, PackageManager::Bun, &env()),
        "cp -R \"$HOME/Library/Application Support/sawit/bundled-skills/sawit-canvas-design\" <agent's skills folder>/"
    );
}

/// An install first makes sure the folder Sawit writes out is there, then copies it into each
/// agent's folder, replacing what was there, as a copy does; an update runs the same, and nothing
/// is cloned or downloaded. Paths in the home folder are written from "$HOME", others quoted.
#[test]
fn an_install_copies_the_written_out_folder_into_each_agent() {
    let none = Installed::default();
    let pack = canvas_design();
    let shared = folder(".agents/skills", Some("codex"));
    let places = [(&claude(None), &none), (&shared, &none)];
    let lines = |action, env: &Env| {
        let pm = PackageManager::Npm;
        script(pack, pack.methods[0], action, &places, pm, env).unwrap()
    };
    let from = "\"$HOME/Library/Application Support/sawit/bundled-skills/sawit-canvas-design\"";
    let expected = [
        guard(from),
        "mkdir -p \"$HOME/.claude/skills\"".to_string(),
        "rm -rf \"$HOME/.claude/skills/sawit-canvas-design\"".to_string(),
        format!("cp -R {from} \"$HOME/.claude/skills\"/"),
        "mkdir -p \"$HOME/.agents/skills\"".to_string(),
        "rm -rf \"$HOME/.agents/skills/sawit-canvas-design\"".to_string(),
        format!("cp -R {from} \"$HOME/.agents/skills\"/"),
    ];
    assert_eq!(lines(Action::Install, &env()), expected);
    assert_eq!(lines(Action::Update, &env()), expected);
    assert!(kept(pack, &places, &env()).is_empty());

    // A data folder outside the home folder (SAWIT_HOME), with a space and a quote in its name.
    let odd = Env {
        bundled: "/opt/it's sawit/bundled-skills".into(),
        ..env()
    };
    let installed = lines(Action::Install, &odd);
    let from = "'/opt/it'\\''s sawit/bundled-skills/sawit-canvas-design'";
    assert_eq!(installed[0], guard(from));
    assert_eq!(
        installed[3],
        format!("cp -R {from} \"$HOME/.claude/skills\"/")
    );
    let ok = std::process::Command::new("/bin/sh")
        .args(["-n", "-c", &installed.join("\n")])
        .status()
        .unwrap()
        .success();
    assert!(ok, "{installed:?}");
}

/// The whole round, run by the shell in a home folder of the test's own: Sawit writes the skill
/// out, the install copies it into two agents' folders (one where a link to an edited copy was,
/// kept in Sawit's data folder), each then has it as Sawit ships it, and removing it from one leaves the other's.
#[test]
fn it_installs_is_found_and_is_removed_in_a_home_of_its_own() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let env = Env {
        home: home.clone(),
        claude_config_dir_set: false,
        bundled: home.join("sawit-data/bundled-skills"),
    };
    let (main, shared) = agents_in(&home);
    // What was there before: a link to an older copy in the shared folder.
    let old = home.join("old/sawit-canvas-design");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(
        old.join("SKILL.md"),
        "---\nname: sawit-canvas-design\n---\nold\n",
    )
    .unwrap();
    std::fs::create_dir_all(&shared.dir).unwrap();
    std::os::unix::fs::symlink(&old, shared.dir.join(NAME)).unwrap();

    let pack = canvas_design();
    let status_in = |target: &Target| status(pack, &read_installed(target));
    assert_eq!(status_in(&main), Status::Missing);
    assert_eq!(status_in(&shared), Status::Differs);

    let (has_main, has_shared) = (read_installed(&main), read_installed(&shared));
    let places = [(&main, &has_main), (&shared, &has_shared)];
    let install = script(
        pack,
        pack.methods[0],
        Action::Install,
        &places,
        PackageManager::Npm,
        &env,
    );
    let bak = backup(&env, &shared, NAME);
    assert_eq!(kept(pack, &places, &env), std::slice::from_ref(&bak));
    assert!(
        bak.starts_with(home.join("sawit-data/skill-backups")),
        "{bak:?}"
    );
    bundled::write_out(NAME, &env.bundled).unwrap();
    let said = run_in(&home, &install.unwrap());
    assert_eq!(
        said.trim(),
        format!(
            "Kept the copy that differs from this Sawit's version as {}",
            bak.display()
        )
    );
    let skill = bundled::text(NAME, "SKILL.md").unwrap();
    for target in [&main, &shared] {
        let copied = target.dir.join(NAME);
        assert!(
            std::fs::symlink_metadata(&copied).unwrap().is_dir(),
            "{copied:?}"
        );
        assert_eq!(
            std::fs::read_to_string(copied.join("SKILL.md")).unwrap(),
            skill
        );
        assert_eq!(status_in(target), Status::Installed { plugin: false });
    }
    // The link is kept beside it, and the folder it points at stays.
    assert_eq!(std::fs::read_link(&bak).unwrap(), old);
    assert!(old.join("SKILL.md").is_file());

    let installed = read_installed(&main);
    let remove = script(
        pack,
        pack.methods[0],
        Action::Remove,
        &[(&main, &installed)],
        PackageManager::Npm,
        &env,
    )
    .unwrap();
    assert_eq!(
        remove,
        ["rm -rf \"$HOME/.claude/skills/sawit-canvas-design\""]
    );
    run_in(&home, &remove);
    assert_eq!(status_in(&main), Status::Missing);
    assert_eq!(status_in(&shared), Status::Installed { plugin: false });
    let nothing = script(
        pack,
        pack.methods[0],
        Action::Remove,
        &[(&main, &read_installed(&main))],
        PackageManager::Npm,
        &env,
    );
    assert!(nothing.unwrap_err().contains("nothing to remove"));
}

/// Copied and run without Sawit, where it never wrote the skill out, the lines stop at the first
/// one: the agent's copy (edited, here) is still there, and no backup is made.
#[test]
fn the_lines_stop_before_removing_anything_when_nothing_was_written_out() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().canonicalize().unwrap();
    let env = Env {
        home: home.clone(),
        claude_config_dir_set: false,
        bundled: home.join("sawit-data/bundled-skills"),
    };
    let (main, _) = agents_in(&home);
    let copy = bundled::write_out(NAME, &main.dir).unwrap();
    std::fs::write(copy.join("CREDITS.md"), "mine").unwrap();
    let installed = read_installed(&main);
    let pack = canvas_design();
    let lines = script(
        pack,
        pack.methods[0],
        Action::Update,
        &[(&main, &installed)],
        PackageManager::Npm,
        &env,
    )
    .unwrap();
    let ran = shell_in(&home, &lines);
    assert!(!ran.status.success());
    let said = String::from_utf8_lossy(&ran.stderr);
    assert!(
        said.contains("is missing: Sawit writes the skill out"),
        "{said}"
    );
    assert_eq!(
        std::fs::read_to_string(copy.join("CREDITS.md")).unwrap(),
        "mine"
    );
    assert!(!backup(&env, &main, NAME).exists());
}

/// Running an install or update writes the skill out first, into the folder its lines copy from;
/// a removal writes nothing. (Its one line here touches nothing.)
#[tokio::test]
async fn running_an_install_writes_the_skill_out_first() {
    let _one = PACK_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env = Env {
        bundled: dir.path().join("bundled-skills"),
        ..env()
    };
    let pack = canvas_design();
    let lines = ["true".to_string()];
    run(pack, Action::Remove, &lines, &env, |_| {})
        .await
        .unwrap();
    assert!(!env.bundled.exists());
    for action in [Action::Install, Action::Update] {
        run(pack, action, &lines, &env, |_| {}).await.unwrap();
        let written = std::fs::read_to_string(env.bundled.join(NAME).join("SKILL.md"));
        assert_eq!(written.unwrap(), bundled::text(NAME, "SKILL.md").unwrap());
    }
}

/// The pack's lock comes first: while another run holds it, a run writes nothing out and says its
/// lines never started.
#[tokio::test]
async fn a_run_takes_the_packs_lock_before_it_writes_anything_out() {
    let _one = PACK_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let env = Env {
        bundled: dir.path().join("bundled-skills"),
        ..env()
    };
    let pack = canvas_design();
    let held = crate::dev_tools::lock(&format!("skill-{}", pack.id), pack.name).unwrap();
    let failed = run(pack, Action::Install, &["true".to_string()], &env, |_| {})
        .await
        .unwrap_err();
    assert!(!failed.started);
    assert!(failed.to_string().contains("already being"), "{failed}");
    assert!(!env.bundled.exists());
    drop(held);
    // A script that fails ran.
    let fails = ["false".to_string()];
    let failed = run(pack, Action::Install, &fails, &env, |_| {}).await;
    assert!(failed.unwrap_err().started);
}

/// Claude Code's folder and the shared `.agents/skills` in `home`.
pub(super) fn agents_in(home: &Path) -> (Target, Target) {
    let claude_dir = home.join(".claude");
    let main = Target {
        dir: claude_dir.join("skills"),
        claude: Some(ClaudeHome {
            dir: claude_dir,
            profile_dir: None,
        }),
        ..claude(None)
    };
    let shared = Target {
        dir: home.join(".agents/skills"),
        ..folder(".agents/skills", Some("codex"))
    };
    (main, shared)
}

/// Runs `lines` with `/bin/sh -e`, in an environment that holds only `home` as HOME and the
/// system's PATH, so "$HOME/…" is the test's own folder.
pub(super) fn shell_in(home: &Path, lines: &[String]) -> std::process::Output {
    for line in lines {
        assert!(
            line.contains("\"$HOME/"),
            "{line} acts outside the test's home"
        );
    }
    std::process::Command::new("/bin/sh")
        .args(["-e", "-c", &lines.join("\n")])
        .env_clear()
        .env("HOME", home)
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap()
}

/// [`shell_in`], which must succeed: what it printed.
pub(super) fn run_in(home: &Path, lines: &[String]) -> String {
    let ran = shell_in(home, lines);
    assert!(
        ran.status.success(),
        "{lines:?}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8_lossy(&ran.stdout).into_owned()
}

/// The Sawit pack is several skills Sawit ships: a guard for each folder written out, then each
/// copied into every agent's folder. It counts as installed only with every one there, each
/// Sawit's own and this Sawit's version; a copy that differs is kept aside, and a folder of one's
/// name that isn't Sawit's stops the install there.
#[test]
fn a_pack_of_several_built_in_skills() {
    use bundled::Held;
    let pack = get(SAWIT_PACK).unwrap();
    let n = pack.skills.len();
    assert_eq!((pack.methods, n), (&[Method::Bundled][..], 5));
    assert_eq!(pack.bundled(), Some(pack.skills));
    let from =
        |skill: &str| format!("\"$HOME/Library/Application Support/sawit/bundled-skills/{skill}\"");
    let sample = sample(pack, Method::Bundled, PackageManager::Npm, &env());
    assert_eq!(sample.matches("cp -R ").count(), n, "{sample}");
    let target = claude(None);
    let none = Installed::default();
    let lines = script(
        pack,
        Method::Bundled,
        Action::Install,
        &[(&target, &none)],
        PackageManager::Npm,
        &env(),
    )
    .unwrap();
    assert_eq!(lines.len(), n + 1 + 2 * n, "{lines:#?}");
    for (ix, skill) in pack.skills.iter().enumerate() {
        assert_eq!(lines[ix], guard(&from(skill)));
        let copy = format!("cp -R {} \"$HOME/.claude/skills\"/", from(skill));
        assert!(lines.contains(&copy), "{copy}");
    }

    let mut some = Installed::default();
    some.bundled.insert("sawit".into(), Held::Same);
    assert_eq!(status(pack, &some), Status::Partial { have: 1, of: n });
    for skill in pack.skills {
        some.bundled.insert(skill.to_string(), Held::Same);
    }
    assert_eq!(status(pack, &some), Status::Installed { plugin: false });
    some.bundled.insert("sawit-canvas".into(), Held::Differs);
    assert_eq!(status(pack, &some), Status::Differs);
    assert_eq!(
        kept(pack, &[(&target, &some)], &env()),
        [home().join("Library/Application Support/sawit/skill-backups/claude-code/sawit-canvas")]
    );
    some.bundled.insert("sawit".into(), Held::Other);
    assert_eq!(status(pack, &some), Status::Partial { have: n - 1, of: n });
    let refused = script(
        pack,
        Method::Bundled,
        Action::Install,
        &[(&target, &some)],
        PackageManager::Npm,
        &env(),
    )
    .unwrap_err();
    assert!(
        refused.contains("a sawit folder that isn't Sawit's skill"),
        "{refused}"
    );
}
