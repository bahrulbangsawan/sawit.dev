//! A saved server: its settings, how it is named, and what would stop a connection to it.

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Auth {
    /// The agent and the default keys (`~/.ssh/id_*`), as plain `ssh` does.
    #[default]
    Default,
    /// One private key file.
    Key,
    /// A password, saved in the Keychain or typed when asked.
    Password,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshHost {
    pub id: String,
    pub name: String,
    pub host: String,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub identity_file: String,
    /// `ProxyJump`: `user@bastion:22`, comma-separated for several hops.
    #[serde(default)]
    pub jump: String,
    /// Folder shown in the sidebar; empty for none.
    #[serde(default)]
    pub group: String,
    /// Directory to start in on the server.
    #[serde(default)]
    pub remote_dir: String,
    /// Run after logging in (`tmux new -A -s main`); a shell follows it.
    #[serde(default)]
    pub startup: String,
    /// Local forwards: `8080:localhost:80`, or `5432` for the same port.
    #[serde(default)]
    pub forwards: Vec<String>,
    /// Unix seconds.
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub last_connected_at: u64,
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or_default();
    format!("{nanos:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

impl SshHost {
    pub fn new(name: &str, host: &str) -> SshHost {
        SshHost {
            id: new_id(),
            name: name.trim().to_string(),
            host: host.trim().to_string(),
            port: None,
            user: String::new(),
            auth: Auth::Default,
            identity_file: String::new(),
            jump: String::new(),
            group: String::new(),
            remote_dir: String::new(),
            startup: String::new(),
            forwards: Vec::new(),
            created_at: 0,
            last_connected_at: 0,
        }
    }

    /// The name, else `user@host`, else a placeholder.
    pub fn label(&self) -> String {
        if !self.name.trim().is_empty() {
            self.name.trim().to_string()
        } else if !self.host.trim().is_empty() {
            self.address()
        } else {
            "(new server)".into()
        }
    }

    /// `user@host:port`, leaving out what isn't set.
    pub fn address(&self) -> String {
        let mut s = String::new();
        if !self.user.trim().is_empty() {
            s.push_str(self.user.trim());
            s.push('@');
        }
        s.push_str(self.host.trim());
        if let Some(port) = self.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{port}"));
        }
        s
    }

    /// What is wrong with the settings, if anything stops a connection.
    pub fn problem(&self) -> Option<String> {
        let host = self.host.trim();
        if host.is_empty() {
            return Some("Enter the server's address.".into());
        }
        if host.starts_with('-') || host.contains(char::is_whitespace) {
            return Some("The address can't start with “-” or contain spaces.".into());
        }
        if self.user.trim().starts_with('-') || self.user.trim().contains(char::is_whitespace) {
            return Some("The user name can't start with “-” or contain spaces.".into());
        }
        if self.auth == Auth::Key && self.identity_file.trim().is_empty() {
            return Some("Choose a private key file, or switch to another sign-in method.".into());
        }
        for f in &self.forwards {
            if parse_forward(f).is_none() {
                return Some(format!(
                    "“{f}” isn't a port forward. Use 8080:localhost:80, or just 5432."
                ));
            }
        }
        None
    }

    /// Keychain account for this host's password.
    pub(super) fn keychain_account(&self) -> &str {
        &self.id
    }
}

/// A local forward as `ssh -L` takes it: `5432` becomes `5432:localhost:5432`.
pub fn parse_forward(spec: &str) -> Option<String> {
    let spec = spec.trim();
    let port = |s: &str| s.parse::<u16>().ok().filter(|p| *p > 0);
    let parts: Vec<&str> = spec.split(':').collect();
    match parts.as_slice() {
        [p] => port(p).map(|p| format!("{p}:localhost:{p}")),
        [local, host, remote] if !host.is_empty() && !host.contains(char::is_whitespace) => {
            port(local)?;
            port(remote)?;
            Some(spec.to_string())
        }
        _ => None,
    }
}

/// Splits `8080:localhost:80, 5432` (commas or spaces) into forwards.
pub fn split_forwards(text: &str) -> Vec<String> {
    text.split([',', ' ', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Seconds since the Unix epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
