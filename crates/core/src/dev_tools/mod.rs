//! Developer tools that install with a one-line cURL script (Claude Code,
//! Bun, uv, …): the catalog, which of them are already installed, the order
//! to install a selection in (prerequisites first), the latest released
//! versions, and running an installer, upgrade or uninstaller while
//! streaming its output.

mod catalog;
mod detect;
pub mod extras;
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
pub use script::{Lock, install, lock, run, run_shell, run_shell_holding, uninstall};
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
    /// The Homebrew formula that installs it (`owner/tap/name` for a tap's). The copy found is
    /// Homebrew's when it lies in Homebrew's Cellar: only that copy is upgraded (`brew upgrade`
    /// of the formula its install receipt names) or removed; any other (the `.pkg` from the
    /// tool's site, MacPorts, conda) is installed outside Homebrew and left alone.
    pub formula: Option<&'static str>,
    /// Text its version output has (any case) when the command found is this tool: another
    /// program by the same name (Midnight Commander's `mc`) doesn't count as installed.
    pub signature: Option<&'static str>,
    /// What prints its version: `--version`, or `-version` for FFmpeg.
    pub version_arg: &'static str,
    /// Runs only on a Mac with Apple silicon: an Intel Mac shows "Needs an Apple Silicon Mac"
    /// instead of an install that fails.
    pub apple_silicon: bool,
    /// Its install builds from source and takes minutes: the page runs it (and its upgrade) in
    /// a terminal, where its progress shows. Agents' tools run it as usual.
    pub long: bool,
    /// The user's settings and data, which uninstalling leaves in place (`~` is `$HOME`): the
    /// page offers deleting them as a choice of its own, confirmed separately.
    pub keeps: &'static [&'static str],
    /// What uninstalling also stops, said before it runs ("Agents running in herdr-core
    /// stop").
    pub warning: Option<&'static str>,
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
    /// A release manifest: a JSON file whose top-level `version` is the latest (Herdr's
    /// `releases.herdr.dev/app/latest.json`, which its installer and updater read).
    Manifest(&'static str),
    /// A crate on crates.io: its newest stable version, what `cargo install` builds.
    Crate(&'static str),
}

impl Tool {
    /// The command that brings an installed copy up to date.
    pub fn upgrade_command(&self) -> &'static str {
        self.upgrade.unwrap_or(self.install)
    }

    /// Why it can't install on this Mac, when it can't: "Needs an Apple Silicon Mac".
    pub fn unsupported(&self) -> Option<&'static str> {
        (self.apple_silicon && !apple_silicon()).then_some("Needs an Apple Silicon Mac")
    }
}

/// Whether this Mac has Apple silicon, also when Sawit runs under Rosetta (which reports
/// x86_64, as the Herdr installer notes): `hw.optional.arm64` is 1.
pub fn apple_silicon() -> bool {
    if cfg!(target_arch = "aarch64") {
        return true;
    }
    static ARM: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ARM.get_or_init(|| {
        std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.optional.arm64"])
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "1")
    })
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
/// the package again brings the latest, unless the tool has an upgrade of its
/// own (`hyperframes update`), which knows how it was installed.
pub fn upgrade_command(tool: &Tool, pm: Option<PackageManager>) -> String {
    match (npm_package(tool), pm, tool.upgrade) {
        (Some(package), Some(pm), None) => pm.global_install(package),
        _ => tool.upgrade_command().to_string(),
    }
}

/// What upgrades the copy of `tool` found at `found` (or, with none found, its own upgrade):
/// `brew upgrade` of the formula a Homebrew copy was installed from (`minio-mc` from
/// Homebrew's core, `minio/stable/mc` from MinIO's tap), else [`upgrade_command`] with the
/// package manager that installed it. What the page, `upgrade_dev_tools` and `sawit pupuk`
/// run.
pub fn upgrade_for(tool: &Tool, found: Option<&std::path::Path>) -> String {
    let Some(found) = found else {
        return tool.upgrade_command().to_string();
    };
    if tool.formula.is_some()
        && let Some(formula) = removal::brew_formula(found)
    {
        return format!("brew upgrade {formula}");
    }
    upgrade_command(tool, Some(installed_with(found)))
}

/// The places of `tool`'s [`Tool::keeps`] under `home` that are there: what deleting its
/// settings would remove.
pub fn kept(tool: &Tool, home: &std::path::Path) -> Vec<std::path::PathBuf> {
    tool.keeps
        .iter()
        .map(|p| programs::expand_in(p, home))
        .filter(|p| p.exists())
        .collect()
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
