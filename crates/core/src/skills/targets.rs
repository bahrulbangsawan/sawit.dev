//! The skills folders of the agents on this Mac: each Claude Code profile's own, one per folder
//! the other agents share, and finding them by name.

use std::path::{Path, PathBuf};

use crate::mcp_clients::{self, Client};

/// A Claude Code profile: its config folder (`~/.claude`, or the profile's
/// `CLAUDE_CONFIG_DIR`), where its skills, plugins and marketplaces live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHome {
    pub dir: PathBuf,
    /// The folder `CLAUDE_CONFIG_DIR` names; `None` for the default profile,
    /// whose commands run with the variable unset.
    pub profile_dir: Option<PathBuf>,
}

/// One skills folder and the agents that read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// "Claude Code", "Claude Code · work", "Codex, Zed".
    pub label: String,
    pub clients: Vec<Client>,
    /// The folder the skills go in, one folder per skill.
    pub dir: PathBuf,
    /// For a Claude Code profile, where its plugins live.
    pub claude: Option<ClaudeHome>,
    /// The `skills` CLI's name for an agent that installs here (`-a`).
    pub cli_agent: Option<&'static str>,
}

/// Each client's global skills folder (from the home folder) and the `skills`
/// CLI's name for it, as the CLI's table of agents and the agents' docs give
/// them. Clients without skills (Claude Desktop, LM Studio, Amazon Q) have
/// none.
fn skills_folder(client: Client) -> Option<(&'static str, &'static str)> {
    Some(match client {
        Client::Codex => (".agents/skills", "codex"),
        Client::Cline => (".agents/skills", "cline"),
        Client::Warp => (".agents/skills", "warp"),
        Client::Zed => (".agents/skills", "zed"),
        Client::Cursor => (".cursor/skills", "cursor"),
        Client::Gemini => (".gemini/skills", "gemini-cli"),
        Client::CopilotCli | Client::VsCode | Client::VsCodeInsiders => {
            (".copilot/skills", "github-copilot")
        }
        Client::OpenCode => (".config/opencode/skills", "opencode"),
        Client::Amp => (".config/agents/skills", "amp"),
        Client::FactoryDroid => (".factory/skills", "droid"),
        Client::Kiro => (".kiro/skills", "kiro-cli"),
        Client::Goose => (".config/goose/skills", "goose"),
        Client::Windsurf => (".codeium/windsurf/skills", "windsurf"),
        Client::Qwen => (".qwen/skills", "qwen-code"),
        Client::Antigravity => (".gemini/antigravity/skills", "antigravity"),
        Client::Junie => (".junie/skills", "junie"),
        Client::Crush => (".config/crush/skills", "crush"),
        Client::KiloCode => (".kilo/skills", "kilo"),
        Client::Continue => (".continue/skills", "continue"),
        Client::Auggie => (".augment/skills", "augment"),
        Client::Trae => (".trae/skills", "trae"),
        Client::RooCode => (".roo/skills", "roo"),
        Client::ClaudeCode | Client::ClaudeDesktop | Client::LmStudio | Client::AmazonQ => {
            return None;
        }
    })
}

/// The skills folders of the agents on this Mac ([`mcp_clients::targets`]).
pub fn targets() -> Vec<Target> {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    targets_for(&home, mcp_clients::targets())
}

/// The skills folders of `clients` under `home`: each Claude Code profile's
/// own, then one per folder the other agents share (Codex, Cline, Warp and
/// Zed all read `~/.agents/skills`). `~/.agents/skills` and Codex's older
/// `~/.codex/skills` are there too when they exist.
pub fn targets_for(home: &Path, clients: Vec<mcp_clients::Target>) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    for t in &clients {
        if t.client != Client::ClaudeCode {
            continue;
        }
        let dir = t
            .profile_dir
            .clone()
            .unwrap_or_else(|| home.join(".claude"));
        let label = if t.profile_dir.is_none() {
            "Claude Code".to_string()
        } else {
            format!("Claude Code · {}", t.profile)
        };
        out.push(Target {
            label,
            clients: vec![Client::ClaudeCode],
            dir: dir.join("skills"),
            claude: Some(ClaudeHome {
                dir,
                profile_dir: t.profile_dir.clone(),
            }),
            cli_agent: Some("claude-code"),
        });
    }
    let mut add = |client: Option<Client>, folder: &str, agent: Option<&'static str>| {
        let dir = home.join(folder);
        match out.iter_mut().find(|t| t.dir == dir) {
            Some(t) => {
                if let Some(client) = client
                    && !t.clients.contains(&client)
                {
                    t.clients.push(client);
                }
                if t.cli_agent.is_none() {
                    t.cli_agent = agent;
                }
            }
            None => out.push(Target {
                label: String::new(),
                clients: client.into_iter().collect(),
                dir,
                claude: None,
                cli_agent: agent,
            }),
        }
    };
    for client in Client::ALL {
        // A client's default profile: the skills CLI has no other.
        let found = clients
            .iter()
            .any(|t| t.client == client && t.profile_dir.is_none());
        if found && let Some((folder, agent)) = skills_folder(client) {
            add(Some(client), folder, Some(agent));
        }
    }
    if home.join(".agents/skills").is_dir() {
        add(None, ".agents/skills", Some("codex"));
    }
    if home.join(".codex/skills").is_dir() {
        add(None, ".codex/skills", None);
    }
    for t in out.iter_mut().filter(|t| t.label.is_empty()) {
        let names: Vec<&str> = t.clients.iter().map(|c| c.name()).collect();
        t.label = if names.is_empty() {
            match t.dir.strip_prefix(home) {
                Ok(rest) if rest == Path::new(".codex/skills") => "Codex (older folder)".into(),
                _ => "Shared agents folder".into(),
            }
        } else {
            names.join(", ")
        };
    }
    out
}

/// The targets `wanted` names, by their place in `targets`: a target's label
/// ("Claude Code · work", "Codex, Zed"), one of its agents ("codex", "zed";
/// "Claude Code" is the default profile's), or its folder (`~/.cursor/skills`),
/// matched without case, spaces or punctuation ("claude-code-work").
pub fn find_targets(targets: &[Target], wanted: &[String]) -> Result<Vec<usize>, String> {
    let key = |s: &str| -> String {
        s.to_lowercase()
            .replace("profile", "")
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    };
    let home = std::env::var("HOME").unwrap_or_default();
    let mut out = Vec::new();
    for name in wanted {
        let path = match name.trim().strip_prefix("~/") {
            Some(rest) => Path::new(&home).join(rest),
            None => PathBuf::from(name.trim()),
        };
        let wanted_key = key(name);
        let found = targets
            .iter()
            .position(|t| key(&t.label) == wanted_key)
            .or_else(|| {
                targets.iter().position(|t| {
                    t.claude.as_ref().is_none_or(|c| c.profile_dir.is_none())
                        && t.clients.iter().any(|c| key(c.name()) == wanted_key)
                })
            })
            .or_else(|| {
                targets
                    .iter()
                    .position(|t| t.dir == path || t.dir == path.join("skills"))
            });
        match found {
            Some(ix) if !out.contains(&ix) => out.push(ix),
            Some(_) => {}
            None => {
                return Err(format!(
                    "No agent \"{name}\" on this Mac. The agents are: {}.",
                    targets
                        .iter()
                        .map(|t| t.label.as_str())
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
        }
    }
    Ok(out)
}
