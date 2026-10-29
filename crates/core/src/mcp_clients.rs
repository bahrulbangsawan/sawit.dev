//! Installing MCP servers (the bundled `sawit-mcp`, and the catalog in
//! `mcp_catalog`) into AI clients: Claude Code (every
//! profile — the default `~/.claude.json` and each `CLAUDE_CONFIG_DIR`
//! folder such as `~/.claude-work`), Claude Desktop, Codex (and `CODEX_HOME`
//! profiles), Cursor, Windsurf, Gemini CLI, Zed, opencode and VS Code.
//!
//! Claude Code and Codex are changed through their own CLI when it is
//! installed (they rewrite their config files while running); everything
//! else, and those two without a CLI, by editing the config file in place —
//! other settings, key order and TOML comments are kept, and the original is
//! backed up once next to it. Remote servers are written in each client's
//! own format; clients that only run local programs get the `mcp-remote`
//! bridge.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::files::write_text_atomic;
use crate::{CoreError, Result};

/// The server's name in every client's config.
pub const SERVER_NAME: &str = "sawit";

/// The name used before the rename to Sawit (PG Desk). Its entry points at
/// the old app, so installing removes it.
const LEGACY_SERVER_NAME: &str = "pgdesk";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    // Agents and CLIs
    ClaudeCode,
    Codex,
    Gemini,
    Qwen,
    OpenCode,
    Amp,
    CopilotCli,
    FactoryDroid,
    Kiro,
    Cline,
    Auggie,
    AmazonQ,
    Junie,
    Warp,
    Goose,
    Crush,
    // Editors and IDEs
    ClaudeDesktop,
    Cursor,
    Windsurf,
    VsCode,
    VsCodeInsiders,
    Zed,
    Antigravity,
    Trae,
    RooCode,
    KiloCode,
    Continue,
    LmStudio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Coding agents and CLIs.
    Agent,
    /// Editors, IDEs and desktop apps.
    Editor,
}

impl Client {
    pub const ALL: [Client; 28] = [
        Client::ClaudeCode,
        Client::Codex,
        Client::Gemini,
        Client::Qwen,
        Client::OpenCode,
        Client::Amp,
        Client::CopilotCli,
        Client::FactoryDroid,
        Client::Kiro,
        Client::Cline,
        Client::Auggie,
        Client::AmazonQ,
        Client::Junie,
        Client::Warp,
        Client::Goose,
        Client::Crush,
        Client::ClaudeDesktop,
        Client::Cursor,
        Client::Windsurf,
        Client::VsCode,
        Client::VsCodeInsiders,
        Client::Zed,
        Client::Antigravity,
        Client::Trae,
        Client::RooCode,
        Client::KiloCode,
        Client::Continue,
        Client::LmStudio,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::Gemini => "Gemini CLI",
            Client::Qwen => "Qwen Code",
            Client::OpenCode => "opencode",
            Client::Amp => "Amp",
            Client::CopilotCli => "GitHub Copilot CLI",
            Client::FactoryDroid => "Factory Droid",
            Client::Kiro => "Kiro",
            Client::Cline => "Cline",
            Client::Auggie => "Augment (Auggie)",
            Client::AmazonQ => "Amazon Q CLI",
            Client::Junie => "JetBrains Junie",
            Client::Warp => "Warp",
            Client::Goose => "Goose",
            Client::Crush => "Crush",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Cursor => "Cursor",
            Client::Windsurf => "Windsurf / Devin",
            Client::VsCode => "VS Code",
            Client::VsCodeInsiders => "VS Code Insiders",
            Client::Zed => "Zed",
            Client::Antigravity => "Antigravity",
            Client::Trae => "Trae",
            Client::RooCode => "Roo Code",
            Client::KiloCode => "Kilo Code",
            Client::Continue => "Continue",
            Client::LmStudio => "LM Studio",
        }
    }

    pub fn category(self) -> Category {
        match self {
            Client::ClaudeDesktop
            | Client::Cursor
            | Client::Windsurf
            | Client::VsCode
            | Client::VsCodeInsiders
            | Client::Zed
            | Client::Antigravity
            | Client::Trae
            | Client::RooCode
            | Client::KiloCode
            | Client::Continue
            | Client::LmStudio => Category::Editor,
            _ => Category::Agent,
        }
    }

    /// Whether Sawit can edit the config (JSON/TOML). YAML and script
    /// configs get a snippet to paste instead.
    pub fn editable(self) -> bool {
        !matches!(self, Client::Goose | Client::Continue | Client::Crush)
    }

    /// The environment variable that selects a profile folder, for clients
    /// that have profiles.
    pub fn profile_var(self) -> Option<&'static str> {
        match self {
            Client::ClaudeCode => Some("CLAUDE_CONFIG_DIR"),
            Client::Codex => Some("CODEX_HOME"),
            _ => None,
        }
    }

    /// The client's own CLI, used instead of editing files when installed.
    pub fn cli(self) -> Option<&'static str> {
        match self {
            Client::ClaudeCode => Some("claude"),
            Client::Codex => Some("codex"),
            _ => None,
        }
    }
}

/// One place a server can be installed: a client, and for Claude Code and
/// Codex one of its profiles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub client: Client,
    /// "default", or the profile folder's suffix (`.claude-work` → "work").
    pub profile: String,
    pub config: PathBuf,
    /// The folder `profile_var` must point at; `None` for the default
    /// profile, which must run with the variable unset.
    pub profile_dir: Option<PathBuf>,
}

fn target(client: Client, profile: &str, config: PathBuf, profile_dir: Option<PathBuf>) -> Target {
    Target {
        client,
        profile: profile.into(),
        config,
        profile_dir,
    }
}

/// Folders in `home` named `<prefix>-<profile>` that contain `marker`.
fn profiles(home: &Path, prefix: &str, marker: &str) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let profile = name.strip_prefix(prefix)?.strip_prefix('-')?.to_string();
            let dir = e.path();
            (dir.is_dir() && dir.join(marker).exists()).then_some((profile, dir))
        })
        .collect();
    found.sort();
    found
}

