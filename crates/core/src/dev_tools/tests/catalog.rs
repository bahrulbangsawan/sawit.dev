//! The catalog: unique ids, prerequisites, uninstallers, the apps, and install commands.

use crate::dev_tools::detect::{MARK, detect_script};
use crate::dev_tools::removal::is_app_tool;
use crate::package_manager::PackageManager;

use super::*;

#[test]
fn ids_are_unique_and_needs_exist() {
    for (ix, t) in TOOLS.iter().enumerate() {
        assert!(
            TOOLS[..ix].iter().all(|o| o.id != t.id),
            "duplicate {}",
            t.id
        );
        for need in t.needs {
            assert!(find(need).is_some(), "{} needs unknown {need}", t.id);
        }
    }
}

#[test]
fn everything_but_npm_can_be_uninstalled() {
    for t in TOOLS {
        assert_eq!(t.uninstall.is_none(), t.id == "npm", "{}", t.id);
    }
    let ids: Vec<_> = dependents("node").iter().map(|t| t.id).collect();
    assert_eq!(ids, ["npm", "gemini", "cf"]);
}

/// PureMac is the app from its Homebrew cask, as its README installs it: Homebrew first,
/// then `brew install --cask`, upgraded and removed with brew, whatever package manager is
/// picked. It has no command of its own (`puremac` is the separate PureMac CLI), so
/// detection looks for the app only.
#[test]
fn puremac_is_the_app_from_its_homebrew_cask() {
    let puremac = find("puremac").unwrap();
    assert_eq!(puremac.category, Category::Other);
    assert_eq!(puremac.cask, Some("puremac"));
    assert_eq!(puremac.latest, Latest::Cask("puremac"));
    assert_eq!(puremac.needs, ["homebrew"]);
    assert!(!puremac.interactive);
    for pm in [None, Some(PackageManager::Bun), Some(PackageManager::Pnpm)] {
        assert_eq!(install_command(puremac, pm), "brew install --cask puremac");
        assert_eq!(upgrade_command(puremac, pm), "brew upgrade --cask puremac");
    }
    assert_eq!(npm_package(puremac), None);
    assert_eq!(puremac.uninstall, Some("brew uninstall --cask puremac"));
    assert_eq!(
        puremac.also,
        ["/Applications/PureMac.app", "~/Applications/PureMac.app"]
    );
    let ids = |tools: Vec<&Tool>| tools.iter().map(|t| t.id).collect::<Vec<_>>();
    assert_eq!(ids(plan(&["puremac"], &|_| false)), ["homebrew", "puremac"]);
    assert_eq!(ids(plan(&["puremac"], &|id| id == "homebrew")), ["puremac"]);
    assert_eq!(ids(dependents("homebrew")), ["puremac"]);
    // No `command -v` for it: that would find the PureMac CLI.
    let script = detect_script();
    assert!(!script.contains(&format!("{MARK}puremac")), "{script}");
    assert!(!script.contains("command -v  "), "{script}");
    assert!(script.contains(&format!("{MARK}zed")), "{script}");
}

/// Every app of the catalog (an app bundle among its places) is one whose copy the rule of
/// `installed_outside` works out: Cap, Zed and PureMac, each with an uninstaller, which
/// that rule is built on.
#[test]
fn the_catalogs_apps_are_cap_zed_and_puremac() {
    let apps: Vec<&str> = TOOLS
        .iter()
        .filter(|t| is_app_tool(t))
        .map(|t| t.id)
        .collect();
    assert_eq!(apps, ["cap", "zed", "puremac"]);
    for id in apps {
        assert!(find(id).unwrap().uninstall.is_some(), "{id}");
    }
}

/// The tools npm installs globally install with the package manager picked, the way the cf
/// docs list them; npm's own update and the curl installers stay as they are.
#[test]
fn npm_tools_install_with_the_package_manager_picked() {
    let tool = |id: &str| find(id).unwrap();
    let npm_tools: Vec<&str> = TOOLS
        .iter()
        .filter(|t| npm_package(t).is_some())
        .map(|t| t.id)
        .collect();
    assert_eq!(npm_tools, ["gemini", "cf"]);
    assert_eq!(
        install_command(tool("cf"), Some(PackageManager::Bun)),
        "bun add -g cf"
    );
    assert_eq!(
        install_command(tool("gemini"), Some(PackageManager::Pnpm)),
        "pnpm add -g @google/gemini-cli"
    );
    assert_eq!(
        upgrade_command(tool("gemini"), Some(PackageManager::Yarn)),
        "yarn global add @google/gemini-cli"
    );
    assert_eq!(install_command(tool("cf"), None), "npm install -g cf");
    assert_eq!(
        install_command(tool("npm"), Some(PackageManager::Bun)),
        "npm install -g npm@latest"
    );
    assert_eq!(
        install_command(tool("bun"), Some(PackageManager::Pnpm)),
        tool("bun").install
    );
    assert_eq!(
        upgrade_command(tool("bun"), Some(PackageManager::Pnpm)),
        "bun upgrade"
    );
}

#[test]
fn plan_puts_missing_prerequisites_first() {
    let none = |_: &str| false;
    let ids: Vec<_> = plan(&["gemini", "npm", "python"], &none)
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, ["nvm", "node", "gemini", "npm", "uv", "python"]);
}

#[test]
fn plan_skips_installed_prerequisites() {
    let installed = |id: &str| id == "nvm" || id == "uv";
    let ids: Vec<_> = plan(&["gemini", "python"], &installed)
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, ["node", "gemini", "python"]);
}
