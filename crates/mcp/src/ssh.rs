//! SSH servers: Sawit's saved hosts (`ssh_hosts.json`, passwords in the
//! Keychain) and `~/.ssh/config` hosts; probing and running commands.

use std::path::PathBuf;
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Store;
use sawit_core::ssh::{self, Auth, ConfigHost, HostList, Probe, SshHost};

use crate::{Sawit, blocking, clip, fail, ok, tri};

const EXEC_TIMEOUT: u64 = 30;
const MAX_EXEC_TIMEOUT: u64 = 600;
/// Bytes of stdout/stderr returned each.
const MAX_STREAM: usize = 40_000;

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
    #[schemars(description = "Local port forwards: `8080:localhost:80`, or `5432` for the same port")]
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
    #[schemars(description = "Sign-in: default (agent and default keys), key or password")]
    pub auth: Option<String>,
    #[schemars(description = "Private key file; setting it switches sign-in to key")]
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
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HostIdArgs {
    #[schemars(description = "Saved host id or exact name (see list_ssh_hosts)")]
    pub id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct HostArgs {
    #[schemars(
        description = "Saved host id or name, a ~/.ssh/config alias, or a plain host name"
    )]
    pub host: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ExecArgs {
    #[schemars(
        description = "Saved host id or name, a ~/.ssh/config alias, or user@host"
    )]
    pub host: String,
    #[schemars(description = "Command to run on the server (non-interactive, no TTY)")]
    pub command: String,
    #[schemars(description = "Seconds before the command is killed (default 30, max 600)")]
    pub timeout_secs: Option<u64>,
}

/// Where a host argument leads.
enum Dest {
    Saved(Box<SshHost>),
    Alias(String),
}

fn auth_name(auth: Auth) -> &'static str {
    match auth {
        Auth::Default => "default keys",
        Auth::Key => "key",
        Auth::Password => "password",
    }
}

