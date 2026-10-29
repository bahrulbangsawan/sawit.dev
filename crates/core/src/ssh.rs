//! SSH to remote servers: saved hosts (one JSON file), hosts read from
//! `~/.ssh/config`, the `ssh` command line for each, saved passwords in the
//! Keychain, and a quick reachability check.
//!
//! Sessions run the system `ssh` in a terminal, so keys, the agent,
//! `known_hosts`, `ProxyJump` and the user's own config all keep working.
//! Every session to the same server shares one connection (ControlMaster):
//! a second tab opens instantly and asks for nothing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::files::write_text_atomic;
use crate::terminal::shell_quote;
use crate::{CoreError, Result};

const KEYCHAIN_SERVICE: &str = "sawit.ssh";
/// Set on the askpass helper's environment: the host whose password to give.
pub const ASKPASS_ENV: &str = "SAWIT_SSH_ASKPASS";
/// The system client; `PATH` may hold an older Homebrew one, which is fine too.
const SSH: &str = "ssh";

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
    fn keychain_account(&self) -> &str {
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

// ---------------------------------------------------------------- saved hosts

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostList {
    #[serde(default)]
    pub hosts: Vec<SshHost>,
}

impl HostList {
    /// A missing file is an empty list; an unreadable one is an error, so it
    /// is never replaced by an empty list.
    pub fn load(path: &Path) -> Result<HostList> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| CoreError::Parse(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HostList::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| CoreError::Parse(e.to_string()))?;
        write_text_atomic(path, &text)
    }

    pub fn add(&mut self, mut host: SshHost, now: u64) -> String {
        host.created_at = now;
        let id = host.id.clone();
        self.hosts.push(host);
        id
    }

    pub fn get(&self, id: &str) -> Option<&SshHost> {
        self.hosts.iter().find(|h| h.id == id)
    }

    pub fn update(&mut self, id: &str, edit: impl FnOnce(&mut SshHost)) -> bool {
        match self.hosts.iter_mut().find(|h| h.id == id) {
            Some(h) => {
                let before = h.clone();
                edit(h);
                *h != before
            }
            None => false,
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.hosts.len();
        self.hosts.retain(|h| h.id != id);
        self.hosts.len() != before
    }

    /// Hosts matching every word of `query` (name, address, group), grouped
    /// (ungrouped first) and by name within a group.
    pub fn search(&self, query: &str) -> Vec<&SshHost> {
        let words = query_words(query);
        let mut found: Vec<&SshHost> = self
            .hosts
            .iter()
            .filter(|h| {
                let hay = format!("{} {} {} {}", h.name, h.address(), h.group, h.jump)
                    .to_lowercase();
                words.iter().all(|w| hay.contains(w))
            })
            .collect();
        found.sort_by_cached_key(|h| (h.group.trim().to_lowercase(), h.label().to_lowercase()));
        found
    }
}

fn query_words(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(|w| w.to_lowercase())
        .collect()
}

// ---------------------------------------------------------------- ~/.ssh/config

/// A `Host` entry from `~/.ssh/config` (patterns left out: they can't be
/// connected to by name).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigHost {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub jump: Option<String>,
}

impl ConfigHost {
    /// `user@hostname:port` as far as the config says.
    pub fn address(&self) -> String {
        let mut s = String::new();
        if let Some(user) = &self.user {
            s.push_str(user);
            s.push('@');
        }
        s.push_str(self.hostname.as_deref().unwrap_or(&self.alias));
        if let Some(port) = self.port.filter(|p| *p != 22) {
            s.push_str(&format!(":{port}"));
        }
        s
    }

    pub fn matches(&self, query: &str) -> bool {
        let hay = format!("{} {}", self.alias, self.address()).to_lowercase();
        query_words(query).iter().all(|w| hay.contains(w))
    }
}

/// The user's `~/.ssh/config`.
pub fn user_config_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".ssh").join("config"))
}