/// Every installed client (and profile) under `home`. `claude_config_dir`
/// is the app's `CLAUDE_CONFIG_DIR`, included when it lives elsewhere.
pub fn discover(home: &Path, claude_config_dir: Option<PathBuf>) -> Vec<Target> {
    let mut out = Vec::new();
    if home.join(".claude.json").exists() || home.join(".claude").is_dir() {
        out.push(target(
            Client::ClaudeCode,
            "default",
            home.join(".claude.json"),
            None,
        ));
    }
    for (profile, dir) in profiles(home, ".claude", ".claude.json") {
        out.push(target(
            Client::ClaudeCode,
            &profile,
            dir.join(".claude.json"),
            Some(dir),
        ));
    }
    if let Some(dir) = claude_config_dir
        && dir.join(".claude.json").exists()
    {
        let canonical = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let known = out.iter().any(|t| {
            t.profile_dir
                .as_deref()
                .is_some_and(|d| canonical(d) == canonical(&dir))
        });
        if !known {
            let profile = dir
                .file_name()
                .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
                .unwrap_or_else(|| "CLAUDE_CONFIG_DIR".into());
            out.push(target(
                Client::ClaudeCode,
                &profile,
                dir.join(".claude.json"),
                Some(dir),
            ));
        }
    }
    let desktop = home.join("Library/Application Support/Claude");
    if desktop.is_dir() {
        out.push(target(
            Client::ClaudeDesktop,
            "default",
            desktop.join("claude_desktop_config.json"),
            None,
        ));
    }
    if home.join(".codex").is_dir() {
        out.push(target(
            Client::Codex,
            "default",
            home.join(".codex/config.toml"),
            None,
        ));
    }
    for (profile, dir) in profiles(home, ".codex", "config.toml") {
        out.push(target(
            Client::Codex,
            &profile,
            dir.join("config.toml"),
            Some(dir),
        ));
    }
    // (client, folder that shows it's installed, config file in it). Paths
    // checked against each tool's documentation.
    let documented: [(Client, &str, &str); 19] = [
        (Client::Gemini, ".gemini", "settings.json"),
        (Client::Qwen, ".qwen", "settings.json"),
        (Client::OpenCode, ".config/opencode", "opencode.json"),
        (Client::Amp, ".config/amp", "settings.json"),
        (Client::CopilotCli, ".copilot", "mcp-config.json"),
        (Client::FactoryDroid, ".factory", "mcp.json"),
        (Client::Kiro, ".kiro", "settings/mcp.json"),
        (
            Client::Cline,
            ".cline",
            "data/settings/cline_mcp_settings.json",
        ),
        (Client::Auggie, ".augment", "settings.json"),
        (Client::Junie, ".junie", "mcp/mcp.json"),
        (Client::Warp, ".warp", ".mcp.json"),
        (Client::Goose, ".config/goose", "config.yaml"),
        (Client::Crush, ".config/crush", "crushrc"),
        (Client::Cursor, ".cursor", "mcp.json"),
        (Client::Windsurf, ".config/devin", "mcp_config.json"),
        (Client::Zed, ".config/zed", "settings.json"),
        (Client::Antigravity, ".gemini/config", "mcp_config.json"),
        (Client::KiloCode, ".config/kilo", "kilo.json"),
        (Client::Continue, ".continue", "config.yaml"),
    ];
    let documented_mac: [(Client, &str, &str); 2] = [
        (
            Client::VsCode,
            "Library/Application Support/Code/User",
            "mcp.json",
        ),
        (
            Client::VsCodeInsiders,
            "Library/Application Support/Code - Insiders/User",
            "mcp.json",
        ),
    ];
    for (client, dir, file) in documented.into_iter().chain(documented_mac) {
        let dir = home.join(dir);
        if dir.is_dir() {
            out.push(target(client, "default", dir.join(file), None));
        }
    }
    // Unconfirmed or older locations: offered only when the file itself is
    // there, so a wrong guess never creates a stray file.
    let existing: [(Client, &str); 7] = [
        (Client::LmStudio, ".lmstudio/mcp.json"),
        (Client::AmazonQ, ".aws/amazonq/mcp.json"),
        (
            Client::Trae,
            "Library/Application Support/Trae/User/mcp.json",
        ),
        (
            Client::RooCode,
            "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings/mcp_settings.json",
        ),
        (
            Client::Cline,
            "Library/Application Support/Code/User/globalStorage/saoudrizwan.claude-dev/settings/cline_mcp_settings.json",
        ),
        (Client::Windsurf, ".codeium/windsurf/mcp_config.json"),
        (Client::Antigravity, ".gemini/antigravity/mcp_config.json"),
    ];
    for (client, file) in existing {
        let file = home.join(file);
        if file.is_file() && !out.iter().any(|t| t.config == file) {
            let profile = if out.iter().any(|t| t.client == client) {
                "legacy"
            } else {
                "default"
            };
            out.push(target(client, profile, file, None));
        }
    }
    // LM Studio's folder is documented even if mcp.json isn't there yet.
    if home.join(".lmstudio").is_dir() && !out.iter().any(|t| t.client == Client::LmStudio) {
        out.push(target(
            Client::LmStudio,
            "default",
            home.join(".lmstudio/mcp.json"),
            None,
        ));
    }
    out
}

/// How a server is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// A program on this Mac.
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// A remote (streamable HTTP) server.
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// A server to install: its name in every config, and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub transport: Transport,
    /// `npx` for the `mcp-remote` bridge, which clients without remote
    /// support use. An absolute path when found: GUI clients get a minimal
    /// `PATH`.
    pub npx: String,
}

impl Spec {
    /// The bundled `sawit-mcp`.
    pub fn sawit(server: &Path) -> Spec {
        Spec::stdio(SERVER_NAME, server.display().to_string(), Vec::new())
    }

    pub fn stdio(name: &str, command: impl Into<String>, args: Vec<String>) -> Spec {
        Spec {
            name: name.into(),
            transport: Transport::Stdio {
                command: command.into(),
                args,
                env: Vec::new(),
            },
            npx: "npx".into(),
        }
    }

    pub fn http(name: &str, url: impl Into<String>) -> Spec {
        Spec {
            name: name.into(),
            transport: Transport::Http {
                url: url.into(),
                headers: Vec::new(),
            },
            npx: "npx".into(),
        }
    }

    pub fn header(mut self, key: &str, value: &str) -> Spec {
        if let Transport::Http { headers, .. } = &mut self.transport {
            headers.push((key.into(), value.into()));
        }
        self
    }

    pub fn with_npx(mut self, npx: impl Into<String>) -> Spec {
        self.npx = npx.into();
        self
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.transport, Transport::Http { .. })
    }

    /// What identifies this server in a config entry: the URL; else the
    /// arguments, since the command may be found at another path; else, for
    /// a server without arguments, the command itself.
    fn key(&self) -> String {
        match &self.transport {
            Transport::Http { url, .. } => url.clone(),
            Transport::Stdio { command, args, .. } if args.is_empty() => command.clone(),
            Transport::Stdio { args, .. } => args.join(" "),
        }
    }

    /// `VAR` in an `Authorization: Bearer ${VAR}` header, the only header.
    fn bearer_env(&self) -> Option<&str> {
        let Transport::Http { headers, .. } = &self.transport else {
            return None;
        };
        match headers.as_slice() {
            [(k, v)] if k.eq_ignore_ascii_case("authorization") => v
                .strip_prefix("Bearer ${")
                .and_then(|v| v.strip_suffix('}')),
            _ => None,
        }
    }

    /// The server as a program to run: itself, or for a remote server the
    /// `mcp-remote` bridge.
    fn as_stdio(&self) -> (String, Vec<String>, Vec<(String, String)>) {
        match &self.transport {
            Transport::Stdio { command, args, env } => (command.clone(), args.clone(), env.clone()),
            Transport::Http { url, headers } => {
                let mut args = vec!["-y".to_string(), "mcp-remote".into(), url.clone()];
                for (k, v) in headers {
                    args.push("--header".into());
                    args.push(format!("{k}:{v}"));
                }
                (self.npx.clone(), args, Vec::new())
            }
        }
    }
}

