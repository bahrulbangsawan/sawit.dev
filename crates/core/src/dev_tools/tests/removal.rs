//! Uninstalling: whose copy was found, the command that removes it, and the plan's order and
//! warnings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dev_tools::removal::{
    installed_outside_at, uninstall_command_at, uninstall_command_in, uninstall_plan_with,
};

use super::*;

/// PureMac's copy is Homebrew's, and `brew uninstall --cask` runs on it, only where the cask
/// puts the app and while Homebrew's Caskroom has the cask; one dragged in from the disk image
/// is left alone, as is a copy anywhere else.
#[test]
fn a_cask_app_is_removed_only_when_homebrew_installed_it() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    let rooms = [dir.path().join("Caskroom")];
    let caskroom = &rooms[0];
    std::fs::create_dir_all(caskroom).unwrap();
    let puremac = find("puremac").unwrap();
    let script = |found: &Path| uninstall_command_at(puremac, found, found, &home, &rooms);
    let system = Path::new("/Applications/PureMac.app");
    let user = home.join("Applications/PureMac.app");
    let elsewhere = home.join("Downloads/PureMac.app");
    // Dragged in from the disk image: no Caskroom entry.
    assert_eq!(script(system), None);
    assert_eq!(script(&user), None);
    std::fs::create_dir_all(caskroom.join("puremac/3.2.0")).unwrap();
    let brew = Some("brew uninstall --cask puremac".to_string());
    assert_eq!(script(system), brew);
    assert_eq!(script(&user), brew);
    assert_eq!(script(&elsewhere), None);
    // Another cask's Caskroom folder isn't PureMac's.
    std::fs::remove_dir_all(caskroom.join("puremac")).unwrap();
    std::fs::create_dir_all(caskroom.join("pesty")).unwrap();
    assert_eq!(script(system), None);
    // As the page and `sawit cabut` work it out: left alone, or run.
    let found = HashMap::from([(
        "puremac",
        Found {
            path: system.to_path_buf(),
            version: Some("3.2.0".into()),
            outside: None,
        },
    )]);
    let plan = || {
        uninstall_plan_with(&["puremac"], &found, &|tool, path| {
            uninstall_command_at(tool, path, path, &home, &rooms)
        })
    };
    assert_eq!(plan()[0].removal, Removal::LeftAlone);
    std::fs::create_dir_all(caskroom.join("puremac")).unwrap();
    assert_eq!(
        plan()[0].removal,
        Removal::Run("brew uninstall --cask puremac".into())
    );
    // A cask's Caskroom doesn't make another app Homebrew's: Zed's app, with no command of
    // its install script leading into it, is left alone.
    assert_eq!(
        uninstall_command_at(
            find("zed").unwrap(),
            Path::new("/Applications/Zed.app"),
            Path::new("/Applications/Zed.app"),
            &home,
            &rooms
        ),
        None
    );
}

/// An app found at its place is its install script's copy only while the command the script
/// links leads into it (Zed's ~/.local/bin/zed, Cap's ~/.cap/bin/cap): then Sawit upgrades
/// and removes it. Without one it was dragged in from the disk image and keeps itself up to
/// date: "Installed outside its install script", no Upgrade and no Uninstall. A cask's app
/// without its Caskroom entry is "Installed outside Homebrew". Other tools are as before.
#[test]
fn an_app_is_its_scripts_only_with_its_command_linked() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("me");
    let rooms = [dir.path().join("Caskroom")];
    std::fs::create_dir_all(&rooms[0]).unwrap();
    let app = |name: &str, program: &str| {
        let app = home.join(format!("Applications/{name}.app"));
        let macos = app.join("Contents/MacOS");
        std::fs::create_dir_all(&macos).unwrap();
        std::fs::write(macos.join(program), "").unwrap();
        app
    };
    let link = |from: &str, to: &Path| {
        let from = home.join(from);
        std::fs::create_dir_all(from.parent().unwrap()).unwrap();
        let _ = std::fs::remove_file(&from);
        std::os::unix::fs::symlink(to, from).unwrap();
    };
    let (uninstall, outside) = (
        |id: &str, found: &Path| {
            let real = std::fs::canonicalize(found).unwrap();
            uninstall_command_at(find(id).unwrap(), found, &real, &home, &rooms)
        },
        |id: &str, found: &Path| {
            let real = std::fs::canonicalize(found).unwrap();
            installed_outside_at(find(id).unwrap(), found, &real, &home, &rooms)
        },
    );
    let own = |id: &str| find(id).unwrap().uninstall.map(String::from);

    let zed = app("Zed", "cli");
    assert_eq!(uninstall("zed", &zed), None);
    assert_eq!(outside("zed", &zed), Some(Outside::InstallScript));
    // A link into another Zed doesn't make this one the script's.
    let other = dir.path().join("elsewhere/Zed.app/Contents/MacOS");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("cli"), "").unwrap();
    link(".local/bin/zed", &other.join("cli"));
    assert_eq!(outside("zed", &zed), Some(Outside::InstallScript));
    link(".local/bin/zed", &zed.join("Contents/MacOS/cli"));
    assert_eq!(uninstall("zed", &zed), own("zed"));
    assert_eq!(outside("zed", &zed), None);
    // Found by that command, as detection usually finds it: the script's.
    assert_eq!(outside("zed", &home.join(".local/bin/zed")), None);

    // Cap's shim in ~/.cap/bin, which the login shell may not have on its PATH.
    let cap = app("Cap", "cap-cli");
    assert_eq!(outside("cap", &cap), Some(Outside::InstallScript));
    link(".cap/bin/cap", &cap.join("Contents/MacOS/cap-cli"));
    assert_eq!(uninstall("cap", &cap), own("cap"));
    assert_eq!(outside("cap", &cap), None);

    let puremac = app("PureMac", "PureMac");
    assert_eq!(outside("puremac", &puremac), Some(Outside::Homebrew));
    std::fs::create_dir_all(rooms[0].join("puremac/3.2.0")).unwrap();
    assert_eq!(outside("puremac", &puremac), None);

    // Not an app: whatever copy it is, its upgrade is offered as before.
    let bun = home.join("bin/bun");
    std::fs::create_dir_all(bun.parent().unwrap()).unwrap();
    std::fs::write(&bun, "").unwrap();
    assert_eq!(uninstall("bun", &bun), None);
    assert_eq!(outside("bun", &bun), None);
}