/// Hosts named in `~/.ssh/config` and the files it `Include`s.
pub fn load_config_hosts() -> Vec<ConfigHost> {
    let Some(path) = user_config_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let ssh_dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut out = Vec::new();
    parse_config(&text, &ssh_dir, &mut |p| std::fs::read_to_string(p).ok(), 0, &mut out);
    out
}

/// Parses config `text`; `Include` paths are relative to `ssh_dir` and may
/// end in a `*` pattern. The first value of each keyword wins, as in `ssh`.
pub fn parse_config(
    text: &str,
    ssh_dir: &Path,
    read: &mut dyn FnMut(&Path) -> Option<String>,
    depth: usize,
    out: &mut Vec<ConfigHost>,
) {
    // Indices into `out` the current `Host` line declared.
    let mut current: Vec<usize> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_lowercase(), v.trim().trim_start_matches('=').trim()),
            None => continue,
        };
        let value = value.trim_matches('"');
        match key.as_str() {
            "host" => {
                current.clear();
                for alias in value.split_whitespace() {
                    let alias = alias.trim_matches('"');
                    if alias.contains(['*', '?', '!']) {
                        continue;
                    }
                    let ix = match out.iter().position(|h| h.alias == alias) {
                        Some(ix) => ix,
                        None => {
                            out.push(ConfigHost {
                                alias: alias.to_string(),
                                ..ConfigHost::default()
                            });
                            out.len() - 1
                        }
                    };
                    current.push(ix);
                }
            }
            // A `Match` block's settings apply conditionally; don't guess.
            "match" => current.clear(),
            "include" if depth < 8 => {
                for pattern in value.split_whitespace() {
                    for file in expand_include(pattern, ssh_dir) {
                        if let Some(text) = read(&file) {
                            parse_config(&text, ssh_dir, read, depth + 1, out);
                        }
                    }
                }
                // An included file ends its own blocks, not the includer's;
                // keep it simple and stop applying settings until the next `Host`.
                current.clear();
            }
            "hostname" => set_each(out, &current, |h| &mut h.hostname, value),
            "user" => set_each(out, &current, |h| &mut h.user, value),
            "proxyjump" => set_each(out, &current, |h| &mut h.jump, value),
            "port" => {
                if let Ok(port) = value.parse::<u16>() {
                    for &ix in &current {
                        out[ix].port.get_or_insert(port);
                    }
                }
            }
            _ => {}
        }
    }
}

fn set_each(
    out: &mut [ConfigHost],
    current: &[usize],
    field: impl Fn(&mut ConfigHost) -> &mut Option<String>,
    value: &str,
) {
    for &ix in current {
        field(&mut out[ix]).get_or_insert_with(|| value.to_string());
    }
}

/// `Include` targets: `~/` and relative paths resolved, a `*` in the file
/// name matched against the directory's entries (sorted, as `ssh` does).
fn expand_include(pattern: &str, ssh_dir: &Path) -> Vec<PathBuf> {
    let path = if let Some(rest) = pattern.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home).join(rest),
            None => return Vec::new(),
        }
    } else if Path::new(pattern).is_absolute() {
        PathBuf::from(pattern)
    } else {
        ssh_dir.join(pattern)
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !name.contains('*') {
        return vec![path];
    }
    let Some(dir) = path.parent() else {
        return Vec::new();
    };
    let (prefix, suffix) = name.split_once('*').unwrap_or((&name, ""));
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name().is_some_and(|n| {
                let n = n.to_string_lossy();
                n.len() >= prefix.len() + suffix.len()
                    && n.starts_with(prefix)
                    && n.ends_with(suffix)
            })
        })
        .collect();
    files.sort();
    files
}

// ---------------------------------------------------------------- command line

/// Where connections are shared between sessions. `%C` is a hash of the
/// destination; kept short because socket paths max out at 104 bytes.
fn control_path() -> String {
    "~/.ssh/sawit-%C".into()
}

