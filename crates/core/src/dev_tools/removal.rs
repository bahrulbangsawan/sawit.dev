//! Uninstalling: whose copy was found (the tool's installer, Homebrew, npm or another way), the
//! command that removes it, and the plan the page, `uninstall_dev_tools` and `sawit cabut` share.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::package_manager::PackageManager;

use super::detect::{Found, Outside, is_app};
use super::programs::expand_in;
use super::{Latest, Tool, dependents, find, plan};

/// How to remove the copy of `tool` found at `found`: `brew uninstall`
/// when Homebrew put it there, `npm uninstall -g` when npm did, the tool's
/// own command when its installer did (at one of its known locations, or in
/// the folders it installs to: [`Tool::owns`]), and `None` for anything
/// else (pyenv's Python, Capistrano's `cap`, …), which Sawit leaves alone.
/// A tool with no uninstaller of its own (npm, which comes with Node.js)
/// gets none either: Homebrew's npm is its `node`, and removing it would
/// remove Node.js.
pub fn uninstall_command(tool: &Tool, found: &Path) -> Option<String> {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    uninstall_command_in(tool, found, &real, &home)
}

/// Homebrew's Caskroom, which has a folder for each cask installed: on Apple
/// silicon, and on Intel.
const CASKROOMS: [&str; 2] = ["/opt/homebrew/Caskroom", "/usr/local/Caskroom"];

pub(super) fn uninstall_command_in(
    tool: &Tool,
    found: &Path,
    real: &Path,
    home: &Path,
) -> Option<String> {
    uninstall_command_at(tool, found, real, home, &CASKROOMS.map(PathBuf::from))
}

/// [`uninstall_command_in`] with `caskrooms` for Homebrew's Caskroom.
pub(super) fn uninstall_command_at(
    tool: &Tool,
    found: &Path,
    real: &Path,
    home: &Path,
    caskrooms: &[PathBuf],
) -> Option<String> {
    let own = tool.uninstall?;
    // An app from a Homebrew cask, where the cask puts it: Homebrew's copy while its Caskroom
    // has the cask. Otherwise it was dragged in from a disk image, and `brew uninstall` would
    // fail on it.
    if let Some(token) = tool.cask {
        let at_its_place = tool.also.iter().any(|p| expand_in(p, home) == found);
        let homebrews = caskrooms.iter().any(|room| room.join(token).is_dir());
        return (at_its_place && homebrews).then(|| own.to_string());
    }
    // Another app is its install script's copy when the command the script links (Zed's
    // ~/.local/bin/zed, Cap's shims) leads into it. Without one, it was dragged in from a disk
    // image and keeps itself up to date: running the script again would replace it or put a
    // second copy beside it, and its uninstaller would delete it, so Sawit leaves it alone.
    if is_app(found) {
        let linked = tool.owns.iter().filter(|p| !p.starts_with('*')).any(|p| {
            std::fs::canonicalize(expand_in(p, home))
                .is_ok_and(|to| to.starts_with(found) || to.starts_with(real))
        });
        return linked.then(|| own.to_string());
    }
    let parts: Vec<_> = real
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    if tool.id != "homebrew"
        && let Some(ix) = parts.iter().position(|p| p == "Cellar")
        && let Some(formula) = parts.get(ix + 1)
    {
        // A folder name, into a shell command: only a formula's.
        let formula_like = formula
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "@._+-".contains(c));
        return formula_like.then(|| format!("brew uninstall {formula}"));
    }
    let real_text = real.to_string_lossy();
    // An npm package installed globally: the package manager that put it
    // there (npm, pnpm, Bun, Yarn) removes it.
    if let Latest::Npm(package) = tool.latest
        && tool.id != "npm"
        && real_text.contains(&format!("/node_modules/{package}/"))
    {
        return Some(PackageManager::owning(&real_text).global_uninstall(package));
    }
    // An `also` path is the copy itself (a command; an app is worked out
    // above). A command linked into an app from elsewhere (Homebrew's cask
    // of Zed links /opt/homebrew/bin/zed) isn't this one.
    let ours = tool.also.iter().any(|p| expand_in(p, home) == found)
        || tool.owns.iter().any(|p| match p.strip_prefix('*') {
            Some(part) => real_text.contains(part),
            None => {
                let p = expand_in(p, home);
                found.starts_with(&p) || real.starts_with(&p)
            }
        });
    ours.then(|| own.to_string())
}

