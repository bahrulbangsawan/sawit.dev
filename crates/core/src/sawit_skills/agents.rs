//! Each agent's own instruction file, the one it reads for every project, as its docs give it:
//! where the block goes for the agents found on this Mac, and why an agent gets none. Two agents
//! that read the same file share it (Gemini CLI and Antigravity read `~/.gemini/GEMINI.md`).

use std::path::{Path, PathBuf};

use crate::mcp_clients::{self, Client};

/// One agent's instruction file for every project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentFile {
    /// "Claude Code", "Claude Code · work", "Gemini CLI, Antigravity".
    pub label: String,
    pub path: PathBuf,
    /// Other files of the same agent a removal takes the block out of too: Codex's AGENTS.md
    /// while its AGENTS.override.md is the one it reads.
    pub also: Vec<PathBuf>,
}

/// An agent found without a file Sawit writes the block into, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub label: String,
    pub why: &'static str,
}

/// Where a client's instruction file for every project is, from the home folder.
enum Place {
    /// This file.
    File(&'static str),
    /// The first of these that exists; none, and the client is skipped with this reason.
    Existing(&'static [&'static str], &'static str),
    /// Its own file when it exists; else Claude Code's `~/.claude/CLAUDE.md`, which it reads
    /// while its own isn't there (opencode, Kilo Code): making its own would hide Claude Code's.
    OwnOrClaudes(&'static str),
    /// No file-based instruction file: why.
    None(&'static str),
}

/// Each client's file, as its docs give it. Claude Code and Codex have profiles: see
/// [`agent_files`].
fn place(client: Client) -> Place {
    match client {
        Client::ClaudeCode | Client::Codex => Place::None("has profiles"),
        Client::Gemini | Client::Antigravity => Place::File(".gemini/GEMINI.md"),
        Client::Qwen => Place::File(".qwen/QWEN.md"),
        Client::OpenCode => Place::OwnOrClaudes(".config/opencode/AGENTS.md"),
        Client::KiloCode => Place::OwnOrClaudes(".config/kilo/AGENTS.md"),
        Client::Amp => Place::File(".config/amp/AGENTS.md"),
        Client::CopilotCli => Place::File(".copilot/copilot-instructions.md"),
        Client::FactoryDroid => Place::File(".factory/AGENTS.md"),
        Client::Goose => Place::File(".config/goose/.goosehints"),
        Client::Zed => Place::File(".config/zed/AGENTS.md"),
        Client::Junie => Place::File(".junie/AGENTS.md"),
        Client::Crush => Place::File(".config/crush/CRUSH.md"),
        Client::Cline => Place::File(".agents/AGENTS.md"),
        Client::Kiro => Place::File(".kiro/steering/AGENTS.md"),
        Client::Windsurf => Place::Existing(
            &[
                ".devin/global_rules.md",
                ".codeium/windsurf/memories/global_rules.md",
            ],
            "its global rules file moved with the rename to Devin and isn't there yet: create ~/.devin/global_rules.md for Sawit to add to it",
        ),
        Client::Cursor => Place::None("its User Rules live in Cursor's settings, not in a file"),
        Client::ClaudeDesktop => {
            Place::None("it has no instruction file; it gets Sawit's MCP server")
        }
        Client::VsCode | Client::VsCodeInsiders => Place::None(
            "its instructions are separate .instructions.md files in a profile's prompts folder, not one file",
        ),
        Client::RooCode => {
            Place::None("its global rules are a folder of files (~/.roo/rules), not one file")
        }
        Client::Auggie => {
            Place::None("its global rules are a folder of files (~/.augment/rules), not one file")
        }
        Client::Trae => Place::None("its user rules are managed in Trae's settings"),
        Client::Continue => Place::None(
            "its global rules live in ~/.continue/config.yaml, which Sawit doesn't edit",
        ),
        Client::Warp => Place::None("its rules live in Warp Drive, not in a file"),
        Client::AmazonQ => {
            Place::None("it has no global instruction file (its context is set per agent)")
        }
        Client::LmStudio => Place::None("it has no instruction file"),
    }
}

/// The instruction files of the agents in `clients` (from [`mcp_clients::targets`]) under `home`,
/// one per file (agents that share one are named together), and the agents that get none.
pub fn agent_files(home: &Path, clients: &[mcp_clients::Target]) -> (Vec<AgentFile>, Vec<Skipped>) {
    let mut files: Vec<AgentFile> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    // A profile's CLAUDE.md is often a link to the default profile's: one file, named once.
    let same = |a: &Path, b: &Path| {
        a == b
            || std::fs::canonicalize(a)
                .is_ok_and(|a| std::fs::canonicalize(b).is_ok_and(|b| a == b))
    };
    let mut add = |label: String, path: PathBuf, also: Vec<PathBuf>| match files
        .iter_mut()
        .find(|f| same(&f.path, &path))
    {
        Some(f) => {
            if !f.label.split(", ").any(|l| l == label) {
                f.label = format!("{}, {label}", f.label);
            }
            for a in also {
                if !f.also.contains(&a) {
                    f.also.push(a);
                }
            }
        }
        None => files.push(AgentFile { label, path, also }),
    };
    let claude_md = home.join(".claude/CLAUDE.md");
    let claude_default = clients
        .iter()
        .any(|t| t.client == Client::ClaudeCode && t.profile_dir.is_none());
    for t in clients {
        // A client's older config is the same agent, with the same instruction file.
        let label = match t.client {
            Client::ClaudeCode | Client::Codex => label_of(t),
            client => client.name().to_string(),
        };
        match t.client {
            Client::ClaudeCode => {
                let dir = t
                    .profile_dir
                    .clone()
                    .unwrap_or_else(|| home.join(".claude"));
                add(label, dir.join("CLAUDE.md"), Vec::new());
            }
            Client::Codex => {
                let dir = t.profile_dir.clone().unwrap_or_else(|| home.join(".codex"));
                let (own, over) = (dir.join("AGENTS.md"), dir.join("AGENTS.override.md"));
                // Codex reads the first of these that isn't empty.
                if std::fs::metadata(&over).is_ok_and(|m| m.len() > 0) {
                    add(label, over, vec![own]);
                } else {
                    add(label, own, Vec::new());
                }
            }
            client => match place(client) {
                Place::File(rel) => add(label, home.join(rel), Vec::new()),
                Place::Existing(rels, why) => {
                    match rels.iter().map(|r| home.join(r)).find(|p| p.is_file()) {
                        Some(path) => add(label, path, Vec::new()),
                        None => skipped.push(Skipped { label, why }),
                    }
                }
                Place::OwnOrClaudes(rel) => {
                    let own = home.join(rel);
                    if own.exists() || !(claude_default || claude_md.is_file()) {
                        add(label, own, Vec::new());
                    } else {
                        add(label, claude_md.clone(), vec![own]);
                    }
                }
                Place::None(why) => skipped.push(Skipped { label, why }),
            },
        }
    }
    let mut seen = Vec::new();
    skipped.retain(|s| {
        let first = !seen.contains(&s.label);
        seen.push(s.label.clone());
        first
    });
    (files, skipped)
}

/// A client as the list names it: with its profile when it isn't the default ("Claude Code ·
/// work"), and "(older)" for a legacy config's.
pub fn label_of(t: &mcp_clients::Target) -> String {
    match t.profile.as_str() {
        "default" => t.client.name().to_string(),
        "legacy" => format!("{} (older)", t.client.name()),
        profile => format!("{} · {profile}", t.client.name()),
    }
}
