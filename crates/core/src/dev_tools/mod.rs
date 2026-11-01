//! Developer tools that install with a one-line cURL script (Claude Code,
//! Bun, uv, …): the catalog, which of them are already installed, the order
//! to install a selection in (prerequisites first), the latest released
//! versions, and running an installer, upgrade or uninstaller while
//! streaming its output.

mod catalog;
mod detect;
mod programs;
mod removal;
mod script;
mod versions;

pub use catalog::TOOLS;
pub use detect::{Detection, Found, Outside, detect};
pub use programs::{find_program, is_cap_cli};
pub use removal::{
    Impact, Removal, Uninstall, impact, in_terminal, installed_outside, installed_with,
    needed_by_warning, uninstall_command, uninstall_plan,
};
pub use script::{install, run, run_shell, uninstall};
pub use versions::{is_newer, latest_versions, newer, upgrade_plan};

use crate::package_manager::PackageManager;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    PackageManager,
    Runtime,
    Agent,
    Other,
}

impl Category {
    pub const ALL: [Category; 4] = [
        Category::PackageManager,
        Category::Runtime,
        Category::Agent,
        Category::Other,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::PackageManager => "PACKAGE MANAGERS",
            Category::Runtime => "LANGUAGES & RUNTIMES",
            Category::Agent => "AI AGENTS",
            Category::Other => "TERMINAL & APPS",
        }
    }
}

#[derive(Debug)]
pub struct Tool {
    /// Stable id, also the logo's key.
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: Category,
    /// The command that shows it is installed; empty for an app that has
    /// none, found at its `also` paths only.
    pub bin: &'static str,
    /// Where it lands when the installer doesn't put it on `PATH` (`~` is
    /// `$HOME`).
    pub also: &'static [&'static str],
    /// Besides the `also` paths themselves, the folders (or files) the
    /// installer puts it in, matched by the path found or, for a link, the
    /// path it leads to; with a leading `*`, a part of that real path. A copy
    /// found anywhere else was installed another way (Homebrew's cask of an
    /// app, pyenv's Python), and `uninstall` isn't run on it.
    pub owns: &'static [&'static str],
    /// The install command, run by the login shell.
    pub install: &'static str,
    /// The command that removes what `install` put in place, when there is
    /// one (npm ships with Node.js, so it has none).
    pub uninstall: Option<&'static str>,
    /// Ids of tools that must be installed first.
    pub needs: &'static [&'static str],
    /// Asks for the user's password, so it runs in the terminal instead.
    pub interactive: bool,
    /// `bin --version` prints something useful (and returns quickly); for an
    /// app bundle found, its `Info.plist` gives the version instead.
    pub version: bool,
    /// The Homebrew cask that installs it, an app: the copy found is
    /// Homebrew's, and `uninstall` runs on it, only while Homebrew's Caskroom
    /// has the cask. A copy dragged in from a disk image is left alone.
    pub cask: Option<&'static str>,
    /// Where its latest release is published.
    pub latest: Latest,
    /// Updates an installed copy; `install` when `None`.
    pub upgrade: Option<&'static str>,
}

/// Where a tool's latest version can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Latest {
    None,
    /// An npm package name.
    Npm(&'static str),
    /// A GitHub `owner/repo`; its latest release's tag.
    GitHub(&'static str),
    /// A Homebrew cask's token: the version Homebrew's API has for it, which
    /// is what `brew upgrade --cask` installs.
    Cask(&'static str),
}

impl Tool {
    /// The command that brings an installed copy up to date.
    pub fn upgrade_command(&self) -> &'static str {
        self.upgrade.unwrap_or(self.install)
    }
}

/// The npm package a tool installs with `npm install -g` (Gemini CLI, the
/// Cloudflare CLI): those install with whichever package manager is picked.
/// Not npm's own update.
pub fn npm_package(tool: &Tool) -> Option<&'static str> {
    let package = tool.install.strip_prefix("npm install -g ")?;
    (tool.id != "npm" && !package.contains(char::is_whitespace)).then_some(package)
}

/// `tool`'s install command with `pm` (`npm install -g x` → `bun add -g
/// x`); the catalog's own for a tool npm doesn't install, or without `pm`.
pub fn install_command(tool: &Tool, pm: Option<PackageManager>) -> String {
    match (npm_package(tool), pm) {
        (Some(package), Some(pm)) => pm.global_install(package),
        _ => tool.install.to_string(),
    }
}

/// `tool`'s upgrade command with `pm`, like [`install_command`]: installing
/// the package again brings the latest.
pub fn upgrade_command(tool: &Tool, pm: Option<PackageManager>) -> String {
    match (npm_package(tool), pm) {
        (Some(package), Some(pm)) => pm.global_install(package),
        _ => tool.upgrade_command().to_string(),
    }
}

pub fn find(id: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.id == id)
}

/// The order to install `wanted` in: every missing prerequisite before the
/// tools that need it, each tool once. Installed tools are left out.
pub fn plan(wanted: &[&str], installed: &dyn Fn(&str) -> bool) -> Vec<&'static Tool> {
    fn visit(id: &str, installed: &dyn Fn(&str) -> bool, out: &mut Vec<&'static Tool>) {
        let Some(tool) = find(id) else { return };
        if out.iter().any(|t| t.id == id) {
            return;
        }
        for need in tool.needs {
            if !installed(need) {
                visit(need, installed, out);
            }
        }
        out.push(tool);
    }
    let mut out = Vec::new();
    for id in wanted {
        visit(id, installed, &mut out);
    }
    out
}

/// The tools that need `id` installed first.
pub fn dependents(id: &str) -> Vec<&'static Tool> {
    TOOLS.iter().filter(|t| t.needs.contains(&id)).collect()
}