/// Whether `tool` is an app: an app bundle is one of its places (Cap, Zed,
/// PureMac).
pub(super) fn is_app_tool(tool: &Tool) -> bool {
    tool.also.iter().any(|p| p.ends_with(".app"))
}

/// [`Found::outside`] for the copy of `tool` found at `found`. An app's
/// upgrade (its cask's `brew upgrade`, its install script again) and
/// uninstall act on the copy they put in place, so a copy that isn't that
/// one (by the rule of [`uninstall_command`]) gets neither: outside
/// Homebrew for a cask's app, outside its install script for another. Every
/// other tool's copy is `None`, as before.
pub fn installed_outside(tool: &Tool, found: &Path) -> Option<Outside> {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    installed_outside_at(tool, found, &real, &home, &CASKROOMS.map(PathBuf::from))
}

/// [`installed_outside`] with `caskrooms` for Homebrew's Caskroom.
pub(super) fn installed_outside_at(
    tool: &Tool,
    found: &Path,
    real: &Path,
    home: &Path,
    caskrooms: &[PathBuf],
) -> Option<Outside> {
    if !is_app_tool(tool) || uninstall_command_at(tool, found, real, home, caskrooms).is_some() {
        return None;
    }
    Some(if tool.cask.is_some() {
        Outside::Homebrew
    } else {
        Outside::InstallScript
    })
}

/// What losing a tool it needs does to `dependent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Impact {
    /// It runs through the tool removed (Gemini CLI through Node.js): it
    /// stops working.
    StopsWorking,
    /// An app from a Homebrew cask (PureMac): it keeps working, but
    /// Homebrew no longer updates it.
    NoHomebrewUpdates,
}

pub fn impact(dependent: &Tool) -> Impact {
    if dependent.cask.is_some() {
        Impact::NoHomebrewUpdates
    } else {
        Impact::StopsWorking
    }
}

/// The warning before removing a tool the installed tools `needed_by` need,
/// as the page, `uninstall_dev_tools` and `list_dev_tools` give it, without
/// its full stop: "npm (latest), Gemini CLI need it and will stop working",
/// "PureMac won't get updates through Homebrew any more", or both joined
/// with "; ". Empty when nothing needs it.
pub fn needed_by_warning(needed_by: &[&Tool]) -> String {
    let names = |which: Impact| -> Vec<&str> {
        needed_by
            .iter()
            .filter(|t| impact(t) == which)
            .map(|t| t.name)
            .collect()
    };
    let mut parts = Vec::new();
    let stop = names(Impact::StopsWorking);
    if !stop.is_empty() {
        let s = if stop.len() == 1 { "s" } else { "" };
        parts.push(format!(
            "{} need{s} it and will stop working",
            stop.join(", ")
        ));
    }
    let apps = names(Impact::NoHomebrewUpdates);
    if !apps.is_empty() {
        parts.push(format!(
            "{} won't get updates through Homebrew any more",
            apps.join(", ")
        ));
    }
    parts.join("; ")
}

/// The package manager that installed the copy of an npm tool found at
/// `found` (its links followed): what upgrades it, so an upgrade doesn't put
/// a second copy beside it with another one.
pub fn installed_with(found: &Path) -> PackageManager {
    let real = std::fs::canonicalize(found).unwrap_or_else(|_| found.to_path_buf());
    PackageManager::owning(&real.to_string_lossy())
}

/// Whether the uninstall `command` of `tool` asks for the user's password,
/// which only a terminal can answer: Homebrew's own uninstaller does, a
/// `brew uninstall` of one formula doesn't.
pub fn in_terminal(tool: &Tool, command: &str) -> bool {
    tool.interactive && !command.starts_with("brew ")
}