/// The key holding the servers in a JSON client config.
fn servers_key(client: Client) -> &'static str {
    match client {
        Client::Zed => "context_servers",
        Client::OpenCode | Client::KiloCode => "mcp",
        Client::VsCode | Client::VsCodeInsiders => "servers",
        // A flat, dotted key in Amp's settings.
        Client::Amp => "amp.mcpServers",
        _ => "mcpServers",
    }
}

fn object(pairs: &[(String, String)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect(),
    )
}

/// A remote server in the client's own format, or `None` when the client
/// only runs local programs (it gets the `mcp-remote` bridge instead).
fn http_entry(client: Client, url: &str, headers: &[(String, String)]) -> Option<Value> {
    let mut v = match client {
        Client::ClaudeCode | Client::VsCode | Client::VsCodeInsiders => {
            json!({"type": "http", "url": url})
        }
        Client::Cursor | Client::Amp | Client::Zed => json!({"url": url}),
        Client::Gemini | Client::Qwen => json!({"httpUrl": url}),
        Client::Windsurf | Client::Antigravity => json!({"serverUrl": url}),
        Client::OpenCode | Client::KiloCode => {
            json!({"type": "remote", "url": url, "enabled": true})
        }
        Client::CopilotCli => json!({"type": "http", "url": url, "tools": ["*"]}),
        Client::FactoryDroid => json!({"type": "http", "url": url, "disabled": false}),
        Client::Kiro => json!({"url": url, "disabled": false}),
        Client::Cline => json!({"type": "streamableHttp", "url": url, "disabled": false}),
        Client::RooCode => json!({"type": "streamable-http", "url": url}),
        _ => return None,
    };
    if !headers.is_empty() {
        v["headers"] = object(headers);
    }
    Some(v)
}

/// Where the server entry lives in a JSON client config, and its value.
pub fn entry(client: Client, spec: &Spec) -> (&'static str, Value) {
    let key = servers_key(client);
    if let Transport::Http { url, headers } = &spec.transport
        && let Some(v) = http_entry(client, url, headers)
    {
        return (key, v);
    }
    let (cmd, args, env) = spec.as_stdio();
    let env = object(&env);
    let v = match client {
        Client::ClaudeCode => json!({"type": "stdio", "command": cmd, "args": args, "env": env}),
        Client::Zed => json!({"command": cmd, "args": args, "env": env}),
        Client::OpenCode | Client::KiloCode => {
            let mut command = vec![cmd];
            command.extend(args);
            json!({"type": "local", "command": command, "enabled": true})
        }
        Client::VsCode | Client::VsCodeInsiders => {
            json!({"type": "stdio", "command": cmd, "args": args})
        }
        Client::CopilotCli => {
            json!({"type": "local", "command": cmd, "args": args, "env": env, "tools": ["*"]})
        }
        Client::FactoryDroid => {
            json!({"type": "stdio", "command": cmd, "args": args, "disabled": false})
        }
        Client::Cursor => json!({"type": "stdio", "command": cmd, "args": args}),
        Client::Kiro => json!({"command": cmd, "args": args, "disabled": false}),
        Client::Cline => {
            json!({"command": cmd, "args": args, "disabled": false, "autoApprove": []})
        }
        Client::RooCode => {
            json!({"command": cmd, "args": args, "disabled": false, "alwaysAllow": []})
        }
        _ => json!({"command": cmd, "args": args}),
    };
    let mut v = v;
    if let (Some(obj), Value::Object(env)) = (v.as_object_mut(), &env)
        && !env.is_empty()
        && !obj.contains_key("env")
        && obj.contains_key("args")
    {
        obj.insert("env".into(), Value::Object(env.clone()));
    }
    (key, v)
}

/// Every command, argument and URL in a JSON entry, space-separated.
fn signature(v: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for key in ["command", "args", "url", "httpUrl", "serverUrl"] {
        match v.get(key) {
            Some(Value::String(s)) => parts.push(s.clone()),
            Some(Value::Array(a)) => {
                parts.extend(a.iter().filter_map(|x| x.as_str().map(String::from)))
            }
            _ => {}
        }
    }
    parts.join(" ")
}

fn toml_signature(item: &toml_edit::Item) -> String {
    let mut parts: Vec<String> = Vec::new();
    for key in ["command", "args", "url"] {
        match item.get(key) {
            Some(i) if i.is_str() => parts.push(i.as_str().unwrap_or_default().into()),
            Some(i) => {
                if let Some(a) = i.as_array() {
                    parts.extend(a.iter().filter_map(|x| x.as_str().map(String::from)));
                }
            }
            None => {}
        }
    }
    parts.join(" ")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Installed,
    /// Installed under this name, but pointing elsewhere (an older build,
    /// another region): what it points at.
    Outdated(String),
    NotInstalled,
    /// The config can't be read safely (comments, syntax errors).
    Unreadable(String),
    /// YAML or script config: paste the snippet by hand.
    Manual,
}

fn read_json(path: &Path) -> std::result::Result<Option<Map<String, Value>>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if text.trim().is_empty() {
        return Ok(Some(Map::new()));
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(Some(map)),
        Ok(_) => Err("not a JSON object".into()),
        Err(e) => Err(format!("{e} (e.g. it contains comments)")),
    }
}

fn read_toml(path: &Path) -> std::result::Result<toml_edit::DocumentMut, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())
}

/// Whether `spec` is installed for `target`.
pub fn status(target: &Target, spec: &Spec) -> Status {
    let wanted = spec.key();
    if !target.client.editable() {
        let text = std::fs::read_to_string(&target.config).unwrap_or_default();
        return if text.contains(&spec.name) && text.contains(&wanted) {
            Status::Installed
        } else {
            Status::Manual
        };
    }
    let found: Option<String> = if target.client == Client::Codex {
        match read_toml(&target.config) {
            Ok(doc) => doc
                .get("mcp_servers")
                .and_then(|t| t.get(&spec.name))
                .map(toml_signature),
            Err(e) => return Status::Unreadable(e),
        }
    } else {
        match read_json(&target.config) {
            Ok(map) => map
                .as_ref()
                .and_then(|m| m.get(servers_key(target.client)))
                .and_then(|s| s.get(&spec.name))
                .map(signature),
            Err(e) => return Status::Unreadable(e),
        }
    };
    match found {
        None => Status::NotInstalled,
        Some(sig) if sig.contains(&wanted) => Status::Installed,
        Some(sig) => Status::Outdated(sig),
    }
}