/// Removing Homebrew warns per kind of tool that needs it: an app from its cask keeps
/// working but won't get updates through Homebrew any more; a tool that runs through what's
/// removed stops working. A copy installed outside it (PureMac from its disk image) isn't
/// named at all.
#[test]
fn removing_homebrew_warns_per_kind_of_dependent() {
    let tool = |id: &str| find(id).unwrap();
    assert_eq!(impact(tool("puremac")), Impact::NoHomebrewUpdates);
    assert_eq!(impact(tool("gemini")), Impact::StopsWorking);
    assert_eq!(
        needed_by_warning(&[tool("puremac")]),
        "PureMac won't get updates through Homebrew any more"
    );
    assert_eq!(
        needed_by_warning(&[tool("npm"), tool("gemini")]),
        "npm (latest), Gemini CLI need it and will stop working"
    );
    assert_eq!(
        needed_by_warning(&[tool("node")]),
        "Node.js needs it and will stop working"
    );
    assert_eq!(
        needed_by_warning(&[tool("puremac"), tool("gemini")]),
        "Gemini CLI needs it and will stop working; PureMac won't get updates through Homebrew any more"
    );
    assert_eq!(needed_by_warning(&[]), "");

    let at = |path: &str, outside: Option<Outside>| Found {
        path: PathBuf::from(path),
        version: None,
        outside,
    };
    let mut found = HashMap::from([
        ("homebrew", at("/opt/homebrew/bin/brew", None)),
        ("puremac", at("/Applications/PureMac.app", None)),
    ]);
    let command = |_: &Tool, _: &Path| Some("uninstall.sh".to_string());
    let plan = uninstall_plan_with(&["homebrew"], &found, &command);
    assert_eq!(
        plan[0].needed_by.iter().map(|t| t.id).collect::<Vec<_>>(),
        ["puremac"]
    );
    assert_eq!(
        needed_by_warning(&plan[0].needed_by),
        "PureMac won't get updates through Homebrew any more"
    );
    found.get_mut("puremac").unwrap().outside = Some(Outside::Homebrew);
    let plan = uninstall_plan_with(&["homebrew"], &found, &command);
    assert!(plan[0].needed_by.is_empty());
}

#[test]
fn uninstall_follows_who_installed_it() {
    let home = std::path::Path::new("/Users/me");
    let p = std::path::Path::new;
    let uv = find("uv").unwrap();
    // Homebrew's copy: brew removes it, not uv's own script.
    assert_eq!(
        uninstall_command_in(
            uv,
            p("/opt/homebrew/bin/uv"),
            p("/opt/homebrew/Cellar/uv/0.12.1/bin/uv"),
            home
        )
        .as_deref(),
        Some("brew uninstall uv")
    );
    // The installer's copy under the home folder.
    assert_eq!(
        uninstall_command_in(
            uv,
            p("/Users/me/.local/bin/uv"),
            p("/Users/me/.local/bin/uv"),
            home
        ),
        uv.uninstall.map(String::from)
    );
    // Somewhere else: left alone.
    assert_eq!(
        uninstall_command_in(uv, p("/usr/local/bin/uv"), p("/usr/local/bin/uv"), home),
        None
    );
    // Homebrew itself, at its known location.
    let brew = find("homebrew").unwrap();
    assert!(
        uninstall_command_in(
            brew,
            p("/opt/homebrew/bin/brew"),
            p("/opt/homebrew/bin/brew"),
            home
        )
        .is_some_and(|c| c.contains("uninstall.sh"))
    );
}

