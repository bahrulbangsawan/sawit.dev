//! SSH servers: Sawit's saved hosts (`ssh_hosts.json`, passwords in the
//! Keychain) and `~/.ssh/config` hosts; probing and running commands.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::ssh::{HostList, SshHost};

use crate::Sawit;

mod exec;
mod hosts;
#[cfg(test)]
mod tests;

pub use exec::askpass;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct AddHostArgs {
    #[schemars(description = "Display name")]
    pub name: String,
    #[schemars(description = "Server address (host name or IP)")]
    pub host: String,
    #[schemars(description = "User name")]
    pub user: Option<String>,
    #[schemars(description = "Port (default 22)")]
    pub port: Option<u16>,
    #[schemars(description = "Private key file (sign-in with this key)")]
    pub identity_file: Option<String>,
    #[schemars(
        description = "Password (sign-in with a password; saved in the macOS Keychain, never in a file)"
    )]
    pub password: Option<String>,
    #[schemars(
        description = "Local port forwards: `8080:localhost:80`, or `5432` for the same port"
    )]
    pub forwards: Option<Vec<String>>,
    #[schemars(description = "ProxyJump: `user@bastion:22`, comma-separated for several hops")]
    pub jump: Option<String>,
    #[schemars(description = "Folder (group) shown in the sidebar")]
    pub group: Option<String>,
    #[schemars(description = "Directory to start in on the server")]
    pub remote_dir: Option<String>,
    #[schemars(description = "Command run after logging in (e.g. `tmux new -A -s main`)")]
    pub startup: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct UpdateHostArgs {
    #[schemars(description = "Saved host id or exact name (see list_ssh_hosts)")]
    pub id: String,
    pub name: Option<String>,
    pub host: Option<String>,
    #[schemars(description = "User name (empty string clears it)")]
    pub user: Option<String>,
    #[schemars(description = "Port (0 resets to 22)")]
    pub port: Option<u16>,
    #[schemars(
        description = "Sign-in: default (agent and default keys), key or password. Moving a host off password deletes its saved Keychain password: pass confirm"
    )]
    pub auth: Option<String>,
    #[schemars(
        description = "Private key file; setting it switches sign-in to key, which deletes a password host's saved Keychain password: pass confirm"
    )]
    pub identity_file: Option<String>,
    #[schemars(
        description = "New password for the Keychain; setting it switches sign-in to password"
    )]
    pub password: Option<String>,
    #[schemars(description = "Replaces the port forwards")]
    pub forwards: Option<Vec<String>>,
    pub jump: Option<String>,
    pub group: Option<String>,
    pub remote_dir: Option<String>,
    pub startup: Option<String>,
    #[schemars(
        description = "The host's id or name again. Needed only when the update moves a password host to default or key sign-in (auth, or identity_file), which deletes its saved Keychain password for good"
    )]
    pub confirm: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveHostArgs {
    #[schemars(description = "Saved host id or exact name (see list_ssh_hosts)")]
    pub id: String,
    #[schemars(
        description = "The host's id or name again, to confirm: the host and its Keychain password are deleted for good"
    )]
    pub confirm: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HostArgs {
    #[schemars(description = "Saved host id or name, a ~/.ssh/config alias, or a plain host name")]
    pub host: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExecArgs {
    #[schemars(description = "Saved host id or name, a ~/.ssh/config alias, or user@host")]
    pub host: String,
    #[schemars(description = "Command to run on the server (non-interactive, no TTY)")]
    pub command: String,
    #[schemars(description = "Seconds before the command is killed (default 30, max 600)")]
    pub timeout_secs: Option<u64>,
}

/// Where a host argument leads.
pub(super) enum Dest {
    Saved(Box<SshHost>),
    Alias(String),
}

/// Saved host by id, else by exact name (unique).
pub(crate) fn find_saved<'a>(list: &'a HostList, key: &str) -> Result<Option<&'a SshHost>, String> {
    if let Some(h) = list.get(key) {
        return Ok(Some(h));
    }
    let named: Vec<&SshHost> = list.hosts.iter().filter(|h| h.name == key).collect();
    match named.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one)),
        many => Err(format!(
            "{} saved hosts are named “{key}”; use the id: {}",
            many.len(),
            many.iter()
                .map(|h| h.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

impl Sawit {
    pub(super) fn ssh_file(&self) -> PathBuf {
        self.paths.root.join("ssh_hosts.json")
    }

    pub(super) fn ssh_hosts(&self) -> Result<HostList, String> {
        HostList::load(&self.ssh_file()).map_err(|e| {
            format!("{e}\nSawit won't save over this file until it is fixed or moved.")
        })
    }

    pub(super) async fn resolve_dest(&self, key: &str) -> Result<Dest, String> {
        let key = key.trim();
        if key.is_empty() {
            return Err("Name a host.".into());
        }
        let list = self.ssh_hosts()?;
        if let Some(h) = find_saved(&list, key)? {
            return Ok(Dest::Saved(Box::new(h.clone())));
        }
        if key.starts_with('-') || key.contains(char::is_whitespace) {
            return Err(format!("“{key}” isn't a host."));
        }
        Ok(Dest::Alias(key.to_string()))
    }
}

#[tool_router(router = ssh_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List SSH servers: those saved in Sawit (id, name, address, sign-in method, forwards) and the hosts in ~/.ssh/config"
    )]
    async fn list_ssh_hosts(&self) -> CallToolResult {
        self.list_ssh_hosts_tool().await
    }

    #[tool(
        description = "Save an SSH server in Sawit. Sign-in: password if given (stored in the Keychain), else the key file if given, else the agent and default keys"
    )]
    async fn add_ssh_host(&self, Parameters(a): Parameters<AddHostArgs>) -> CallToolResult {
        self.add_ssh_host_tool(a).await
    }

    #[tool(
        description = "Change a saved SSH server; only the given fields change. A new password replaces the one saved in the Keychain. DESTRUCTIVE when it moves a password host to default or key sign-in (auth, or identity_file): that deletes its saved Keychain password, which can't be undone, so it needs confirm = the host's id or name"
    )]
    async fn update_ssh_host(&self, Parameters(a): Parameters<UpdateHostArgs>) -> CallToolResult {
        self.update_ssh_host_tool(a).await
    }

    #[tool(
        description = "Remove a saved SSH server and its Keychain password. DESTRUCTIVE, can't be undone: `confirm` must repeat the host's id or name, otherwise nothing happens. ~/.ssh/config is never changed"
    )]
    async fn remove_ssh_host(&self, Parameters(a): Parameters<RemoveHostArgs>) -> CallToolResult {
        self.remove_ssh_host_tool(&a.id, &a.confirm).await
    }

    #[tool(
        description = "Check whether an SSH server answers (TCP connect and SSH banner), without signing in"
    )]
    async fn probe_ssh_host(&self, Parameters(a): Parameters<HostArgs>) -> CallToolResult {
        self.probe_ssh_host_tool(&a.host).await
    }

    #[tool(
        description = "Run a non-interactive command on an SSH server with the system ssh and return exit code, stdout and stderr. Works with keys (agent/~/.ssh), saved passwords, and sessions Sawit already has open"
    )]
    async fn ssh_exec(&self, Parameters(a): Parameters<ExecArgs>) -> CallToolResult {
        self.ssh_exec_tool(a).await
    }
}