/// `<config>.sawit-backup`, written once before the first change.
pub fn backup_path(config: &Path) -> PathBuf {
    let mut name = config.file_name().unwrap_or_default().to_os_string();
    name.push(".sawit-backup");
    config.with_file_name(name)
}

fn write_config(config: &Path, text: &str) -> Result<()> {
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let backup = backup_path(config);
    if config.exists() && !backup.exists() {
        std::fs::copy(config, &backup)?;
    }
    write_text_atomic(config, text)
}

fn manual(target: &Target, spec: &Spec, why: &str) -> CoreError {
    CoreError::Config(format!(
        "{} can't be changed automatically ({why}). Add it manually:\n{}",
        target.config.display(),
        snippet(target, spec)
    ))
}

fn toml_strings<'a>(items: impl IntoIterator<Item = &'a String>) -> toml_edit::Array {
    items.into_iter().map(|s| s.as_str()).collect()
}

fn toml_table(pairs: &[(String, String)]) -> toml_edit::InlineTable {
    pairs
        .iter()
        .map(|(k, v)| (k.as_str(), toml_edit::Value::from(v.as_str())))
        .collect()
}

/// Codex's `[mcp_servers.<name>]` table.
fn codex_table(spec: &Spec) -> toml_edit::Table {
    let mut table = toml_edit::Table::new();
    match &spec.transport {
        Transport::Stdio { command, args, env } => {
            table["command"] = toml_edit::value(command.as_str());
            table["args"] = toml_edit::value(toml_strings(args));
            if !env.is_empty() {
                table["env"] = toml_edit::value(toml_table(env));
            }
        }
        Transport::Http { url, headers } => {
            table["url"] = toml_edit::value(url.as_str());
            if let Some(var) = spec.bearer_env() {
                table["bearer_token_env_var"] = toml_edit::value(var);
            } else if !headers.is_empty() {
                table["http_headers"] = toml_edit::value(toml_table(headers));
            }
        }
    }
    table
}

/// Adds or updates the server by editing the config file.
pub fn install_file(target: &Target, spec: &Spec) -> Result<()> {
    if !target.client.editable() {
        return Err(manual(target, spec, "YAML/script format"));
    }
    let legacy = spec.name == SERVER_NAME;
    if target.client == Client::Codex {
        let mut doc = read_toml(&target.config).map_err(|e| manual(target, spec, &e))?;
        let servers = doc
            .entry("mcp_servers")
            .or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            })
            .as_table_mut()
            .ok_or_else(|| manual(target, spec, "mcp_servers is not a table"))?;
        servers[spec.name.as_str()] = toml_edit::Item::Table(codex_table(spec));
        if legacy {
            servers.remove(LEGACY_SERVER_NAME);
        }
        return write_config(&target.config, &doc.to_string());
    }
    let mut map = read_json(&target.config)
        .map_err(|e| manual(target, spec, &e))?
        .unwrap_or_default();
    let (key, value) = entry(target.client, spec);
    let servers = map.entry(key).or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err(manual(target, spec, &format!("{key} is not an object")));
    };
    servers.insert(spec.name.clone(), value);
    if legacy {
        servers.remove(LEGACY_SERVER_NAME);
    }
    let text = serde_json::to_string_pretty(&Value::Object(map))
        .map_err(|e| CoreError::Config(e.to_string()))?;
    write_config(&target.config, &format!("{text}\n"))
}

/// Removes the server entry (other servers and settings stay).
pub fn uninstall_file(target: &Target, name: &str) -> Result<()> {
    if !target.client.editable() {
        return Err(CoreError::Config(format!(
            "Remove the `{name}` entry from {} manually.",
            target.config.display()
        )));
    }
    if !target.config.exists() {
        return Ok(());
    }
    if target.client == Client::Codex {
        let mut doc = read_toml(&target.config).map_err(CoreError::Config)?;
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) {
            servers.remove(name);
        }
        return write_config(&target.config, &doc.to_string());
    }
    let Some(mut map) = read_json(&target.config).map_err(CoreError::Config)? else {
        return Ok(());
    };
    if let Some(Value::Object(servers)) = map.get_mut(servers_key(target.client)) {
        servers.remove(name);
    }
    let text = serde_json::to_string_pretty(&Value::Object(map))
        .map_err(|e| CoreError::Config(e.to_string()))?;
    write_config(&target.config, &format!("{text}\n"))
}

fn yaml_list(items: &[String]) -> String {
    let quoted: Vec<String> = items.iter().map(|a| format!("{a:?}")).collect();
    format!("[{}]", quoted.join(", "))
}

/// What to paste by hand when the file can't be edited safely.
pub fn snippet(target: &Target, spec: &Spec) -> String {
    let name = &spec.name;
    let (cmd, args, _) = spec.as_stdio();
    match target.client {
        Client::Goose => {
            return format!(
                "extensions:\n  {name}:\n    type: stdio\n    name: {name}\n    cmd: {cmd}\n    args: {}\n    enabled: true\n    envs: {{}}\n    timeout: 300\n",
                yaml_list(&args)
            );
        }
        Client::Continue => {
            return format!(
                "mcpServers:\n  - name: {name}\n    type: stdio\n    command: {cmd}\n    args: {}\n",
                yaml_list(&args)
            );
        }
        Client::Crush => {
            let mut line = format!("mcp add {name} --command {cmd}");
            for a in &args {
                line.push_str(&format!(" --arg {a}"));
            }
            return format!("{line}\n");
        }
        _ => {}
    }
    if target.client == Client::Codex {
        let mut doc = toml_edit::DocumentMut::new();
        let mut servers = toml_edit::Table::new();
        servers.set_implicit(true);
        servers[name.as_str()] = toml_edit::Item::Table(codex_table(spec));
        doc["mcp_servers"] = toml_edit::Item::Table(servers);
        return doc.to_string().trim_end().to_string();
    }
    let (key, value) = entry(target.client, spec);
    let wrapped = json!({ key: { name.as_str(): value } });
    serde_json::to_string_pretty(&wrapped).unwrap_or_default()
}

/// A client CLI call, with the environment it must run in (`None` value =
/// remove the variable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCall {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, Option<PathBuf>)>,
}

pub(crate) fn cli_call(target: &Target, cli: &Path, args: Vec<String>) -> Option<CliCall> {
    let var = target.client.profile_var()?;
    Some(CliCall {
        program: cli.to_path_buf(),
        args,
        env: vec![(var.to_string(), target.profile_dir.clone())],
    })
}

