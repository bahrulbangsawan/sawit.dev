//! Installing MCP servers (the bundled `sawit-mcp`, and the catalog in
//! `mcp_catalog`) into AI clients: Claude Code (every
//! profile — the default `~/.claude.json` and each `CLAUDE_CONFIG_DIR`
//! folder such as `~/.claude-work`), Claude Desktop, Codex (and `CODEX_HOME`
//! profiles), Cursor, Windsurf, Gemini CLI, Zed, opencode and VS Code.
//!
//! Claude Code and Codex are changed through their own CLI when it is
//! installed (they rewrite their config files while running); everything
//! else, and those two without a CLI, by editing the config file in place —
//! other settings, their order, the JSON file's indentation and TOML comments
//! are kept (JSON with comments is left alone), and the file is copied next
//! to it before every change. Updating an entry changes only how
//! the server is reached: the keys the user added to it (`autoApprove`,
//! `env`, timeouts, …) stay, through the CLI too. Remote servers are written
//! in each client's own format; clients that only run local programs get
//! the `mcp-remote` bridge.
//!
//! GitHub's server takes Sawit's own GitHub sign-in, and no config ever holds
//! the token: each client is given a command that hands it over as it
//! connects ([`handover`]): Claude Code's `headersHelper`, Codex's
//! `http_headers_helper`, or the bridge started through `sawit github token
//! --exec`; VS Code and Copilot CLI sign in to GitHub themselves.

mod cli;
mod clients;
mod file;
mod install;
mod lock;
mod merge;
mod read;
mod shape;
mod spec;
#[cfg(test)]
mod tests;

pub use cli::{CliCall, cli_add, cli_remove};
pub(crate) use cli::{cli_call, run_cli};
pub(crate) use clients::profiles;
pub use clients::{Category, Client, Target, discover, targets};
pub use file::{backup_path, install_file, snippet, uninstall_file};
pub use install::{install, uninstall};
pub use read::{Entries, Status, read_entries, status, status_in};
pub use shape::{Handover, bridged, entry, handover, needs_bridge};
pub use spec::{Spec, Transport};

/// The server's name in every client's config.
pub const SERVER_NAME: &str = "sawit";

/// The name used before the rename to Sawit (PG Desk). Its entry points at
/// the old app, so installing removes it.
const LEGACY_SERVER_NAME: &str = "pgdesk";