/// Options every Sawit session uses: share one connection per server, notice
/// a dead network within a minute and a dead address within 10 seconds.
fn common_options() -> Vec<String> {
    [
        "ControlMaster=auto".to_string(),
        format!("ControlPath={}", control_path()),
        "ControlPersist=10m".into(),
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
/// with the saved password, see [`askpass_main`]) the password is filled in.
pub fn launch(host: &SshHost, askpass: Option<&Path>) -> Launch {
    let mut args = common_options();
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
    let mut args = common_options();
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

// ---------------------------------------------------------------- Keychain

pub fn load_password(host: &SshHost) -> Result<Option<String>> {
    load_password_for(host.keychain_account())
}

fn load_password_for(account: &str) -> Result<Option<String>> {
    crate::keychain::get(KEYCHAIN_SERVICE, account)
}

/// Whether a password is saved for `host`, without reading (or asking for) it.
pub fn has_password(host: &SshHost) -> bool {
    matches!(crate::keychain::exists(KEYCHAIN_SERVICE, host.keychain_account()), Ok(true))
}

pub fn store_password(host: &SshHost, password: &str) -> Result<()> {
    crate::keychain::set(KEYCHAIN_SERVICE, host.keychain_account(), password)
}

pub fn delete_password(host: &SshHost) -> Result<()> {
    crate::keychain::delete(KEYCHAIN_SERVICE, host.keychain_account())
}

// ---------------------------------------------------------------- askpass

/// Whether `ssh` is asking for the account password (not a key passphrase,
/// a one-time code or a host key confirmation).
pub fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    p.contains("password") && !p.contains("passphrase") && !p.contains("yes/no")
}

/// Runs as `SSH_ASKPASS`: `ssh` passes the prompt and reads the answer from
/// stdout. The first password prompt gets the saved password; anything else
/// (a retry after a wrong password, a code, `yes/no`) is asked in the
/// terminal the session runs in. Returns the exit code.
pub fn askpass_main(account: &str, prompt: &str) -> i32 {
    if is_password_prompt(prompt)
        && first_prompt_of_parent()
        && let Ok(Some(password)) = load_password_for(account)
    {
        println!("{password}");
        return 0;
    }
    match ask_tty(prompt) {
        Some(answer) => {
            println!("{answer}");
            0
        }
        None => 1,
    }
}

/// True the first time this is asked for the calling `ssh` process, so a
/// rejected password is typed by hand instead of retried.
fn first_prompt_of_parent() -> bool {
    #[cfg(unix)]
    {
        let ppid = unsafe { libc::getppid() };
        let marker = std::env::temp_dir().join(format!("sawit-askpass-{ppid}"));
        let fresh = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .is_ok();
        // Markers of finished sessions are removed as they are noticed.
        if fresh && let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(pid) = name.strip_prefix("sawit-askpass-")
                    && let Ok(pid) = pid.parse::<i32>()
                    && pid != ppid
                    && unsafe { libc::kill(pid, 0) } != 0
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        fresh
    }
    #[cfg(not(unix))]
    true
}

/// Asks on the controlling terminal; input is hidden unless it is a
/// `yes/no` question.
fn ask_tty(prompt: &str) -> Option<String> {
    #[cfg(unix)]
    {
        use std::io::{BufRead, Write};
        use std::os::fd::AsRawFd;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let mut out = &tty;
        let _ = out.write_all(prompt.as_bytes());
        let _ = out.flush();
        let fd = tty.as_raw_fd();
        let hide = !prompt.to_lowercase().contains("yes/no");
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        let have = unsafe { libc::tcgetattr(fd, &mut saved) } == 0;
        if hide && have {
            let mut quiet = saved;
            quiet.c_lflag &= !libc::ECHO;
            quiet.c_lflag |= libc::ECHONL;
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &quiet) };
        }
        let mut line = String::new();
        let read = std::io::BufReader::new(&tty).read_line(&mut line);
        if hide && have {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
        }
        read.ok().filter(|n| *n > 0)?;
        Some(line.trim_end_matches(['\r', '\n']).to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = prompt;
        None
    }
}

// ---------------------------------------------------------------- probe

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// The SSH server answered: its version banner and how long it took.
    Up { latency_ms: u64, banner: String },
    /// Reached through a jump host or proxy command; not checked directly.
    Indirect,
    Down(String),
}

