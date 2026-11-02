//! Coding agents' config files on this machine and in the open projects
//! (`~/.claude/settings.json`, `.codex/config.toml`, `AGENTS.md`, …), in
//! the order each agent applies them: a file overrides the ones after it,
//! and keys they don't share are merged.

mod files;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::mcp_clients::Client;
use crate::shell_config::writable;
use files::{CLAUDE_CODE, CODEX, OTHERS};

/// Where a file sits in its agent's order, highest priority first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// Set by an administrator for every user; overrides everything.
    Managed,
    /// In a project, for this user only (not committed).
    ProjectLocal,
    /// In a project, shared through its repository.
    Project,
    /// The user's own, for every project.
    User,
    /// State the agent writes itself (`~/.claude.json`).
    Global,
    /// Defaults for every user; anything above overrides them.
    System,
}

impl Layer {
    pub fn label(self) -> &'static str {
        match self {
            Layer::Managed => "Managed",
            Layer::ProjectLocal => "Project, only you",
            Layer::Project => "Project",
            Layer::User => "User",
            Layer::Global => "Global state",
            Layer::System => "System defaults",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    pub client: Client,
    pub layer: Layer,
    pub path: PathBuf,
    /// What the file holds.
    pub note: &'static str,
    /// The open project it belongs to, for project files.
    pub project: Option<PathBuf>,
    /// The profile it belongs to (`work` for `~/.claude-work`), for a
    /// profile's own files; `None` for the default profile's and the rest.
    pub profile: Option<String>,
    /// Saving it works without an admin password.
    pub writable: bool,
}

/// Where a file's path starts.
#[derive(Clone, Copy)]
enum Base {
    /// An absolute path.
    Root,
    /// The home folder.
    Home,
    /// The agent's own folder: `~/.claude`, `~/.codex`, or a profile of it.
    Dir,
    /// An open project's folder.
    Project,
}

type Entry = (Layer, Base, &'static str, &'static str);

/// The config files that exist, by agent in [`Client::ALL`] order, each
/// agent's highest priority first, then each of its other profiles' own
/// files: profiles are alternatives, not layers. `projects` are the open
/// projects' folders; an agent's project files are listed only when the
/// agent is installed (it has a file outside the projects).
pub fn detect(projects: &[PathBuf]) -> Vec<AgentConfig> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };
    // The profiles the environment names, wherever they are.
    let named: Vec<(Client, PathBuf)> = [
        (Client::ClaudeCode, "CLAUDE_CONFIG_DIR"),
        (Client::Codex, "CODEX_HOME"),
    ]
    .into_iter()
    .filter_map(|(client, var)| {
        let dir = std::env::var_os(var).filter(|d| !d.is_empty())?;
        Some((client, PathBuf::from(dir)))
    })
    .collect();
    detect_with(&home, Path::new("/"), projects, &named)
}

/// [`detect`] with the folders given, for tests. `root` stands in for `/`.
#[cfg(test)]
fn detect_in(home: &Path, root: &Path, projects: &[PathBuf]) -> Vec<AgentConfig> {
    detect_with(home, root, projects, &[])
}

