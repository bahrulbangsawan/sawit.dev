//! The other agents' permission settings, read from the config files `agent_config` finds:
//! Codex's approval policy and sandbox, Gemini CLI's and Qwen Code's allowed and excluded tools,
//! OpenCode's `permission`. Shown, not changed: their formats differ (TOML, nested maps), and a
//! wrong edit there changes what an agent may do without asking.

use std::path::PathBuf;

use crate::agent_config::{AgentConfig, detect};
use crate::mcp_clients::Client;

/// One agent's permission settings in one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtherAgent {
    pub agent: &'static str,
    pub path: PathBuf,
    /// "approval_policy = on-request", "tools.exclude: run_shell_command".
    pub lines: Vec<String>,
}

/// The other agents' permission settings in their user and project files (`projects` are the
/// open projects), files without any left out.
pub fn other_agents(projects: &[PathBuf]) -> Vec<OtherAgent> {
    read_all(&detect(projects))
}

pub(super) fn read_all(configs: &[AgentConfig]) -> Vec<OtherAgent> {
    configs
        .iter()
        .filter_map(|c| {
            let text = std::fs::read_to_string(&c.path).ok()?;
            let name = c.path.file_name()?.to_str()?;
            let lines = match (c.client, name) {
                (Client::Codex, "config.toml" | "managed_config.toml") => codex(&text),
                (Client::Gemini | Client::Qwen, "settings.json") => gemini(&text),
                (Client::OpenCode, "opencode.json" | "opencode.jsonc") => opencode(&text),
                _ => return None,
            };
            (!lines.is_empty()).then(|| OtherAgent {
                agent: c.client.name(),
                path: c.path.clone(),
                lines,
            })
        })
        .collect()
}

/// Codex: `approval_policy` and `sandbox_mode`, at the top and in each profile.
pub(super) fn codex(text: &str) -> Vec<String> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let keys = ["approval_policy", "sandbox_mode"];
    for key in keys {
        if let Some(v) = doc.get(key).and_then(|v| v.as_str()) {
            out.push(format!("{key} = {v}"));
        }
    }
    if let Some(profiles) = doc.get("profiles").and_then(|p| p.as_table_like()) {
        for (name, profile) in profiles.iter() {
            for key in keys {
                if let Some(v) = profile
                    .as_table_like()
                    .and_then(|t| t.get(key))
                    .and_then(|v| v.as_str())
                {
                    out.push(format!("profiles.{name}.{key} = {v}"));
                }
            }
        }
    }
    out
}

fn strings(v: &serde_json::Value) -> Option<String> {
    let items: Vec<&str> = v.as_array()?.iter().filter_map(|s| s.as_str()).collect();
    (!items.is_empty()).then(|| items.join(", "))
}

/// Gemini CLI and Qwen Code: `tools.allowed` (no confirmation), `tools.exclude`, `tools.core`,
/// and the older top-level `allowedTools`, `excludeTools` and `coreTools`.
pub(super) fn gemini(text: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (path, value) in [
        ("tools.allowed", &v["tools"]["allowed"]),
        ("tools.exclude", &v["tools"]["exclude"]),
        ("tools.core", &v["tools"]["core"]),
        ("allowedTools", &v["allowedTools"]),
        ("excludeTools", &v["excludeTools"]),
        ("coreTools", &v["coreTools"]),
    ] {
        if let Some(list) = strings(value) {
            out.push(format!("{path}: {list}"));
        }
    }
    if let Some(mode) = v["tools"]["autoAccept"].as_bool() {
        out.push(format!("tools.autoAccept: {mode}"));
    }
    out
}

/// OpenCode: `permission`, each tool's `allow`, `ask` or `deny`, patterns included.
pub(super) fn opencode(text: &str) -> Vec<String> {
    // opencode.jsonc may have comments: drop whole-line ones before reading.
    let clean: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&clean) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let Some(map) = v["permission"].as_object() else {
        return out;
    };
    for (tool, setting) in map {
        match setting {
            serde_json::Value::String(s) => out.push(format!("{tool}: {s}")),
            serde_json::Value::Object(patterns) => {
                for (pattern, s) in patterns {
                    out.push(format!("{tool} {pattern}: {}", s.as_str().unwrap_or("?")));
                }
            }
            _ => {}
        }
    }
    out
}