/// Asks `ssh -G` where `args` (connection arguments) really lead, then
/// opens a TCP connection there and reads the server's banner.
async fn probe_args(args: Vec<String>) -> Probe {
    let resolved = tokio::process::Command::new(SSH)
        .arg("-G")
        .args(&args)
        .stdin(std::process::Stdio::null())
        .output()
        .await;
    let output = match resolved {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => return Probe::Down(String::from_utf8_lossy(&o.stderr).trim().to_string()),
        Err(e) => return Probe::Down(format!("Unable to run ssh. {e}")),
    };
    let (mut hostname, mut port) = (String::new(), 22u16);
    for line in output.lines() {
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        match k {
            "hostname" => hostname = v.to_string(),
            "port" => port = v.parse().unwrap_or(22),
            "proxyjump" | "proxycommand" if v != "none" && !v.is_empty() => {
                return Probe::Indirect;
            }
            _ => {}
        }
    }
    probe_tcp(&hostname, port).await
}

async fn probe_tcp(hostname: &str, port: u16) -> Probe {
    use tokio::io::AsyncReadExt;
    let started = Instant::now();
    let connect = tokio::net::TcpStream::connect((hostname, port));
    let mut stream = match tokio::time::timeout(Duration::from_secs(5), connect).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Probe::Down(friendly_io(&e, hostname, port)),
        Err(_) => return Probe::Down(format!("{hostname}:{port} didn't answer within 5 seconds.")),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    let mut buf = [0u8; 256];
    let banner = match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf)).await {
        Ok(Ok(n)) => String::from_utf8_lossy(&buf[..n])
            .lines()
            .find(|l| l.starts_with("SSH-"))
            .map(|l| l.trim().to_string()),
        _ => None,
    };
    match banner {
        Some(banner) => Probe::Up { latency_ms, banner },
        None => Probe::Down(format!(
            "Port {port} is open on {hostname}, but no SSH server answered."
        )),
    }
}

fn friendly_io(e: &std::io::Error, hostname: &str, port: u16) -> String {
    match e.kind() {
        std::io::ErrorKind::ConnectionRefused => {
            format!("{hostname} refused the connection on port {port}. Is sshd running?")
        }
        _ if e.to_string().contains("failed to lookup") || e.to_string().contains("nodename") => {
            format!("Unable to find {hostname}. Check the address.")
        }
        _ => format!("Unable to reach {hostname}:{port}. {e}"),
    }
}

pub async fn probe_host(host: SshHost) -> Probe {
    if let Some(problem) = host.problem() {
        return Probe::Down(problem);
    }
    probe_args(connection_args(&host)).await
}