pub fn cli_add(target: &Target, spec: &Spec, cli: &Path) -> Option<CliCall> {
    let name = spec.name.clone();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let args: Vec<String> = match (target.client, &spec.transport) {
        (Client::ClaudeCode, Transport::Stdio { command, args, env }) => {
            let mut v = s(&["mcp", "add", "--scope", "user"]);
            v.push(name);
            for (k, val) in env {
                v.extend(["-e".into(), format!("{k}={val}")]);
            }
            v.push("--".into());
            v.push(command.clone());
            v.extend(args.iter().cloned());
            v
        }
        (Client::ClaudeCode, Transport::Http { url, headers }) => {
            let mut v = s(&["mcp", "add", "--scope", "user", "--transport", "http"]);
            v.extend([name, url.clone()]);
            for (k, val) in headers {
                v.extend(["--header".into(), format!("{k}: {val}")]);
            }
            v
        }
        (Client::Codex, Transport::Stdio { command, args, env }) => {
            let mut v = s(&["mcp", "add"]);
            v.push(name);
            for (k, val) in env {
                v.extend(["--env".into(), format!("{k}={val}")]);
            }
            v.push("--".into());
            v.push(command.clone());
            v.extend(args.iter().cloned());
            v
        }
        (Client::Codex, Transport::Http { url, headers }) => {
            let mut v = s(&["mcp", "add"]);
            v.extend([name, "--url".into(), url.clone()]);
            match spec.bearer_env() {
                Some(var) => v.extend(["--bearer-token-env-var".into(), var.into()]),
                // Other headers: only the config file can hold them.
                None if !headers.is_empty() => return None,
                None => {}
            }
            v
        }
        _ => return None,
    };
    cli_call(target, cli, args)
}

pub fn cli_remove(target: &Target, cli: &Path, name: &str) -> Option<CliCall> {
    let args: Vec<&str> = match target.client {
        Client::ClaudeCode => vec!["mcp", "remove", "--scope", "user", name],
        Client::Codex => vec!["mcp", "remove", name],
        _ => return None,
    };
    cli_call(target, cli, args.into_iter().map(String::from).collect())
}

pub(crate) async fn run_cli(
    call: &CliCall,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new(&call.program);
    cmd.args(&call.args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    for (k, v) in &call.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| CoreError::Config(format!("The client's CLI didn't finish within {} seconds. Try again, or add the server manually.", timeout.as_secs())))??;
    Ok(out)
}

const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