pub(crate) fn parse_auth(text: &str) -> Result<Auth, String> {
    match text.trim().to_lowercase().as_str() {
        "default" | "agent" => Ok(Auth::Default),
        "key" => Ok(Auth::Key),
        "password" => Ok(Auth::Password),
        other => Err(format!("auth “{other}” must be default, key or password.")),
    }
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
            many.iter().map(|h| h.id.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn saved_line(h: &SshHost, has_password: bool) -> String {
    let mut parts = vec![
        format!("- {} · {}", h.label(), h.address()),
        format!("id {}", h.id),
        format!(
            "auth {}{}",
            auth_name(h.auth),
            if h.auth == Auth::Password && !has_password { " (none saved)" } else { "" }
        ),
    ];
    if h.auth == Auth::Key {
        parts.push(format!("key {}", h.identity_file));
    }
    if !h.jump.trim().is_empty() {
        parts.push(format!("via {}", h.jump.trim()));
    }
    if !h.forwards.is_empty() {
        parts.push(format!("forwards {}", h.forwards.join(", ")));
    }
    if !h.group.trim().is_empty() {
        parts.push(format!("group {}", h.group.trim()));
    }
    parts.join(" · ")
}

fn config_line(c: &ConfigHost) -> String {
    let mut line = format!("- {} · {}", c.alias, c.address());
    if let Some(j) = &c.jump {
        line.push_str(&format!(" · via {j}"));
    }
    line
}

fn probe_text(target: &str, probe: Probe) -> CallToolResult {
    match probe {
        Probe::Up { latency_ms, banner } => {
            ok(format!("{target} is up: {banner} ({latency_ms} ms to connect)."))
        }
        Probe::Indirect => ok(format!(
            "{target} is reached through a jump host or proxy command; it wasn't checked directly."
        )),
        Probe::Down(why) => fail(format!("{target} is not reachable. {why}")),
    }
}

/// Options for one non-interactive command: reuse a session the app has
/// open (without starting a lingering master), never ask in a terminal.
pub(crate) fn exec_options(password: bool) -> Vec<String> {
    let mut opts = vec![
        "ControlMaster=no".to_string(),
        "ControlPath=~/.ssh/sawit-%C".into(),
        "ConnectTimeout=10".into(),
        "ServerAliveInterval=15".into(),
        "ServerAliveCountMax=2".into(),
    ];
    if password {
        opts.push("NumberOfPasswordPrompts=1".into());
    } else {
        opts.push("BatchMode=yes".into());
    }
    opts.into_iter().flat_map(|o| ["-o".to_string(), o]).collect()
}

/// `SSH_ASKPASS` entry point for `sawit-mcp` (see `main.rs`): answers the
/// first password prompt with the host's saved password and nothing else.
/// Unlike the app's helper it never asks in a terminal: there is no one to
/// type for an agent.
pub fn askpass(account: &str, prompt: &str) -> i32 {
    if !ssh::is_password_prompt(prompt) {
        return 1;
    }
    let mut host = SshHost::new("", "");
    host.id = account.to_string();
    match ssh::load_password(&host) {
        Ok(Some(password)) => {
            println!("{password}");
            0
        }
        _ => 1,
    }
}

impl Sawit {
    fn ssh_file(&self) -> PathBuf {
        self.paths.root.join("ssh_hosts.json")
    }

    fn ssh_hosts(&self) -> Result<HostList, String> {
        HostList::load(&self.ssh_file()).map_err(|e| {
            format!("{e}\nSawit won't save over this file until it is fixed or moved.")
        })
    }

    async fn resolve_dest(&self, key: &str) -> Result<Dest, String> {
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

    async fn list_ssh_hosts_tool(&self) -> CallToolResult {
        let list = tri!(self.ssh_hosts());
        let hosts = list.search("");
        let saved: Vec<SshHost> = hosts.iter().map(|h| (*h).clone()).collect();
        let (lines, config) = tri!(
            blocking(move || {
                let lines: Vec<String> = saved
                    .iter()
                    .map(|h| {
                        let has = h.auth == Auth::Password && ssh::has_password(h);
                        saved_line(h, has)
                    })
                    .collect();
                (lines, ssh::load_config_hosts())
            })
            .await
        );
        let mut out = Vec::new();
        out.push(if lines.is_empty() {
            "Saved in Sawit: none".to_string()
        } else {
            format!("Saved in Sawit ({}):", lines.len())
        });
        out.extend(lines);
        out.push(String::new());
        out.push(if config.is_empty() {
            "~/.ssh/config: no hosts".to_string()
        } else {
            format!("~/.ssh/config ({}; use the alias as `host`):", config.len())
        });
        out.extend(config.iter().map(config_line));
        ok(out.join("\n"))
    }

    async fn add_ssh_host_tool(&self, a: AddHostArgs) -> CallToolResult {
        let mut host = SshHost::new(&a.name, &a.host);
        host.user = a.user.unwrap_or_default().trim().to_string();
        host.port = a.port.filter(|p| *p != 0 && *p != 22);
        if let Some(key) = a.identity_file.filter(|k| !k.trim().is_empty()) {
            host.auth = Auth::Key;
            host.identity_file = key.trim().to_string();
        }
        let password = a.password.filter(|p| !p.is_empty());
        if password.is_some() {
            host.auth = Auth::Password;
        }
        host.forwards = a.forwards.unwrap_or_default();
        host.jump = a.jump.unwrap_or_default();
        host.group = a.group.unwrap_or_default();
        host.remote_dir = a.remote_dir.unwrap_or_default();
        host.startup = a.startup.unwrap_or_default();
        if let Some(problem) = host.problem() {
            return fail(problem);
        }
        let mut list = tri!(self.ssh_hosts());
        let id = list.add(host.clone(), ssh::now());
        if let Some(pw) = password {
            let h = host.clone();
            tri!(blocking(move || ssh::store_password(&h, &pw))
                .await
                .and_then(|r| r.map_err(|e| format!("Unable to save the password. {e}"))));
        }
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        self.notify(Store::Ssh).await;
        ok(format!("Saved {} ({}) with id {id}.", host.label(), host.address()))
    }

    async fn update_ssh_host_tool(&self, a: UpdateHostArgs) -> CallToolResult {
        let mut list = tri!(self.ssh_hosts());
        let Some(current) = tri!(find_saved(&list, &a.id)).cloned() else {
            return fail(format!("No saved host “{}”. See list_ssh_hosts.", a.id));
        };
        let mut host = current.clone();
        if let Some(v) = a.name {
            host.name = v.trim().to_string();
        }
        if let Some(v) = a.host {
            host.host = v.trim().to_string();
        }
        if let Some(v) = a.user {
            host.user = v.trim().to_string();
        }
        if let Some(p) = a.port {
            host.port = Some(p).filter(|p| *p != 0 && *p != 22);
        }
        if let Some(auth) = a.auth {
            host.auth = tri!(parse_auth(&auth));
        }
        if let Some(key) = a.identity_file {
            host.identity_file = key.trim().to_string();
            if !host.identity_file.is_empty() {
                host.auth = Auth::Key;
            }
        }
        let password = a.password.filter(|p| !p.is_empty());
        if password.is_some() {
            host.auth = Auth::Password;
        }
        if let Some(v) = a.forwards {
            host.forwards = v;
        }
        if let Some(v) = a.jump {
            host.jump = v;
        }
        if let Some(v) = a.group {
            host.group = v;
        }
        if let Some(v) = a.remote_dir {
            host.remote_dir = v;
        }
        if let Some(v) = a.startup {
            host.startup = v;
        }
        if let Some(problem) = host.problem() {
            return fail(problem);
        }
        let edited = host.clone();
        list.update(&current.id, |h| *h = edited);
        if let Some(pw) = password {
            let h = host.clone();
            tri!(blocking(move || ssh::store_password(&h, &pw))
                .await
                .and_then(|r| r.map_err(|e| format!("Unable to save the password. {e}"))));
        } else if current.auth == Auth::Password && host.auth != Auth::Password {
            let h = host.clone();
            let _ = blocking(move || ssh::delete_password(&h)).await;
        }
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        self.notify(Store::Ssh).await;
        ok(format!("Updated {}.\n{}", host.label(), saved_line(&host, true)))
    }

    async fn remove_ssh_host_tool(&self, key: &str) -> CallToolResult {
        let mut list = tri!(self.ssh_hosts());
        let Some(host) = tri!(find_saved(&list, key)).cloned() else {
            return fail(format!("No saved host “{key}”. See list_ssh_hosts."));
        };
        list.remove(&host.id);
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        let h = host.clone();
        let _ = blocking(move || ssh::delete_password(&h)).await;
        self.notify(Store::Ssh).await;
        ok(format!("Removed {} ({}).", host.label(), host.address()))
    }

    async fn probe_ssh_host_tool(&self, key: &str) -> CallToolResult {
        match tri!(self.resolve_dest(key).await) {
            Dest::Saved(h) => {
                let label = h.label();
                probe_text(&label, ssh::probe_host(*h).await)
            }
            Dest::Alias(alias) => probe_text(&alias.clone(), ssh::probe_alias(alias).await),
        }
    }

    async fn ssh_exec_tool(&self, a: ExecArgs) -> CallToolResult {
        if a.command.trim().is_empty() {
            return fail("The command is empty.");
        }
        let secs = a.timeout_secs.unwrap_or(EXEC_TIMEOUT).clamp(1, MAX_EXEC_TIMEOUT);
        let dest = tri!(self.resolve_dest(&a.host).await);
        let mut cmd = tokio::process::Command::new("ssh");
        let label = match &dest {
            Dest::Saved(h) => {
                if let Some(problem) = h.problem() {
                    return fail(problem);
                }
                let password = h.auth == Auth::Password;
                if password {
                    let probe = h.clone();
                    let saved = tri!(blocking(move || ssh::load_password(&probe)).await);
                    if !matches!(saved, Ok(Some(_))) {
                        return fail(format!(
                            "{} signs in with a password, but none is saved in the Keychain. Save one with update_ssh_host (password), or switch it to key sign-in.",
                            h.label()
                        ));
                    }
                    let helper = tri!(std::env::current_exe()
                        .map_err(|e| format!("Unable to find sawit-mcp for the password helper. {e}")));
                    // The same askpass environment the app uses; `main.rs`
                    // answers it.
                    for (k, v) in ssh::launch(h, Some(&helper)).env {
                        cmd.env(k, v);
                    }
                }
                cmd.args(exec_options(password)).args(ssh::connection_args(h));
                h.label()
            }
            Dest::Alias(alias) => {
                cmd.args(exec_options(false)).arg("--").arg(alias);
                alias.clone()
            }
        };
        cmd.arg(&a.command)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let out = match tokio::time::timeout(Duration::from_secs(secs), cmd.output()).await {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return fail(format!("Unable to run ssh. {e}")),
            Err(_) => return fail(format!("Timed out after {secs} s on {label}; the command was stopped.")),
        };
        let stdout = clip(String::from_utf8_lossy(&out.stdout).into_owned(), MAX_STREAM);
        let stderr = clip(String::from_utf8_lossy(&out.stderr).into_owned(), MAX_STREAM);
        let code = out.status.code();
        let mut text = format!(
            "{label}: exit {}",
            code.map_or_else(|| "signal".to_string(), |c| c.to_string())
        );
        if !stdout.trim().is_empty() {
            text.push_str(&format!("\n--- stdout ---\n{}", stdout.trim_end()));
        }
        if !stderr.trim().is_empty() {
            text.push_str(&format!("\n--- stderr ---\n{}", stderr.trim_end()));
        }
        // 255 is ssh's own failure (connection, sign-in).
        if code == Some(255) {
            text.push_str("\n\nssh couldn't connect or sign in. ssh_exec needs a key in ssh-agent / ~/.ssh, a saved password, or a host key already accepted (connect once in Sawit or a terminal).");
        }
        if out.status.success() {
            ok(text)
        } else {
            fail(text)
        }
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

    #[tool(description = "Change a saved SSH server; only the given fields change")]
    async fn update_ssh_host(&self, Parameters(a): Parameters<UpdateHostArgs>) -> CallToolResult {
        self.update_ssh_host_tool(a).await
    }

    #[tool(description = "Remove a saved SSH server (and its Keychain password)")]
    async fn remove_ssh_host(&self, Parameters(a): Parameters<HostIdArgs>) -> CallToolResult {
        self.remove_ssh_host_tool(&a.id).await
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_hosts_by_id_or_unique_name() {
        let mut list = HostList::default();
        let a = list.add(SshHost::new("web", "1.2.3.4"), 1);
        list.add(SshHost::new("db", "1.2.3.5"), 1);
        list.add(SshHost::new("db", "1.2.3.6"), 1);
        assert_eq!(find_saved(&list, &a).unwrap().unwrap().name, "web");
        assert_eq!(find_saved(&list, "web").unwrap().unwrap().host, "1.2.3.4");
        assert!(find_saved(&list, "nope").unwrap().is_none());
        assert!(find_saved(&list, "db").is_err());
    }

    #[test]
    fn exec_never_prompts_for_key_hosts() {
        let opts = exec_options(false);
        assert!(opts.contains(&"BatchMode=yes".to_string()));
        assert!(opts.contains(&"ControlMaster=no".to_string()));
        let opts = exec_options(true);
        assert!(!opts.contains(&"BatchMode=yes".to_string()));
        assert!(opts.contains(&"NumberOfPasswordPrompts=1".to_string()));
    }

    #[test]
    fn auth_names() {
        assert_eq!(parse_auth("Password").unwrap(), Auth::Password);
        assert_eq!(parse_auth("key").unwrap(), Auth::Key);
        assert!(parse_auth("otp").is_err());
    }

    #[test]
    fn askpass_ignores_other_prompts() {
        assert_eq!(askpass("nope", "Enter passphrase for key:"), 1);
        assert_eq!(askpass("nope", "Are you sure (yes/no)?"), 1);
    }
}