pub async fn probe_alias(alias: String) -> Probe {
    probe_args(vec!["--".into(), alias]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> SshHost {
        let mut h = SshHost::new("web", "203.0.113.7");
        h.id = "abc".into();
        h
    }

    #[test]
    fn plain_host_is_just_the_destination_after_the_options() {
        let l = launch(&host(), None);
        assert_eq!(l.program, "ssh");
        assert_eq!(l.args.last().unwrap(), "203.0.113.7");
        assert!(l.args.contains(&"ControlMaster=auto".to_string()));
        assert!(!l.args.contains(&"-t".to_string()));
        assert!(l.env.is_empty());
    }

    #[test]
    fn user_port_key_and_jump_become_flags() {
        let mut h = host();
        h.user = "deploy".into();
        h.port = Some(2222);
        h.auth = Auth::Key;
        h.identity_file = "/k/id_ed25519".into();
        h.jump = "me@bastion".into();
        let a = launch(&h, None).args;
        let pos = |s: &str| a.iter().position(|x| x == s).unwrap();
        assert_eq!(a[pos("-p") + 1], "2222");
        assert_eq!(a[pos("-l") + 1], "deploy");
        assert_eq!(a[pos("-i") + 1], "/k/id_ed25519");
        assert!(a.contains(&"IdentitiesOnly=yes".to_string()));
        assert_eq!(a[pos("-J") + 1], "me@bastion");
    }

    #[test]
    fn key_file_is_ignored_unless_key_auth_is_chosen() {
        let mut h = host();
        h.identity_file = "/k/id".into();
        assert!(!launch(&h, None).args.contains(&"-i".to_string()));
    }

    #[test]
    fn remote_dir_and_startup_run_before_a_login_shell() {
        let mut h = host();
        h.remote_dir = "~/apps/my app".into();
        h.startup = "tmux new -A -s main".into();
        let a = launch(&h, None).args;
        assert!(a.contains(&"-t".to_string()));
        assert_eq!(
            a.last().unwrap(),
            "cd ~/'apps/my app'; tmux new -A -s main; exec \"$SHELL\" -l"
        );
        assert_eq!(a[a.len() - 2], "203.0.113.7");
    }

    #[test]
    fn forwards_accept_a_bare_port() {
        assert_eq!(parse_forward("5432").as_deref(), Some("5432:localhost:5432"));
        assert_eq!(
            parse_forward("8080:localhost:80").as_deref(),
            Some("8080:localhost:80")
        );
        assert_eq!(parse_forward("80:x"), None);
        assert_eq!(parse_forward("abc"), None);
        let mut h = host();
        h.forwards = vec!["5432".into()];
        let a = launch(&h, None).args;
        let i = a.iter().position(|x| x == "-L").unwrap();
        assert_eq!(a[i + 1], "5432:localhost:5432");
    }

    #[test]
    fn saved_password_uses_the_askpass_helper() {
        let mut h = host();
        h.auth = Auth::Password;
        let l = launch(&h, Some(Path::new("/A/sawit")));
        assert!(l.env.contains(&("SSH_ASKPASS".into(), "/A/sawit".into())));
        assert!(l.env.contains(&("SSH_ASKPASS_REQUIRE".into(), "force".into())));
        assert!(l.env.contains(&(ASKPASS_ENV.into(), "abc".into())));
        // Without a helper (no saved password) ssh asks in the terminal.
        assert!(launch(&h, None).env.is_empty());
    }

    #[test]
    fn options_cannot_be_smuggled_through_the_address() {
        let mut h = host();
        h.host = "-oProxyCommand=evil".into();
        assert!(h.problem().is_some());
        h.host = "ok".into();
        h.user = "-x".into();
        assert!(h.problem().is_some());
        assert!(launch_alias("-oX").args.ends_with(&["--".into(), "-oX".into()]));
    }

    #[test]
    fn command_line_is_shell_quoted() {
        let mut h = host();
        h.startup = "echo hi".into();
        assert!(launch(&h, None).command_line().ends_with("'echo hi; exec \"$SHELL\" -l'"));
    }

    #[test]
    fn plain_command_leaves_out_sawit_options() {
        let mut h = host();
        h.user = "root".into();
        h.port = Some(2222);
        assert_eq!(plain_command(&h), "ssh -p 2222 -l root 203.0.113.7");
    }

    #[test]
    fn password_prompts_are_told_apart() {
        assert!(is_password_prompt("root@203.0.113.7's password: "));
        assert!(is_password_prompt("(deploy@web) Password:"));
        assert!(!is_password_prompt("Enter passphrase for key '/k/id': "));
        assert!(!is_password_prompt("Verification code: "));
        assert!(!is_password_prompt(
            "Are you sure you want to continue connecting (yes/no/[fingerprint])? "
        ));
    }

    fn parse(text: &str, files: &[(&str, &str)]) -> Vec<ConfigHost> {
        let mut out = Vec::new();
        let mut read = |p: &Path| {
            files
                .iter()
                .find(|(name, _)| p == Path::new("/home/.ssh").join(name))
                .map(|(_, t)| t.to_string())
        };
        parse_config(text, Path::new("/home/.ssh"), &mut read, 0, &mut out);
        out
    }

    #[test]
    fn config_hosts_skip_patterns_and_keep_the_first_value() {
        let hosts = parse(
            "Host *\n  User everyone\n\nHost web prod-web\n  HostName 203.0.113.7\n  User deploy\n  Port 2222\n  User ignored\n\nHost db\n  Hostname=10.0.0.5\n  ProxyJump web\nHost *.internal !x\n",
            &[],
        );
        assert_eq!(hosts.len(), 3);
        assert_eq!(hosts[0].alias, "web");
        assert_eq!(hosts[0].address(), "deploy@203.0.113.7:2222");
        assert_eq!(hosts[1].alias, "prod-web");
        assert_eq!(hosts[1].user.as_deref(), Some("deploy"));
        assert_eq!(hosts[2].hostname.as_deref(), Some("10.0.0.5"));
        assert_eq!(hosts[2].jump.as_deref(), Some("web"));
    }

    #[test]
    fn config_includes_are_followed() {
        let hosts = parse(
            "Include extra\nHost a\n",
            &[("extra", "Host orb\n  HostName 127.0.0.1\n  Port 32222\n")],
        );
        assert_eq!(
            hosts.iter().map(|h| h.alias.as_str()).collect::<Vec<_>>(),
            ["orb", "a"]
        );
        assert_eq!(hosts[0].port, Some(32222));
    }

    #[test]
    fn match_blocks_do_not_leak_into_the_previous_host() {
        let hosts = parse("Host a\nMatch host b\n  User nope\n", &[]);
        assert_eq!(hosts[0].user, None);
    }

    #[test]
    fn search_groups_then_sorts_by_name() {
        let mut list = HostList::default();
        for (name, group) in [("zeta", ""), ("alpha", "prod"), ("beta", "")] {
            let mut h = SshHost::new(name, "h");
            h.group = group.into();
            list.add(h, 0);
        }
        let names: Vec<_> = list.search("").iter().map(|h| h.name.clone()).collect();
        assert_eq!(names, ["beta", "zeta", "alpha"]);
        assert_eq!(list.search("PROD").len(), 1);
    }

    #[test]
    fn host_list_round_trips_and_tolerates_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ssh_hosts.json");
        assert_eq!(HostList::load(&file).unwrap(), HostList::default());
        let mut list = HostList::default();
        list.add(host(), 5);
        list.save(&file).unwrap();
        assert_eq!(HostList::load(&file).unwrap(), list);
        let old = r#"{"hosts":[{"id":"x","name":"n","host":"h"}]}"#;
        std::fs::write(&file, old).unwrap();
        let loaded = HostList::load(&file).unwrap();
        assert_eq!(loaded.hosts[0].auth, Auth::Default);
        std::fs::write(&file, "{oops").unwrap();
        assert!(HostList::load(&file).is_err());
    }

    #[tokio::test]
    async fn probe_reads_the_ssh_banner() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let (mut s, _) = listener.accept().await.unwrap();
            s.write_all(b"SSH-2.0-OpenSSH_9.6\r\n").await.unwrap();
        });
        match probe_tcp("127.0.0.1", port).await {
            Probe::Up { banner, .. } => assert_eq!(banner, "SSH-2.0-OpenSSH_9.6"),
            other => panic!("{other:?}"),
        }
    }
}
