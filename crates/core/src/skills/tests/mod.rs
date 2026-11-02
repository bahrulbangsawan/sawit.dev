//! The Skills catalog's tests: the fixtures they share here, the tests by what they check.

mod built_in;
mod catalog;
mod commands;
mod shipped;
mod targets;
mod versions;

use std::path::PathBuf;

use super::*;
use crate::mcp_clients::{self, Client};

/// Held by each test that runs a pack's script: they take the pack's lock, which the others' runs
/// would find held.
static PACK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn home() -> PathBuf {
    PathBuf::from("/Users/me")
}

/// The commands' setting on a Mac whose home folder is /Users/me, with Sawit's data folder where
/// it is by default.
fn env() -> Env {
    Env {
        home: home(),
        claude_config_dir_set: false,
        bundled: home().join("Library/Application Support/sawit/bundled-skills"),
    }
}

fn client(client: Client, profile_dir: Option<&str>) -> mcp_clients::Target {
    mcp_clients::Target {
        client,
        profile: profile_dir
            .map(|d| d.trim_start_matches("/Users/me/.claude-").to_string())
            .unwrap_or_else(|| "default".into()),
        config: PathBuf::from("/x"),
        profile_dir: profile_dir.map(PathBuf::from),
    }
}

fn claude(profile: Option<&str>) -> Target {
    let dir = profile.map_or(home().join(".claude"), PathBuf::from);
    Target {
        label: "Claude Code".into(),
        clients: vec![Client::ClaudeCode],
        dir: dir.join("skills"),
        claude: Some(ClaudeHome {
            dir,
            profile_dir: profile.map(PathBuf::from),
        }),
        cli_agent: Some("claude-code"),
    }
}

fn folder(dir: &str, agent: Option<&'static str>) -> Target {
    Target {
        label: dir.into(),
        clients: vec![],
        dir: home().join(dir),
        claude: None,
        cli_agent: agent,
    }
}