fn cli_error(target: &Target, out: &std::process::Output) -> CoreError {
    CoreError::Command {
        program: target.client.cli().unwrap_or_default().into(),
        code: out.status.code(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    }
}

/// Installs (or re-points) the server for `target`: through the client's
/// CLI when `cli` is given and supported, else by editing the file.
pub async fn install(target: &Target, spec: &Spec, cli: Option<&Path>) -> Result<()> {
    if let Some(cli) = cli
        && let Some(add) = cli_add(target, spec, cli)
    {
        if status(target, spec) != Status::NotInstalled
            && let Some(remove) = cli_remove(target, cli, &spec.name)
        {
            run_cli(&remove, CLI_TIMEOUT).await?;
        }
        let out = run_cli(&add, CLI_TIMEOUT).await?;
        if !out.status.success() {
            return Err(cli_error(target, &out));
        }
        if spec.name == SERVER_NAME
            && has_legacy(target)
            && let Some(remove) = cli_remove(target, cli, LEGACY_SERVER_NAME)
        {
            // Best effort: the new entry is already in place.
            let _ = run_cli(&remove, CLI_TIMEOUT).await;
        }
        return Ok(());
    }
    install_file(target, spec)
}

/// Whether the config still has the pre-rename (PG Desk) entry.
fn has_legacy(target: &Target) -> bool {
    if target.client == Client::Codex {
        return read_toml(&target.config).is_ok_and(|doc| {
            doc.get("mcp_servers")
                .and_then(|t| t.get(LEGACY_SERVER_NAME))
                .is_some()
        });
    }
    read_json(&target.config).is_ok_and(|map| {
        map.as_ref()
            .and_then(|m| m.get(servers_key(target.client)))
            .and_then(|s| s.get(LEGACY_SERVER_NAME))
            .is_some()
    })
}

pub async fn uninstall(target: &Target, name: &str, cli: Option<&Path>) -> Result<()> {
    if let Some(cli) = cli
        && let Some(remove) = cli_remove(target, cli, name)
    {
        let out = run_cli(&remove, CLI_TIMEOUT).await?;
        if !out.status.success() {
            return Err(cli_error(target, &out));
        }
        return Ok(());
    }
    uninstall_file(target, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn home() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let h = d.path();
        fs::create_dir_all(h.join(".claude")).unwrap();
        fs::write(
            h.join(".claude.json"),
            r#"{"numStartups": 3, "projects": {}}"#,
        )
        .unwrap();
        for p in ["work", "personal"] {
            fs::create_dir_all(h.join(format!(".claude-{p}"))).unwrap();
            fs::write(h.join(format!(".claude-{p}/.claude.json")), "{}").unwrap();
        }
        // A helper folder, not a profile.
        fs::create_dir_all(h.join(".claude-pool")).unwrap();
        fs::create_dir_all(h.join(".codex")).unwrap();
        fs::create_dir_all(h.join("Library/Application Support/Claude")).unwrap();
        fs::create_dir_all(h.join(".config/zed")).unwrap();
        d
    }

    fn find<'a>(targets: &'a [Target], client: Client, profile: &str) -> &'a Target {
        targets
            .iter()
            .find(|t| t.client == client && t.profile == profile)
            .unwrap_or_else(|| panic!("{client:?}/{profile} not found in {targets:#?}"))
    }

    #[test]
    fn discovers_every_claude_profile_and_installed_clients() {
        let h = home();
        let targets = discover(h.path(), None);
        let default = find(&targets, Client::ClaudeCode, "default");
        assert_eq!(default.config, h.path().join(".claude.json"));
        assert_eq!(default.profile_dir, None);
        let work = find(&targets, Client::ClaudeCode, "work");
        assert_eq!(work.config, h.path().join(".claude-work/.claude.json"));
        assert_eq!(
            work.profile_dir.as_deref(),
            Some(h.path().join(".claude-work").as_path())
        );
        find(&targets, Client::ClaudeCode, "personal");
        assert!(!targets.iter().any(|t| t.profile == "pool"));
        find(&targets, Client::Codex, "default");
        find(&targets, Client::ClaudeDesktop, "default");
        find(&targets, Client::Zed, "default");
        // Clients that aren't there aren't offered.
        assert!(!targets.iter().any(|t| t.client == Client::Cursor));
    }

    #[test]
    fn a_claude_config_dir_outside_home_is_included() {
        let h = home();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::write(elsewhere.path().join(".claude.json"), "{}").unwrap();
        let targets = discover(h.path(), Some(elsewhere.path().to_path_buf()));
        let t = targets
            .iter()
            .find(|t| t.config == elsewhere.path().join(".claude.json"))
            .expect("CLAUDE_CONFIG_DIR profile");
        assert_eq!(t.client, Client::ClaudeCode);
        // The same folder via the env var is not listed twice.
        let again = discover(h.path(), Some(h.path().join(".claude-work")));
        assert_eq!(again.iter().filter(|t| t.profile == "work").count(), 1);
    }

    #[test]
    fn json_install_keeps_other_settings_and_order() {
        let h = home();
        let targets = discover(h.path(), None);
        let t = find(&targets, Client::ClaudeCode, "default");
        let server = Path::new("/Apps/Sawit.app/Contents/MacOS/sawit-mcp");
        assert_eq!(status(t, &Spec::sawit(server)), Status::NotInstalled);
        install_file(t, &Spec::sawit(server)).unwrap();
        let text = fs::read_to_string(&t.config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert_eq!(
            v["mcpServers"]["sawit"]["command"],
            server.display().to_string()
        );
        assert_eq!(v["mcpServers"]["sawit"]["type"], "stdio");
        // Original key order kept, new key appended.
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["numStartups", "projects", "mcpServers"]);
        assert_eq!(status(t, &Spec::sawit(server)), Status::Installed);
        // Backed up once, before the first change.
        let backup = fs::read_to_string(backup_path(&t.config)).unwrap();
        assert_eq!(backup, r#"{"numStartups": 3, "projects": {}}"#);
        install_file(t, &Spec::sawit(server)).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&t.config)).unwrap(), backup);

        // A different binary is reported, then replaced.
        let other = Path::new("/old/sawit-mcp");
        assert_eq!(
            status(t, &Spec::sawit(other)),
            Status::Outdated(server.display().to_string())
        );

        uninstall_file(t, SERVER_NAME).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&t.config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("sawit").is_none());
        assert_eq!(v["numStartups"], 3);
        assert_eq!(status(t, &Spec::sawit(server)), Status::NotInstalled);
    }

    #[test]
    fn each_client_gets_its_own_entry_shape() {
        let server = "/bin/sawit-mcp";
        let shape = |c| entry(c, &Spec::sawit(Path::new(server)));
        assert_eq!(shape(Client::ClaudeDesktop).0, "mcpServers");
        assert_eq!(shape(Client::Cursor).1["command"], server);
        assert_eq!(shape(Client::Zed).0, "context_servers");
        assert_eq!(shape(Client::OpenCode).0, "mcp");
        assert_eq!(shape(Client::OpenCode).1["command"][0], server);
        assert_eq!(shape(Client::OpenCode).1["type"], "local");
        assert_eq!(shape(Client::VsCode).0, "servers");
    }

    #[test]
    fn missing_config_files_are_created() {
        let h = home();
        fs::create_dir_all(h.path().join(".cursor")).unwrap();
        let targets = discover(h.path(), None);
        let t = find(&targets, Client::Cursor, "default");
        assert!(!t.config.exists());
        install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
        assert_eq!(
            status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
            Status::Installed
        );
    }

    #[test]
    fn install_replaces_the_pre_rename_entry() {
        let h = home();
        fs::create_dir_all(h.path().join(".cursor")).unwrap();
        fs::write(
            h.path().join(".cursor/mcp.json"),
            r#"{"mcpServers": {"pgdesk": {"command": "/old/pgdesk-mcp"}, "other": {}}}"#,
        )
        .unwrap();
        fs::write(
            h.path().join(".codex/config.toml"),
            "[mcp_servers.pgdesk]\ncommand = \"/old/pgdesk-mcp\"\n",
        )
        .unwrap();
        let targets = discover(h.path(), None);
        for client in [Client::Cursor, Client::Codex] {
            let t = find(&targets, client, "default");
            assert!(has_legacy(t));
            install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
            assert!(!has_legacy(t));
            assert_eq!(
                status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
                Status::Installed
            );
        }
        let text = fs::read_to_string(h.path().join(".cursor/mcp.json")).unwrap();
        assert!(text.contains("\"other\""), "{text}");
    }

    #[test]
    fn codex_toml_keeps_comments_and_other_servers() {
        let h = home();
        let config = h.path().join(".codex/config.toml");
        fs::write(
            &config,
            "# my model\nmodel = \"o3\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();
        let targets = discover(h.path(), None);
        let t = find(&targets, Client::Codex, "default");
        install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# my model\nmodel = \"o3\""), "{text}");
        assert!(text.contains("[mcp_servers.other]"));
        assert!(
            text.contains("[mcp_servers.sawit]\ncommand = \"/bin/sawit-mcp\""),
            "{text}"
        );
        assert_eq!(
            status(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))),
            Status::Installed
        );
        uninstall_file(t, SERVER_NAME).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert!(!text.contains("sawit"), "{text}");
        assert!(text.contains("[mcp_servers.other]"));
    }

    #[test]
    fn commented_json_is_left_alone() {
        let h = home();
        let config = h.path().join(".config/zed/settings.json");
        let original = "// Zed settings\n{\n  \"theme\": \"One Dark\", // mine\n}\n";
        fs::write(&config, original).unwrap();
        let targets = discover(h.path(), None);
        let t = find(&targets, Client::Zed, "default");
        assert!(matches!(
            status(t, &Spec::sawit(Path::new("/bin/x"))),
            Status::Unreadable(_)
        ));
        let err = install_file(t, &Spec::sawit(Path::new("/bin/x"))).unwrap_err();
        assert!(err.to_string().contains("manual"), "{err}");
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert!(snippet(t, &Spec::sawit(Path::new("/bin/x"))).contains("context_servers"));
    }

    #[test]
    fn cli_targets_the_right_profile() {
        let h = home();
        let targets = discover(h.path(), None);
        let server = Path::new("/bin/sawit-mcp");
        let default = find(&targets, Client::ClaudeCode, "default");
        let inv = cli_add(default, &Spec::sawit(server), Path::new("/usr/bin/claude")).unwrap();
        assert_eq!(
            inv.args,
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "sawit",
                "--",
                "/bin/sawit-mcp"
            ]
        );
        // The default profile must not inherit the app's CLAUDE_CONFIG_DIR.
        assert_eq!(inv.env, vec![("CLAUDE_CONFIG_DIR".to_string(), None)]);
        let work = find(&targets, Client::ClaudeCode, "work");
        let inv = cli_add(work, &Spec::sawit(server), Path::new("/usr/bin/claude")).unwrap();
        assert_eq!(
            inv.env,
            vec![(
                "CLAUDE_CONFIG_DIR".to_string(),
                Some(h.path().join(".claude-work"))
            )]
        );
        let codex = find(&targets, Client::Codex, "default");
        let inv = cli_add(codex, &Spec::sawit(server), Path::new("/usr/bin/codex")).unwrap();
        assert_eq!(inv.args, ["mcp", "add", "sawit", "--", "/bin/sawit-mcp"]);
        assert_eq!(inv.env, vec![("CODEX_HOME".to_string(), None)]);
        // Clients without a CLI path aren't driven by one.
        let desktop = find(&targets, Client::ClaudeDesktop, "default");
        assert!(cli_add(desktop, &Spec::sawit(server), Path::new("/x")).is_none());
    }

    #[tokio::test]
    async fn cli_runs_with_the_profile_env_and_never_inherits_it() {
        use std::os::unix::fs::PermissionsExt;
        let h = home();
        let log = h.path().join("calls.log");
        let cli = h.path().join("claude");
        fs::write(
            &cli,
            format!(
                "#!/bin/sh\necho \"${{CLAUDE_CONFIG_DIR:-unset}} $*\" >> {}\n",
                log.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        let targets = discover(h.path(), None);
        let server = Path::new("/bin/sawit-mcp");
        // SAFETY: test-only; mimics the app launched from a profile shell.
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", "/somewhere/else") };
        install(
            find(&targets, Client::ClaudeCode, "default"),
            &Spec::sawit(server),
            Some(&cli),
        )
        .await
        .unwrap();
        install(
            find(&targets, Client::ClaudeCode, "work"),
            &Spec::sawit(server),
            Some(&cli),
        )
        .await
        .unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<_> = calls.lines().collect();
        assert_eq!(
            lines[0],
            "unset mcp add --scope user sawit -- /bin/sawit-mcp"
        );
        assert_eq!(
            lines[1],
            format!(
                "{} mcp add --scope user sawit -- /bin/sawit-mcp",
                h.path().join(".claude-work").display()
            )
        );
    }

    #[test]
    fn popular_agents_and_editors_are_detected() {
        let h = home();
        let p = h.path();
        for dir in [
            ".gemini",
            ".qwen",
            ".config/opencode",
            ".config/amp",
            ".copilot",
            ".factory",
            ".kiro",
            ".junie",
            ".augment",
            ".warp",
            ".lmstudio",
            ".config/kilo",
            ".config/devin",
            ".gemini/config",
            ".cline/data/settings",
            ".config/goose",
            ".continue",
            ".config/crush",
            "Library/Application Support/Code - Insiders/User",
        ] {
            fs::create_dir_all(p.join(dir)).unwrap();
        }
        let targets = discover(p, None);
        let config = |c: Client| {
            targets
                .iter()
                .find(|t| t.client == c)
                .unwrap_or_else(|| panic!("{c:?} not detected"))
                .config
                .strip_prefix(p)
                .unwrap()
                .to_path_buf()
        };
        assert_eq!(config(Client::Qwen), Path::new(".qwen/settings.json"));
        assert_eq!(config(Client::Amp), Path::new(".config/amp/settings.json"));
        assert_eq!(
            config(Client::CopilotCli),
            Path::new(".copilot/mcp-config.json")
        );
        assert_eq!(config(Client::FactoryDroid), Path::new(".factory/mcp.json"));
        assert_eq!(config(Client::Kiro), Path::new(".kiro/settings/mcp.json"));
        assert_eq!(config(Client::Junie), Path::new(".junie/mcp/mcp.json"));
        assert_eq!(config(Client::Auggie), Path::new(".augment/settings.json"));
        assert_eq!(config(Client::Warp), Path::new(".warp/.mcp.json"));
        assert_eq!(config(Client::LmStudio), Path::new(".lmstudio/mcp.json"));
        assert_eq!(
            config(Client::KiloCode),
            Path::new(".config/kilo/kilo.json")
        );
        assert_eq!(
            config(Client::Windsurf),
            Path::new(".config/devin/mcp_config.json")
        );
        assert_eq!(
            config(Client::Antigravity),
            Path::new(".gemini/config/mcp_config.json")
        );
        assert_eq!(
            config(Client::Cline),
            Path::new(".cline/data/settings/cline_mcp_settings.json")
        );
        assert_eq!(
            config(Client::Goose),
            Path::new(".config/goose/config.yaml")
        );
        assert_eq!(config(Client::Continue), Path::new(".continue/config.yaml"));
        assert_eq!(config(Client::Crush), Path::new(".config/crush/crushrc"));
        assert_eq!(
            config(Client::VsCodeInsiders),
            Path::new("Library/Application Support/Code - Insiders/User/mcp.json")
        );
    }

    #[test]
    fn unverified_locations_need_the_file_itself() {
        let h = home();
        let p = h.path();
        let trae = p.join("Library/Application Support/Trae/User");
        fs::create_dir_all(&trae).unwrap();
        let roo = p.join(
            "Library/Application Support/Code/User/globalStorage/rooveterinaryinc.roo-cline/settings",
        );
        fs::create_dir_all(&roo).unwrap();
        fs::create_dir_all(p.join(".aws/amazonq")).unwrap();
        // Folders alone: not offered (we'd be guessing a file name).
        let targets = discover(p, None);
        for c in [Client::Trae, Client::RooCode, Client::AmazonQ] {
            assert!(!targets.iter().any(|t| t.client == c), "{c:?}");
        }
        fs::write(trae.join("mcp.json"), "{}").unwrap();
        fs::write(roo.join("mcp_settings.json"), "{}").unwrap();
        fs::write(p.join(".aws/amazonq/mcp.json"), "{}").unwrap();
        let targets = discover(p, None);
        for c in [Client::Trae, Client::RooCode, Client::AmazonQ] {
            assert!(targets.iter().any(|t| t.client == c), "{c:?}");
        }
    }

    #[test]
    fn legacy_locations_are_offered_when_their_file_exists() {
        let h = home();
        let p = h.path();
        fs::create_dir_all(p.join(".codeium/windsurf")).unwrap();
        fs::write(p.join(".codeium/windsurf/mcp_config.json"), "{}").unwrap();
        let targets = discover(p, None);
        let windsurf: Vec<_> = targets
            .iter()
            .filter(|t| t.client == Client::Windsurf)
            .collect();
        assert_eq!(windsurf.len(), 1);
        assert!(
            windsurf[0]
                .config
                .ends_with(".codeium/windsurf/mcp_config.json")
        );
    }

    #[test]
    fn agent_specific_entry_shapes() {
        let s = Path::new("/bin/sawit-mcp");
        assert_eq!(entry(Client::Amp, &Spec::sawit(s)).0, "amp.mcpServers");
        let copilot = entry(Client::CopilotCli, &Spec::sawit(s)).1;
        assert_eq!(copilot["type"], "local");
        assert_eq!(copilot["tools"][0], "*");
        assert_eq!(
            entry(Client::FactoryDroid, &Spec::sawit(s)).1["type"],
            "stdio"
        );
        assert_eq!(entry(Client::Kiro, &Spec::sawit(s)).1["disabled"], false);
        assert_eq!(entry(Client::KiloCode, &Spec::sawit(s)).0, "mcp");
        assert_eq!(
            entry(Client::KiloCode, &Spec::sawit(s)).1["command"][0],
            "/bin/sawit-mcp"
        );
        assert_eq!(entry(Client::VsCodeInsiders, &Spec::sawit(s)).0, "servers");
        assert_eq!(entry(Client::Cursor, &Spec::sawit(s)).1["type"], "stdio");
        assert_eq!(
            entry(Client::RooCode, &Spec::sawit(s)).1["alwaysAllow"],
            serde_json::json!([])
        );
    }

    #[test]
    fn amp_uses_a_flat_dotted_key() {
        let h = home();
        fs::create_dir_all(h.path().join(".config/amp")).unwrap();
        let targets = discover(h.path(), None);
        let t = find(&targets, Client::Amp, "default");
        install_file(t, &Spec::sawit(Path::new("/bin/sawit-mcp"))).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&t.config).unwrap()).unwrap();
        assert_eq!(v["amp.mcpServers"]["sawit"]["command"], "/bin/sawit-mcp");
        assert!(v.get("amp").is_none());
    }

    #[test]
    fn yaml_and_script_configs_are_manual() {
        let h = home();
        let p = h.path();
        fs::create_dir_all(p.join(".config/goose")).unwrap();
        fs::write(p.join(".config/goose/config.yaml"), "GOOSE_MODEL: x\n").unwrap();
        fs::create_dir_all(p.join(".continue")).unwrap();
        let targets = discover(p, None);
        let server = Path::new("/bin/sawit-mcp");
        let goose = find(&targets, Client::Goose, "default");
        assert!(!goose.client.editable());
        assert!(matches!(
            status(goose, &Spec::sawit(server)),
            Status::Manual
        ));
        assert!(install_file(goose, &Spec::sawit(server)).is_err());
        assert_eq!(
            fs::read_to_string(&goose.config).unwrap(),
            "GOOSE_MODEL: x\n"
        );
        assert!(snippet(goose, &Spec::sawit(server)).contains("cmd: /bin/sawit-mcp"));
        // Pasted in by hand: reported as installed.
        fs::write(&goose.config, snippet(goose, &Spec::sawit(server))).unwrap();
        assert_eq!(status(goose, &Spec::sawit(server)), Status::Installed);
        let cont = find(&targets, Client::Continue, "default");
        assert!(snippet(cont, &Spec::sawit(server)).contains("- name: sawit"));
    }

    #[test]
    fn every_client_has_a_category() {
        for c in Client::ALL {
            let _ = c.category();
            assert!(!c.name().is_empty());
        }
        assert_eq!(Client::ClaudeCode.category(), Category::Agent);
        assert_eq!(Client::Cursor.category(), Category::Editor);
    }

    #[test]
    fn remote_servers_use_each_clients_format_or_the_bridge() {
        let spec = Spec::http("mobbin", "https://api.mobbin.com/mcp").with_npx("/opt/bin/npx");
        let url = "https://api.mobbin.com/mcp";
        assert_eq!(
            entry(Client::ClaudeCode, &spec).1,
            json!({"type": "http", "url": url})
        );
        assert_eq!(entry(Client::Gemini, &spec).1["httpUrl"], url);
        assert_eq!(entry(Client::Windsurf, &spec).1["serverUrl"], url);
        assert_eq!(entry(Client::OpenCode, &spec).1["type"], "remote");
        assert_eq!(
            entry(Client::Zed, &spec),
            ("context_servers", json!({"url": url}))
        );
        // Claude Desktop only runs programs: the mcp-remote bridge.
        assert_eq!(
            entry(Client::ClaudeDesktop, &spec).1,
            json!({"command": "/opt/bin/npx", "args": ["-y", "mcp-remote", url]})
        );
        let keyed =
            Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
        assert_eq!(
            entry(Client::Cursor, &keyed).1["headers"]["Authorization"],
            "Bearer ${ES_KEY}"
        );
        assert_eq!(
            entry(Client::ClaudeDesktop, &keyed).1["args"],
            json!([
                "-y",
                "mcp-remote",
                "https://es.dev/mcp",
                "--header",
                "Authorization:Bearer ${ES_KEY}"
            ])
        );
    }

    #[test]
    fn remote_servers_install_and_report_status() {
        let h = home();
        let targets = discover(h.path(), None);
        let spec = Spec::http("aws", "https://aws-mcp.us-east-1.api.aws/mcp");
        for client in [Client::ClaudeCode, Client::ClaudeDesktop, Client::Codex] {
            let t = find(&targets, client, "default");
            assert_eq!(status(t, &spec), Status::NotInstalled);
            install_file(t, &spec).unwrap();
            assert_eq!(status(t, &spec), Status::Installed, "{client:?}");
            // Another region is the same server pointing elsewhere.
            let other = Spec::http("aws", "https://aws-mcp.eu-west-1.api.aws/mcp");
            assert!(
                matches!(status(t, &other), Status::Outdated(_)),
                "{client:?}"
            );
            uninstall_file(t, "aws").unwrap();
            assert_eq!(status(t, &spec), Status::NotInstalled);
        }
        let codex = find(&targets, Client::Codex, "default");
        let keyed =
            Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
        install_file(codex, &keyed).unwrap();
        let text = fs::read_to_string(&codex.config).unwrap();
        assert!(text.contains("url = \"https://es.dev/mcp\""), "{text}");
        assert!(text.contains("bearer_token_env_var = \"ES_KEY\""), "{text}");
        // A local server added by hand with a bare `npx` is still recognised.
        let claude = find(&targets, Client::ClaudeCode, "default");
        install_file(
            claude,
            &Spec::stdio(
                "rea",
                "npx",
                vec!["-y".into(), "rea-agents@6.3.0".into(), "mcp".into()],
            ),
        )
        .unwrap();
        let resolved = Spec::stdio(
            "rea",
            "/opt/homebrew/bin/npx",
            vec!["-y".into(), "rea-agents@6.3.0".into(), "mcp".into()],
        );
        assert_eq!(status(claude, &resolved), Status::Installed);
    }

    #[test]
    fn remote_servers_go_through_the_cli() {
        let h = home();
        let targets = discover(h.path(), None);
        let spec =
            Spec::http("es", "https://es.dev/mcp").header("Authorization", "Bearer ${ES_KEY}");
        let claude = find(&targets, Client::ClaudeCode, "work");
        let call = cli_add(claude, &spec, Path::new("/bin/claude")).unwrap();
        assert_eq!(
            call.args,
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "--transport",
                "http",
                "es",
                "https://es.dev/mcp",
                "--header",
                "Authorization: Bearer ${ES_KEY}"
            ]
        );
        let codex = find(&targets, Client::Codex, "default");
        let call = cli_add(codex, &spec, Path::new("/bin/codex")).unwrap();
        assert_eq!(
            call.args,
            [
                "mcp",
                "add",
                "es",
                "--url",
                "https://es.dev/mcp",
                "--bearer-token-env-var",
                "ES_KEY"
            ]
        );
        // Codex's CLI can't take other headers: the file is edited instead.
        let custom = Spec::http("x", "https://x.dev/mcp").header("X-Team", "a");
        assert!(cli_add(codex, &custom, Path::new("/bin/codex")).is_none());
    }
}
