//! Coding agents' config files on this machine and in the open projects
//! (`~/.claude/settings.json`, `.codex/config.toml`, `AGENTS.md`, …), in
//! the order each agent applies them: a file overrides the ones after it,
//! and keys they don't share are merged.

use std::path::{Path, PathBuf};

use crate::mcp_clients::Client;
use crate::shell_config::writable;

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

const CLAUDE_CODE: &[Entry] = &[
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/managed-settings.json",
        "Settings from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/managed-mcp.json",
        "MCP servers from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/Library/Application Support/ClaudeCode/CLAUDE.md",
        "Instructions from your organization",
    ),
    (
        Layer::ProjectLocal,
        Base::Project,
        ".claude/settings.local.json",
        "Your settings for this project",
    ),
    (
        Layer::ProjectLocal,
        Base::Project,
        "CLAUDE.local.md",
        "Your instructions for this project",
    ),
    (
        Layer::Project,
        Base::Project,
        ".claude/settings.json",
        "Settings shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        ".mcp.json",
        "MCP servers shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        "CLAUDE.md",
        "Instructions shared with the team",
    ),
    (
        Layer::Project,
        Base::Project,
        ".claude/CLAUDE.md",
        "Instructions shared with the team",
    ),
    (
        Layer::User,
        Base::Dir,
        "settings.json",
        "Settings for every project",
    ),
    (
        Layer::User,
        Base::Dir,
        "CLAUDE.md",
        "Instructions for every project",
    ),
    (
        Layer::User,
        Base::Dir,
        "keybindings.json",
        "Keyboard shortcuts",
    ),
    (
        Layer::Global,
        Base::Dir,
        ".claude.json",
        "Account, MCP servers and /config state",
    ),
];

const CODEX: &[Entry] = &[
    (
        Layer::Managed,
        Base::Root,
        "/etc/codex/managed_config.toml",
        "Settings from your organization",
    ),
    (
        Layer::Managed,
        Base::Root,
        "/etc/codex/requirements.toml",
        "Limits from your organization",
    ),
    (
        Layer::Project,
        Base::Project,
        ".codex/config.toml",
        "Settings for this project, once trusted",
    ),
    (
        Layer::Project,
        Base::Project,
        "AGENTS.override.md",
        "Instructions, used instead of AGENTS.md",
    ),
    (
        Layer::Project,
        Base::Project,
        "AGENTS.md",
        "Instructions shared with the team",
    ),
    (
        Layer::User,
        Base::Dir,
        "config.toml",
        "Settings, profiles and MCP servers",
    ),
    (
        Layer::User,
        Base::Dir,
        "AGENTS.override.md",
        "Instructions, used instead of AGENTS.md",
    ),
    (
        Layer::User,
        Base::Dir,
        "AGENTS.md",
        "Instructions for every project",
    ),
    (Layer::User, Base::Dir, "hooks.json", "Hooks"),
    (
        Layer::System,
        Base::Root,
        "/etc/codex/config.toml",
        "Settings for every user",
    ),
];