/// [`detect`], with `named` the profile folders the environment names
/// (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`).
fn detect_with(
    home: &Path,
    root: &Path,
    projects: &[PathBuf],
    named: &[(Client, PathBuf)],
) -> Vec<AgentConfig> {
    let claude = profile_dirs(home, Client::ClaudeCode, ".claude", ".claude.json", named);
    let codex = profile_dirs(home, Client::Codex, ".codex", "config.toml", named);
    let mut agents: Vec<(Client, &[Entry], Vec<Profile>)> = vec![
        (Client::ClaudeCode, CLAUDE_CODE, claude),
        (Client::Codex, CODEX, codex),
    ];
    agents.extend(OTHERS.iter().map(|&(c, e)| (c, e, Vec::new())));
    agents.sort_by_key(|(c, _, _)| Client::ALL.iter().position(|a| a == c));

    let mut out = Vec::new();
    for (client, entries, dirs) in agents {
        // The default profile's files (with the agent's managed, system and
        // project ones), then each other profile's.
        let mut groups: Vec<Vec<AgentConfig>> = vec![Vec::new()];
        groups.extend(dirs.iter().filter(|(p, _)| p.is_some()).map(|_| Vec::new()));
        for &(layer, base, rel, note) in entries {
            let mut add = |group: usize,
                           path: PathBuf,
                           project: Option<&PathBuf>,
                           profile: Option<&String>| {
                // `~/.claude/.claude.json` is a profile's; the default
                // profile keeps it in the home folder.
                let path = match (client, rel) {
                    (Client::ClaudeCode, ".claude.json")
                        if path.parent() == Some(&home.join(".claude")) =>
                    {
                        home.join(".claude.json")
                    }
                    _ => path,
                };
                let found = &mut groups[group];
                if path.is_file() && !found.iter().any(|c: &AgentConfig| c.path == path) {
                    found.push(AgentConfig {
                        client,
                        layer,
                        writable: writable(&path),
                        path,
                        note,
                        project: project.cloned(),
                        profile: profile.cloned(),
                    });
                }
            };
            match base {
                Base::Root => add(0, root.join(rel.trim_start_matches('/')), None, None),
                Base::Home => add(0, home.join(rel), None, None),
                Base::Dir => {
                    let mut group = 0;
                    for (profile, dir) in &dirs {
                        if profile.is_some() {
                            group += 1;
                        }
                        add(group, dir.join(rel), None, profile.as_ref());
                    }
                }
                Base::Project => projects
                    .iter()
                    .for_each(|p| add(0, p.join(rel), Some(p), None)),
            }
        }
        let installed = groups[0].iter().any(|c| c.project.is_none())
            || groups[1..].iter().any(|g| !g.is_empty());
        if installed {
            for mut found in groups {
                found.sort_by_key(|c| c.layer);
                out.extend(found);
            }
        }
    }
    out
}

/// [`detect`]'s files in the groups the Config Files panel shows them in, as
/// `(agent, profile, files)`: each agent's files, then each of its other
/// profiles' own, every group highest priority first.
pub fn groups(configs: &[AgentConfig]) -> Vec<(Client, Option<&str>, Vec<&AgentConfig>)> {
    let mut out: Vec<(Client, Option<&str>, Vec<&AgentConfig>)> = Vec::new();
    for config in configs {
        let profile = config.profile.as_deref();
        match out.last_mut() {
            Some((client, p, files)) if *client == config.client && *p == profile => {
                files.push(config)
            }
            _ => out.push((config.client, profile, vec![config])),
        }
    }
    out
}

/// A group's name: the agent's, with a profile its name too ("Claude Code ·
/// work").
pub fn group_title(client: Client, profile: Option<&str>) -> String {
    match profile {
        None => client.name().into(),
        Some(profile) => format!("{} · {profile}", client.name()),
    }
}

/// A profile's name (`None` for the default one) and its folder.
type Profile = (Option<String>, PathBuf);

/// `~/<prefix>` (the default profile, `None`) and the profiles: folders
/// `~/<prefix>-<name>` the agent ran in (they hold its `marker` file, as
/// the MCP page requires), and those `named` by the environment.
fn profile_dirs(
    home: &Path,
    client: Client,
    prefix: &str,
    marker: &str,
    named: &[(Client, PathBuf)],
) -> Vec<Profile> {
    let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let mut dirs: Vec<Profile> = Vec::new();
    let default = home.join(prefix);
    if default.is_dir() {
        dirs.push((None, default));
    }
    for (name, dir) in crate::mcp_clients::profiles(home, prefix, marker) {
        dirs.push((Some(name), dir));
    }
    for (c, dir) in named {
        if *c != client
            || !dir.join(marker).exists()
            || dirs.iter().any(|(_, d)| canonical(d) == canonical(dir))
        {
            continue;
        }
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
            .unwrap_or_else(|| "profile".into());
        dirs.push((Some(name), dir.clone()));
    }
    dirs
}
