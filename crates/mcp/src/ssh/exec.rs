//! Reaching an SSH server: probing it without signing in, running one non-interactive command with
//! the system `ssh`, and the password helper `ssh` runs for a host with a saved password.

use std::time::Duration;

use rmcp::model::CallToolResult;
use sawit_core::ssh::{self, Auth, Probe, SshHost};

use super::{Dest, ExecArgs};
use crate::{Sawit, blocking, clip, fail, ok, tri};

const EXEC_TIMEOUT: u64 = 30;
const MAX_EXEC_TIMEOUT: u64 = 600;
/// Bytes of stdout/stderr returned each.
const MAX_STREAM: usize = 40_000;

fn probe_text(target: &str, probe: Probe) -> CallToolResult {
    match probe {
        Probe::Up { latency_ms, banner } => ok(format!(
            "{target} is up: {banner} ({latency_ms} ms to connect)."
        )),
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
    opts.into_iter()
        .flat_map(|o| ["-o".to_string(), o])
        .collect()
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
    pub(super) async fn probe_ssh_host_tool(&self, key: &str) -> CallToolResult {
        match tri!(self.resolve_dest(key).await) {
            Dest::Saved(h) => {
                let label = h.label();
                probe_text(&label, ssh::probe_host(*h).await)
            }
            Dest::Alias(alias) => probe_text(&alias.clone(), ssh::probe_alias(alias).await),
        }
    }

    pub(super) async fn ssh_exec_tool(&self, a: ExecArgs) -> CallToolResult {
        if a.command.trim().is_empty() {
            return fail("The command is empty.");
        }
        let secs = a
            .timeout_secs
            .unwrap_or(EXEC_TIMEOUT)
            .clamp(1, MAX_EXEC_TIMEOUT);
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
                    let helper = tri!(std::env::current_exe().map_err(|e| format!(
                        "Unable to find sawit-mcp for the password helper. {e}"
                    )));
                    // The same askpass environment the app uses; `main.rs`
                    // answers it.
                    for (k, v) in ssh::launch(h, Some(&helper)).env {
                        cmd.env(k, v);
                    }
                }
                cmd.args(exec_options(password))
                    .args(ssh::connection_args(h));
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
            Err(_) => {
                return fail(format!(
                    "Timed out after {secs} s on {label}; the command was stopped."
                ));
            }
        };
        let stdout = clip(
            String::from_utf8_lossy(&out.stdout).into_owned(),
            MAX_STREAM,
        );
        let stderr = clip(
            String::from_utf8_lossy(&out.stderr).into_owned(),
            MAX_STREAM,
        );
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
