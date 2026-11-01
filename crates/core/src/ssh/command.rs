//! The `ssh` command line for a server: Sawit's shared-connection options, the connection, its
//! port forwards and the command run on the server.

use std::path::Path;

use super::{ASKPASS_ENV, Auth, SSH, SshHost, parse_forward};
use crate::terminal::shell_quote;

/// Where connections are shared between sessions. `%C` is a hash of the
/// destination; kept short because socket paths max out at 104 bytes.
fn control_path() -> String {
    "~/.ssh/sawit-%C".into()
}

/// Options every Sawit session uses: share one connection per server, notice
/// a dead network within a minute and a dead address within 10 seconds. The
/// shared connection outlives the last session by 10 minutes, so the next
/// one opens at once; for a server with port forwards only by seconds, since
/// the connection holds the forwarded ports while it lasts.
fn common_options(forwards: bool) -> Vec<String> {
    let persist = if forwards { "5s" } else { "10m" };
    [
        "ControlMaster=auto".to_string(),
        format!("ControlPath={}", control_path()),
        format!("ControlPersist={persist}"),
        "ServerAliveInterval=20".into(),
        "ServerAliveCountMax=3".into(),
        "ConnectTimeout=10".into(),
    ]
    .into_iter()
    .flat_map(|o| ["-o".to_string(), o])
    .collect()
}

/// `-p`, `-l`, `-i`, `-J` and the destination: enough to reach the server.
pub fn connection_args(host: &SshHost) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(port) = host.port {
        args.extend(["-p".to_string(), port.to_string()]);
    }
    if !host.user.trim().is_empty() {
        args.extend(["-l".to_string(), host.user.trim().to_string()]);
    }
    match host.auth {
        Auth::Key if !host.identity_file.trim().is_empty() => {
            args.extend([
                "-i".to_string(),
                expand_home(host.identity_file.trim()),
                "-o".into(),
                "IdentitiesOnly=yes".into(),
            ]);
        }
        Auth::Password => {
            args.extend([
                "-o".to_string(),
                "PreferredAuthentications=password,keyboard-interactive".into(),
            ]);
        }
        _ => {}
    }
    if !host.jump.trim().is_empty() {
        args.extend(["-J".to_string(), host.jump.trim().to_string()]);
    }
    args.push(host.host.trim().to_string());
    args
}

fn expand_home(path: &str) -> String {
    match (path.strip_prefix("~/"), std::env::var("HOME")) {
        (Some(rest), Ok(home)) => format!("{home}/{rest}"),
        _ => path.to_string(),
    }
}

/// The command run on the server: go to `remote_dir`, run `startup`, then
/// an interactive login shell. `None` for a plain login.
fn remote_command(host: &SshHost) -> Option<String> {
    let dir = host.remote_dir.trim();
    let startup = host.startup.trim();
    if dir.is_empty() && startup.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    if !dir.is_empty() {
        // Keep a leading `~` unquoted so the remote shell expands it.
        let quoted = match dir.strip_prefix('~') {
            Some("") => "~".to_string(),
            Some(rest) if rest.starts_with('/') => {
                format!("~/{}", shell_quote(Path::new(&rest[1..])))
            }
            _ => shell_quote(Path::new(dir)),
        };
        parts.push(format!("cd {quoted}"));
    }
    if !startup.is_empty() {
        parts.push(startup.to_string());
    }
    parts.push("exec \"$SHELL\" -l".into());
    Some(parts.join("; "))
}

/// A ready-to-spawn `ssh` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    /// Extra environment (the askpass helper, when a password is saved).
    pub env: Vec<(String, String)>,
}

impl Launch {
    /// The command as it would be typed in a shell (no askpass environment).
    pub fn command_line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .map(|a| shell_quote(Path::new(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// `ssh` for a saved host. With `askpass` (the path of a helper that answers
/// with the saved password, see [`askpass_main`](super::askpass_main)) the password is filled in.
pub fn launch(host: &SshHost, askpass: Option<&Path>) -> Launch {
    let forwards = host.forwards.iter().any(|f| parse_forward(f).is_some());
    let mut args = common_options(forwards);
    args.extend(host_args(host));
    let env = match (host.auth, askpass) {
        (Auth::Password, Some(helper)) => vec![
            ("SSH_ASKPASS".into(), helper.to_string_lossy().into_owned()),
            ("SSH_ASKPASS_REQUIRE".into(), "force".into()),
            (ASKPASS_ENV.into(), host.keychain_account().to_string()),
        ],
        _ => Vec::new(),
    };
    Launch {
        program: SSH.into(),
        args,
        env,
    }
}

/// Forwards, the connection and the remote command: `ssh` without Sawit's
/// own options.
fn host_args(host: &SshHost) -> Vec<String> {
    let mut args = Vec::new();
    for f in host.forwards.iter().filter_map(|f| parse_forward(f)) {
        args.extend(["-L".to_string(), f]);
    }
    let command = remote_command(host);
    if command.is_some() {
        args.push("-t".into());
    }
    args.extend(connection_args(host));
    args.extend(command);
    args
}

/// The command to type in any terminal to reach `host`.
pub fn plain_command(host: &SshHost) -> String {
    Launch {
        program: SSH.into(),
        args: host_args(host),
        env: Vec::new(),
    }
    .command_line()
}

/// `ssh <alias>`: everything else comes from the user's config.
pub fn launch_alias(alias: &str) -> Launch {
    let mut args = common_options(false);
    args.push("--".into());
    args.push(alias.to_string());
    Launch {
        program: SSH.into(),
        args,
        env: Vec::new(),
    }
}

/// `~/.ssh` must exist for the shared-connection socket.
pub fn ensure_ssh_dir() {
    if let Ok(home) = std::env::var("HOME") {
        let dir = Path::new(&home).join(".ssh");
        if !dir.exists() && std::fs::create_dir_all(&dir).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
            }
        }
    }
}
