//! The agents that drive the app through `sawit-mcp`, as the app shows them: who each one is
//! ([`AgentIdentity`], sent with every control op), the colour and label the app gives it
//! ([`Roster`]), how long its presence lasts on a surface ([`opacity`]), and what it is doing
//! there, in words ([`canvas_action`]), and the components it builds there ([`work`]).
//!
//! Each `sawit-mcp` process is one agent session: Claude Code, Codex and Cursor each start their
//! own stdio server per session, and a session's subagents share it. So the identity is the
//! process's: the client's name from MCP `initialize`, an id made once per process, and the
//! folder the session works in. An op from a `sawit-mcp` that predates identities carries none,
//! and the app counts it as one anonymous agent ([`AgentIdentity::anonymous`]).

mod actions;
mod presence;
mod roster;
#[cfg(test)]
mod tests;
pub mod work;
#[cfg(test)]
mod work_tests;

use serde::{Deserialize, Serialize};

pub use actions::{
    Action, SLOW_ADD, SLOW_BATCH, browser_action, browser_op_changes_the_page, canvas_action,
    page_name, slow_canvas_op,
};
pub use presence::{FADE_AFTER, FADE_TIME, FADED, GONE_AFTER, glide, opacity, shows_action};
pub use roster::{INK, Known, LIGHT_PAGE, PALETTE, Roster};

/// Who sent a control op: one `sawit-mcp` process. `name` is the client's own name for itself,
/// made readable ([`client_name`]), `label` the project or folder its session works in, when it
/// works in one, and `version` the client's version. The app adds the colour and the label it
/// shows ([`Roster`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The id of the agent an op without an identity counts as.
pub const ANONYMOUS_ID: &str = "agent";

impl AgentIdentity {
    /// The agent an op from an older `sawit-mcp` counts as: they all share it, as nothing tells
    /// them apart.
    pub fn anonymous() -> Self {
        Self {
            id: ANONYMOUS_ID.into(),
            name: "Agent".into(),
            label: None,
            version: None,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.id == ANONYMOUS_ID
    }
}

/// The names AI clients give themselves in MCP `initialize` (`clientInfo.name`), by a part of the
/// name, lowercased, and the name people know each one by. The first that matches wins, so the
/// longer, more particular parts come first.
const CLIENTS: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("claude code", "Claude Code"),
    ("claude-ai", "Claude"),
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
    ("roo", "Roo Code"),
    ("cline", "Cline"),
    ("copilot", "Copilot"),
    ("visual studio code", "VS Code"),
    ("vscode", "VS Code"),
    ("zed", "Zed"),
    ("goose", "Goose"),
    ("continue", "Continue"),
    ("kiro", "Kiro"),
    ("warp", "Warp"),
    ("amp", "Amp"),
    ("droid", "Droid"),
];

/// How long a label may be before it is cut, in characters.
const MAX_NAME: usize = 24;

/// The name to show for an MCP client that calls itself `raw` (`clientInfo.name`, such as
/// `claude-code` or `codex-mcp-client`), with `title` (`clientInfo.title`) when it gives one.
/// A known client gets the name people know it by; another gets its title, else its own name
/// with the `-mcp-client` and `-client` endings off, its dashes as spaces and each word
/// capitalised. Never empty, and never longer than [`MAX_NAME`] characters.
pub fn client_name(raw: &str, title: Option<&str>) -> String {
    let lower = raw.trim().to_lowercase();
    if let Some((_, known)) = CLIENTS.iter().find(|(part, _)| lower.contains(part)) {
        return (*known).to_string();
    }
    let name = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(title) => title.to_string(),
        None => {
            let bare = ["-mcp-client", "_mcp_client", "-client", "_client", "-mcp"]
                .iter()
                .fold(lower.as_str(), |name, end| {
                    name.strip_suffix(end).unwrap_or(name)
                });
            bare.split(['-', '_', ' '])
                .filter(|word| !word.is_empty())
                .map(|word| {
                    let mut letters = word.chars();
                    letters.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(letters).collect()
                    })
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    match name.trim() {
        "" => AgentIdentity::anonymous().name,
        name => name.chars().take(MAX_NAME).collect(),
    }
}

/// The id of a `sawit-mcp` process started as `pid` at `started_nanos` (since the epoch): the
/// pid alone could be another process's once this one has ended.
pub fn session_id(pid: u32, started_nanos: u128) -> String {
    format!("mcp-{pid}-{:08x}", (started_nanos & 0xffff_ffff) as u32)
}
