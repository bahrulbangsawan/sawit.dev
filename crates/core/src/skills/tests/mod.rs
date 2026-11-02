//! The Skills catalog's tests: the fixtures they share here, the tests by what they check.

mod catalog;
mod commands;
mod targets;

use std::path::PathBuf;

use super::*;
use crate::mcp_clients::{self, Client};

fn home() -> PathBuf {
    PathBuf::from("/Users/me")
}

fn env() -> Env {
    Env {
        home: home(),
        claude_config_dir_set: false,
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