/// How [`uninstall_plan`] removes one tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removal {
    /// Detection found no copy: there is nothing to remove.
    Missing,
    /// The copy found wasn't put there by the tool's installer, Homebrew or
    /// npm (pyenv's Python, Capistrano's `cap`, …): Sawit leaves it alone
    /// ([`uninstall_command`]).
    LeftAlone,
    /// The tool has no uninstaller of its own: it comes with the tools it
    /// needs, named here (npm with Node.js), and goes when they go.
    Bundled(Vec<&'static str>),
    /// Its uninstaller asks for the user's password ([`in_terminal`]): the
    /// command, for the user to run in a terminal.
    Terminal(String),
    /// The command that removes the copy found.
    Run(String),
}

/// One tool of an uninstall, worked out as the Dev Tools page's trash button
/// works it out, for `uninstall_dev_tools` and `sawit cabut`.
#[derive(Debug, Clone)]
pub struct Uninstall {
    pub tool: &'static Tool,
    /// Where detection found it.
    pub found: Option<PathBuf>,
    pub removal: Removal,
    /// The installed tools that need it and stay, so they stop working once
    /// it's gone, or for a cask's app stop getting updates
    /// ([`needed_by_warning`]; the page names them before it asks). Empty
    /// when nothing is removed.
    pub needed_by: Vec<&'static Tool>,
}

/// What uninstalling the tools `wanted` does, given what [`detect`](fn@super::detect) found:
/// each tool of the catalog once, a tool before the ones it needs (Python's
/// uninstaller is `uv python uninstall`, so uv goes after it; Gemini CLI's
/// is `npm uninstall`, so nvm and Node.js go after it). Ids not in the
/// catalog are left out.
pub fn uninstall_plan(wanted: &[&str], found: &HashMap<&'static str, Found>) -> Vec<Uninstall> {
    uninstall_plan_with(wanted, found, &|tool, path| uninstall_command(tool, path))
}

/// [`uninstall_plan`] with `command` for [`uninstall_command`].
pub(super) fn uninstall_plan_with(
    wanted: &[&str],
    found: &HashMap<&'static str, Found>,
    command: &dyn Fn(&Tool, &Path) -> Option<String>,
) -> Vec<Uninstall> {
    let mut tools: Vec<&'static Tool> = Vec::new();
    for tool in wanted.iter().filter_map(|id| find(id)) {
        if !tools.iter().any(|t| t.id == tool.id) {
            tools.push(tool);
        }
    }
    // A tool needs fewer tools, directly or through another, than any tool
    // that needs it; the sort keeps the order given otherwise.
    tools.sort_by_key(|t| std::cmp::Reverse(plan(&[t.id], &|_| false).len()));
    let mut out: Vec<Uninstall> = tools
        .into_iter()
        .map(|tool| {
            let path = found.get(tool.id).map(|f| f.path.clone());
            let removal = match &path {
                None => Removal::Missing,
                Some(_) if tool.uninstall.is_none() => Removal::Bundled(
                    tool.needs
                        .iter()
                        .filter_map(|id| find(id))
                        .map(|t| t.name)
                        .collect(),
                ),
                Some(path) => match command(tool, path) {
                    None => Removal::LeftAlone,
                    Some(c) if in_terminal(tool, &c) => Removal::Terminal(c),
                    Some(c) => Removal::Run(c),
                },
            };
            Uninstall {
                tool,
                found: path,
                removal,
                needed_by: Vec::new(),
            }
        })
        .collect();
    let removed: Vec<&str> = out
        .iter()
        .filter(|u| matches!(u.removal, Removal::Run(_) | Removal::Terminal(_)))
        .map(|u| u.tool.id)
        .collect();
    for u in out.iter_mut().filter(|u| removed.contains(&u.tool.id)) {
        // A copy installed outside what the tool needs (PureMac from its disk image) doesn't
        // need it.
        u.needed_by = dependents(u.tool.id)
            .into_iter()
            .filter(|t| {
                found.get(t.id).is_some_and(|f| f.outside.is_none()) && !removed.contains(&t.id)
            })
            .collect();
    }
    out
}