/// "Under the home folder" isn't "put there by this tool's installer":
/// another manager's copy gets no removal script (or its own manager's),
/// never the catalog's, which would remove something else.
#[test]
fn uninstall_is_offered_only_for_the_copy_the_installer_made() {
    let home = std::path::Path::new("/Users/me");
    let p = std::path::Path::new;
    let script = |id: &str, found: &str, real: &str| {
        uninstall_command_in(find(id).unwrap(), p(found), p(real), home)
    };
    let own = |id: &str| find(id).unwrap().uninstall.map(String::from);
    // pyenv's and asdf's shims: `uv python uninstall --all` would remove
    // every uv Python and leave theirs.
    for shim in [
        "/Users/me/.pyenv/shims/python3",
        "/Users/me/.asdf/shims/python3",
    ] {
        assert_eq!(script("python", shim, shim), None, "{shim}");
    }
    assert_eq!(
        script(
            "python",
            "/Users/me/.local/bin/python3",
            "/Users/me/.local/share/uv/python/cpython-3.13.2-macos-aarch64-none/bin/python3.13"
        ),
        own("python")
    );
    // Claude Code from npm: npm removes it, not the native installer's script.
    let npm =
        "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/@anthropic-ai/claude-code/cli.js";
    assert_eq!(
        script(
            "claude-code",
            "/Users/me/.nvm/versions/node/v22.11.0/bin/claude",
            npm
        )
        .as_deref(),
        Some("npm uninstall -g @anthropic-ai/claude-code")
    );
    // Installed with Bun or pnpm: that one removes it.
    assert_eq!(
        script(
            "gemini",
            "/Users/me/.bun/bin/gemini",
            "/Users/me/.bun/install/global/node_modules/@google/gemini-cli/dist/index.js"
        )
        .as_deref(),
        Some("bun remove -g @google/gemini-cli")
    );
    assert_eq!(
        script(
            "cf",
            "/Users/me/Library/pnpm/cf",
            "/Users/me/Library/pnpm/global/5/node_modules/cf/bin/cf"
        )
        .as_deref(),
        Some("pnpm remove -g cf")
    );
    assert_eq!(
        script(
            "claude-code",
            "/Users/me/.local/bin/claude",
            "/Users/me/.local/share/claude/versions/2.0.14"
        ),
        own("claude-code")
    );
    // Capistrano's `cap` isn't Cap's; Cap's own is a link into Cap.app.
    let rbenv = "/Users/me/.rbenv/shims/cap";
    assert_eq!(script("cap", rbenv, rbenv), None);
    assert_eq!(
        script(
            "cap",
            "/Users/me/.local/bin/cap",
            "/Applications/Cap.app/Contents/MacOS/cap-cli"
        ),
        own("cap")
    );
    // Bun from npm, and omp from bun: not where their installers put them.
    assert_eq!(
        script(
            "bun",
            "/Users/me/.nvm/versions/node/v22.11.0/bin/bun",
            "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/bun/bin/bun.exe"
        ),
        None
    );
    assert_eq!(
        script("omp", "/Users/me/.bun/bin/omp", "/Users/me/.bun/bin/omp"),
        None
    );
    assert_eq!(
        script("bun", "/Users/me/.bun/bin/bun", "/Users/me/.bun/bin/bun"),
        own("bun")
    );
    // Zed's CLI from its installer, and from Homebrew's cask, which links the same app.
    let zed_cli = "/Applications/Zed.app/Contents/MacOS/cli";
    assert_eq!(
        script("zed", "/Users/me/.local/bin/zed", zed_cli),
        own("zed")
    );
    assert_eq!(script("zed", "/opt/homebrew/bin/zed", zed_cli), None);
    // The app found on its own, with no ~/.local/bin/zed leading into it: dragged in from
    // the disk image, not the script's (an_app_is_its_scripts_only_with_its_command_linked).
    assert_eq!(
        script("zed", "/Applications/Zed.app", "/Applications/Zed.app"),
        None
    );
    // A Cellar folder whose name isn't a formula's isn't put in a command.
    assert_eq!(
        script(
            "uv",
            "/opt/homebrew/bin/uv",
            "/opt/homebrew/Cellar/uv;touch${IFS}x/0.1/bin/uv"
        ),
        None
    );
}