/// Agents without profiles: `(client, entries)`.
const OTHERS: &[(Client, &[Entry])] = &[
    (
        Client::Gemini,
        &[
            (
                Layer::Managed,
                Base::Root,
                "/Library/Application Support/GeminiCli/settings.json",
                "System settings; override everything",
            ),
            (
                Layer::Project,
                Base::Project,
                ".gemini/settings.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "GEMINI.md",
                "Instructions shared with the team",
            ),
            (
                Layer::Project,
                Base::Project,
                ".gemini/.env",
                "Environment for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/GEMINI.md",
                "Instructions for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".gemini/.env",
                "Environment for every project",
            ),
            (
                Layer::System,
                Base::Root,
                "/Library/Application Support/GeminiCli/system-defaults.json",
                "Defaults for every user",
            ),
        ],
    ),
    (
        Client::Qwen,
        &[
            (
                Layer::Project,
                Base::Project,
                ".qwen/settings.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "QWEN.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".qwen/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".qwen/QWEN.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::OpenCode,
        &[
            (
                Layer::Project,
                Base::Project,
                "opencode.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "opencode.jsonc",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "AGENTS.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/opencode.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/opencode.jsonc",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/opencode/AGENTS.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::Amp,
        &[
            (
                Layer::Project,
                Base::Project,
                "AGENTS.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/amp/settings.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/amp/AGENTS.md",
                "Instructions for every project",
            ),
        ],
    ),
    (
        Client::CopilotCli,
        &[
            (
                Layer::Project,
                Base::Project,
                ".github/copilot-instructions.md",
                "Instructions shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".copilot/config.json",
                "Settings for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".copilot/mcp-config.json",
                "MCP servers",
            ),
        ],
    ),
    (
        Client::Cursor,
        &[
            (
                Layer::Project,
                Base::Project,
                ".cursor/mcp.json",
                "MCP servers for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                ".cursor/cli.json",
                "CLI permissions for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                ".cursorrules",
                "Rules shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".cursor/mcp.json",
                "MCP servers for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".cursor/cli-config.json",
                "CLI settings",
            ),
        ],
    ),
    (
        Client::Windsurf,
        &[
            (
                Layer::Project,
                Base::Project,
                ".windsurfrules",
                "Rules shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".codeium/windsurf/mcp_config.json",
                "MCP servers",
            ),
            (
                Layer::User,
                Base::Home,
                ".codeium/windsurf/memories/global_rules.md",
                "Rules for every project",
            ),
        ],
    ),
    (
        Client::Kiro,
        &[
            (
                Layer::Project,
                Base::Project,
                ".kiro/settings/mcp.json",
                "MCP servers for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".kiro/settings/mcp.json",
                "MCP servers for every project",
            ),
            (
                Layer::User,
                Base::Home,
                ".kiro/settings/cli.json",
                "CLI settings",
            ),
        ],
    ),
    (
        Client::Goose,
        &[
            (
                Layer::Project,
                Base::Project,
                ".goosehints",
                "Hints shared with the team",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/goose/config.yaml",
                "Settings and extensions",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/goose/.goosehints",
                "Hints for every project",
            ),
        ],
    ),
    (
        Client::Crush,
        &[
            (
                Layer::Project,
                Base::Project,
                ".crush.json",
                "Settings for this project",
            ),
            (
                Layer::Project,
                Base::Project,
                "crush.json",
                "Settings for this project",
            ),
            (
                Layer::User,
                Base::Home,
                ".config/crush/crush.json",
                "Settings for every project",
            ),
        ],
    ),
    (
        Client::ClaudeDesktop,
        &[(
            Layer::User,
            Base::Home,
            "Library/Application Support/Claude/claude_desktop_config.json",
            "MCP servers and settings",
        )],
    ),
];

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

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "{}\n").unwrap();
    }

    #[test]
    fn lists_existing_files_by_agent_highest_priority_first() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let root = dir.path().join("root");
        let project = dir.path().join("app");
        touch(&home.join(".claude/settings.json"));
        touch(&home.join(".claude.json"));
        touch(&home.join(".claude-work/settings.json"));
        touch(&home.join(".claude-work/.claude.json"));
        touch(&home.join(".codex/config.toml"));
        touch(&root.join("Library/Application Support/ClaudeCode/managed-settings.json"));
        touch(&project.join(".claude/settings.local.json"));
        touch(&project.join("CLAUDE.md"));
        touch(&project.join("AGENTS.md"));
        // Gemini isn't installed, so its project file is left out.
        touch(&project.join("GEMINI.md"));

        let found = detect_in(&home, &root, std::slice::from_ref(&project));
        let got: Vec<_> = found
            .iter()
            .map(|c| (c.client, c.layer, c.path.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    Client::ClaudeCode,
                    Layer::Managed,
                    root.join("Library/Application Support/ClaudeCode/managed-settings.json")
                ),
                (
                    Client::ClaudeCode,
                    Layer::ProjectLocal,
                    project.join(".claude/settings.local.json")
                ),
                (
                    Client::ClaudeCode,
                    Layer::Project,
                    project.join("CLAUDE.md")
                ),
                (
                    Client::ClaudeCode,
                    Layer::User,
                    home.join(".claude/settings.json")
                ),
                (Client::ClaudeCode, Layer::Global, home.join(".claude.json")),
                // The work profile's own files, after the default's.
                (
                    Client::ClaudeCode,
                    Layer::User,
                    home.join(".claude-work/settings.json")
                ),
                (
                    Client::ClaudeCode,
                    Layer::Global,
                    home.join(".claude-work/.claude.json")
                ),
                (Client::Codex, Layer::Project, project.join("AGENTS.md")),
                (Client::Codex, Layer::User, home.join(".codex/config.toml")),
            ]
        );
        assert_eq!(found[1].project.as_ref(), Some(&project));
    }

    /// Profiles are the folders Claude Code or Codex ran in (they hold its state file), each
    /// its own set of files, not one more layer of the default's; a profile outside the home
    /// folder (`CLAUDE_CONFIG_DIR`) is one too.
    #[test]
    fn profiles_are_their_own_and_only_real_ones() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let root = dir.path().join("root");
        touch(&home.join(".claude/settings.json"));
        touch(&home.join(".claude.json"));
        touch(&home.join(".claude-work/settings.json"));
        touch(&home.join(".claude-work/.claude.json"));
        // A helper folder that happens to share the prefix: not a profile.
        touch(&home.join(".claude-pool/settings.json"));
        let elsewhere = dir.path().join("team/claude");
        touch(&elsewhere.join("settings.json"));
        touch(&elsewhere.join(".claude.json"));

        let found = detect_with(
            &home,
            &root,
            &[],
            &[(Client::ClaudeCode, elsewhere.clone())],
        );
        let got: Vec<_> = found
            .iter()
            .map(|c| (c.profile.as_deref(), c.path.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (None, home.join(".claude/settings.json")),
                (None, home.join(".claude.json")),
                (Some("work"), home.join(".claude-work/settings.json")),
                (Some("work"), home.join(".claude-work/.claude.json")),
                (Some("claude"), elsewhere.join("settings.json")),
                (Some("claude"), elsewhere.join(".claude.json")),
            ]
        );
    }
}
