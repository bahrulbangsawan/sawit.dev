//! Agent Accounts: several signed-in accounts of one coding agent, each its own config folder,
//! switched by hand. Claude Code is the first agent: an account is a `CLAUDE_CONFIG_DIR` folder,
//! and the default one (`~/.claude`) runs with the variable unset. Codex (`CODEX_HOME`) and others
//! fit the same model later.
//!
//! Sawit never reads, copies or moves a sign-in: each agent keeps its own (Claude Code in the
//! Keychain, per folder), and signing in is the agent's own command. What Sawit keeps here is the
//! list of accounts, which one is active, and usage the agent itself reported.

pub mod advice;
pub mod analytics;
pub mod claude;
pub mod cli;
pub mod collector;
pub mod import;
pub mod login;
pub mod run;
pub mod sessions;
pub mod shared;
pub mod shell;
pub mod statusline;
mod store;
pub mod tracking;
pub mod usage;

pub use store::{Accounts, Settings, active_path, read_active, write_active};

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The agents Sawit keeps several accounts of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    #[default]
    ClaudeCode,
}

impl AgentKind {
    pub const ALL: &[AgentKind] = &[AgentKind::ClaudeCode];

    pub fn label(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
        }
    }

    /// The variable that picks the account's folder.
    pub fn profile_var(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "CLAUDE_CONFIG_DIR",
        }
    }

    /// The agent's own command.
    pub fn program(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude",
        }
    }

    /// The default account's folder under `home`.
    pub fn default_dir(self, home: &Path) -> PathBuf {
        match self {
            AgentKind::ClaudeCode => home.join(".claude"),
        }
    }

    /// The prefix of other accounts' folders in `home` (`.claude-work`).
    pub fn dir_prefix(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => ".claude",
        }
    }

    pub fn slug(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude_code",
        }
    }
}

/// One account: a config folder with its own sign-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// A UUID; Claude Pool's own when imported, so its usage files keep their names.
    pub id: String,
    #[serde(default)]
    pub kind: AgentKind,
    /// Kebab-case, unique per agent ([`kebab`]).
    pub name: String,
    pub dir: PathBuf,
    /// Runs with the agent's variable unset, never set to the default folder.
    #[serde(default)]
    pub is_default: bool,
    /// `#RRGGBB`.
    #[serde(default = "default_color")]
    pub color: String,
    /// The user's own order.
    #[serde(default)]
    pub order: usize,
    /// The account's `statusLine` reports usage to Sawit.
    #[serde(default)]
    pub tracking: bool,
    /// The `statusLine` value the account had before tracking, verbatim, to give back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_status_line: Option<serde_json::Value>,
    /// Unix seconds.
    #[serde(default)]
    pub created_at: i64,
}

fn default_color() -> String {
    "#D97757".into()
}

impl Account {
    /// What the agent's variable is set to: the folder, or nothing (unset) for the default.
    pub fn env_value(&self) -> Option<&Path> {
        (!self.is_default).then_some(self.dir.as_path())
    }
}

/// The longest name.
pub const MAX_NAME: usize = 32;

/// Kebab-case: lowercase a–z and digits, every run of anything else one hyphen, none at either
/// end, at most [`MAX_NAME`] characters. "Client — Northwind" → "client-northwind".
pub fn kebab(text: &str) -> String {
    let mut out = String::new();
    let mut hyphen = false;
    for c in text.to_lowercase().chars() {
        let c = fold(c);
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
            hyphen = false;
        } else if !hyphen && !out.is_empty() {
            out.push('-');
            hyphen = true;
        }
    }
    let mut out: String = out.chars().take(MAX_NAME).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The common accented letters as their plain one.
fn fold(c: char) -> char {
    match c {
        'à'..='å' => 'a',
        'ç' => 'c',
        'è'..='ë' => 'e',
        'ì'..='ï' => 'i',
        'ñ' => 'n',
        'ò'..='ö' | 'ø' => 'o',
        'ù'..='ü' => 'u',
        'ý' | 'ÿ' => 'y',
        c => c,
    }
}

/// A new random id, shaped like a UUID (v4).
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read as _;
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02X}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

/// The account folders of `kind` under `home` that exist: the default one, then
/// `<prefix>-<name>` folders, by name. For Adopt Existing.
pub fn discover(kind: AgentKind, home: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let default = kind.default_dir(home);
    if default.is_dir() {
        found.push(default);
    }
    let prefix = format!("{}-", kind.dir_prefix());
    let mut others: Vec<PathBuf> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.starts_with(&prefix)
                // Claude Pool's own folder isn't an account.
                && name != ".claude-pool"
                && e.path().is_dir()
        })
        .map(|e| e.path())
        .collect();
    others.sort();
    found.extend(others);
    found
}

#[cfg(test)]
mod tests;