/// npm comes with Node.js and has no uninstaller of its own. Homebrew's
/// npm is a file of its `node` formula, so `brew uninstall node` would
/// take Node.js with it: npm gets no command, wherever it was found.
#[test]
fn npm_is_never_uninstalled_on_its_own() {
    let home = std::path::Path::new("/Users/me");
    let p = std::path::Path::new;
    let npm = find("npm").unwrap();
    assert_eq!(
        uninstall_command_in(
            npm,
            p("/opt/homebrew/bin/npm"),
            p("/opt/homebrew/Cellar/node/24.1.0/lib/node_modules/npm/bin/npm-cli.js"),
            home
        ),
        None
    );
    let nvm = "/Users/me/.nvm/versions/node/v22.11.0/bin/npm";
    assert_eq!(uninstall_command_in(npm, p(nvm), p(nvm), home), None);
}

/// `uninstall_dev_tools` and `sawit cabut` run only what the page's trash
/// button would: the command for the copy the tool's installer, Homebrew
/// or npm put in place, never one for a copy installed another way.
/// Homebrew's own uninstaller goes to a terminal, a tool goes before the
/// tools it needs, and the installed tools left behind that need one are
/// named.
#[test]
fn an_uninstall_runs_only_what_the_page_would_and_in_order() {
    let home = std::path::Path::new("/Users/me");
    let gemini = "/Users/me/.nvm/versions/node/v22.11.0/bin/gemini";
    // What a found path leads to, as canonicalize would say.
    let real = |path: &Path| match path.to_str() {
        Some(p) if p == gemini => PathBuf::from(
            "/Users/me/.nvm/versions/node/v22.11.0/lib/node_modules/@google/gemini-cli/dist/index.js",
        ),
        _ => path.to_path_buf(),
    };
    let command = |tool: &Tool, path: &Path| uninstall_command_in(tool, path, &real(path), home);
    let at = |path: &str| Found {
        path: PathBuf::from(path),
        version: None,
        outside: None,
    };
    let found: HashMap<&'static str, Found> = HashMap::from([
        ("homebrew", at("/opt/homebrew/bin/brew")),
        ("nvm", at("/Users/me/.nvm/nvm.sh")),
        ("node", at("/Users/me/.nvm/versions/node/v22.11.0/bin/node")),
        ("npm", at("/Users/me/.nvm/versions/node/v22.11.0/bin/npm")),
        ("gemini", at(gemini)),
        ("uv", at("/Users/me/.local/bin/uv")),
        ("python", at("/Users/me/.local/bin/python3")),
        // Capistrano's, from rbenv: not Cap's.
        ("cap", at("/Users/me/.rbenv/shims/cap")),
        ("bun", at("/Users/me/.bun/bin/bun")),
    ]);
    let own = |id: &str| find(id).unwrap().uninstall.unwrap().to_string();
    let ids = |tools: &[&'static Tool]| tools.iter().map(|t| t.id).collect::<Vec<_>>();

    let wanted = [
        "uv", "cap", "homebrew", "nvm", "gemini", "npm", "bun", "bun", "nope", "deno",
    ];
    let plan = uninstall_plan_with(&wanted, &found, &command);
    let got: Vec<(&str, Removal)> = plan
        .iter()
        .map(|u| (u.tool.id, u.removal.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            // They need Node.js, which nvm installs: before both.
            (
                "gemini",
                Removal::Run("npm uninstall -g @google/gemini-cli".into())
            ),
            ("npm", Removal::Bundled(vec!["Node.js"])),
            ("uv", Removal::Run(own("uv"))),
            ("cap", Removal::LeftAlone),
            ("homebrew", Removal::Terminal(own("homebrew"))),
            ("nvm", Removal::Run(own("nvm"))),
            ("bun", Removal::Run(own("bun"))),
            ("deno", Removal::Missing),
        ]
    );
    assert_eq!(
        plan[3].found,
        Some(PathBuf::from("/Users/me/.rbenv/shims/cap"))
    );
    assert_eq!(plan[7].found, None);
    // uv's Python and nvm's Node.js stay, and stop working.
    let needed: Vec<(&str, Vec<&str>)> = plan
        .iter()
        .map(|u| (u.tool.id, ids(&u.needed_by)))
        .collect();
    assert_eq!(
        needed,
        vec![
            ("gemini", vec![]),
            ("npm", vec![]),
            ("uv", vec!["python"]),
            ("cap", vec![]),
            ("homebrew", vec![]),
            ("nvm", vec!["node"]),
            ("bun", vec![]),
            ("deno", vec![]),
        ]
    );

    // Removed together, nothing is left needing them but npm, which comes with Node.js.
    let plan = uninstall_plan_with(&["nvm", "node", "gemini"], &found, &command);
    let order: Vec<&str> = plan.iter().map(|u| u.tool.id).collect();
    assert_eq!(order, ["gemini", "node", "nvm"]);
    assert_eq!(ids(&plan[1].needed_by), ["npm"]);
    assert!(plan[0].needed_by.is_empty() && plan[2].needed_by.is_empty());
}
