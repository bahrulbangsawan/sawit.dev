//! Installing the `pgdesk-mcp` server into AI clients: Claude Code (every
//! profile — the default `~/.claude.json` and each `CLAUDE_CONFIG_DIR`
//! folder such as `~/.claude-work`), Claude Desktop, Codex (and `CODEX_HOME`
//! profiles), Cursor, Windsurf, Gemini CLI, Zed, opencode and VS Code.
//!
//! Claude Code and Codex are changed through their own CLI when it is
//! installed (they rewrite their config files while running); everything
//! else, and those two without a CLI, by editing the config file in place —
//! other settings, key order and TOML comments are kept, and the original is
//! backed up once next to it.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::files::write_text_atomic;
use crate::{CoreError, Result};

/// The server's name in every client's config.
pub const SERVER_NAME: &str = "pgdesk";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Codex,
    Cursor,
    Windsurf,
    Gemini,
    Zed,
    OpenCode,
    VsCode,
}

impl Client {
    pub fn name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
            Client::Windsurf => "Windsurf",
            Client::Gemini => "Gemini CLI",
            Client::Zed => "Zed",
            Client::OpenCode => "opencode",
            Client::VsCode => "VS Code",
        }
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
    let simple: [(Client, &str, &str); 6] = [
        (Client::Cursor, ".cursor", "mcp.json"),
        (Client::Windsurf, ".codeium/windsurf", "mcp_config.json"),
        (Client::Gemini, ".gemini", "settings.json"),
        (Client::Zed, ".config/zed", "settings.json"),
        (Client::OpenCode, ".config/opencode", "opencode.json"),
        (
            Client::VsCode,
            "Library/Application Support/Code/User",
            "mcp.json",
        ),
    ];
    for (client, dir, file) in simple {
        let dir = home.join(dir);
        if dir.is_dir() {
            out.push(target(client, "default", dir.join(file), None));
        }
    }
    out
}

/// Where the server entry lives in a JSON client config, and its value.
pub fn entry(client: Client, server: &Path) -> (&'static str, Value) {
    let cmd = server.display().to_string();
    match client {
        Client::ClaudeCode => (
            "mcpServers",
            json!({"type": "stdio", "command": cmd, "args": [], "env": {}}),
        ),
        Client::Zed => (
            "context_servers",
            json!({"command": cmd, "args": [], "env": {}}),
        ),
        Client::OpenCode => (
            "mcp",
            json!({"type": "local", "command": [cmd], "enabled": true}),
        ),
        Client::VsCode => (
            "servers",
            json!({"type": "stdio", "command": cmd, "args": []}),
        ),
        _ => ("mcpServers", json!({"command": cmd, "args": []})),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Installed,
    /// Installed, but pointing at another binary (e.g. an older build).
    Outdated(String),
    NotInstalled,
    /// The config can't be read safely (comments, syntax errors).
    Unreadable(String),
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
        Ok(_) => Err("bukan objek JSON".into()),
        Err(e) => Err(format!("{e} (mis. ada komentar)")),
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

/// Whether `server` is installed for `target`.
pub fn status(target: &Target, server: &Path) -> Status {
    let wanted = server.display().to_string();
    let found: Option<String> = if target.client == Client::Codex {
        match read_toml(&target.config) {
            Ok(doc) => doc
                .get("mcp_servers")
                .and_then(|t| t.get(SERVER_NAME))
                .map(|s| {
                    s.get("command")
                        .and_then(|c| c.as_str())
                        .unwrap_or_default()
                        .to_string()
                }),
            Err(e) => return Status::Unreadable(e),
        }
    } else {
        let (key, _) = entry(target.client, server);
        match read_json(&target.config) {
            Ok(map) => map
                .as_ref()
                .and_then(|m| m.get(key))
                .and_then(|s| s.get(SERVER_NAME))
                .map(|s| match &s["command"] {
                    Value::String(c) => c.clone(),
                    Value::Array(a) => a
                        .first()
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    _ => String::new(),
                }),
            Err(e) => return Status::Unreadable(e),
        }
    };
    match found {
        None => Status::NotInstalled,
        Some(cmd) if cmd == wanted => Status::Installed,
        Some(cmd) => Status::Outdated(cmd),
    }
}

/// `<config>.pgdesk-backup`, written once before the first change.
pub fn backup_path(config: &Path) -> PathBuf {
    let mut name = config.file_name().unwrap_or_default().to_os_string();
    name.push(".pgdesk-backup");
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

fn manual(target: &Target, server: &Path, why: &str) -> CoreError {
    CoreError::Config(format!(
        "{} tidak bisa diubah otomatis ({why}). Tambahkan manual:\n{}",
        target.config.display(),
        snippet(target, server)
    ))
}

/// Adds or updates the server by editing the config file.
pub fn install_file(target: &Target, server: &Path) -> Result<()> {
    if target.client == Client::Codex {
        let mut doc = read_toml(&target.config).map_err(|e| manual(target, server, &e))?;
        let servers = doc
            .entry("mcp_servers")
            .or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            })
            .as_table_mut()
            .ok_or_else(|| manual(target, server, "mcp_servers bukan tabel"))?;
        let mut table = toml_edit::Table::new();
        table["command"] = toml_edit::value(server.display().to_string());
        table["args"] = toml_edit::value(toml_edit::Array::new());
        servers[SERVER_NAME] = toml_edit::Item::Table(table);
        return write_config(&target.config, &doc.to_string());
    }
    let mut map = read_json(&target.config)
        .map_err(|e| manual(target, server, &e))?
        .unwrap_or_default();
    let (key, value) = entry(target.client, server);
    let servers = map.entry(key).or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err(manual(target, server, &format!("{key} bukan objek")));
    };
    servers.insert(SERVER_NAME.into(), value);
    let text = serde_json::to_string_pretty(&Value::Object(map))
        .map_err(|e| CoreError::Config(e.to_string()))?;
    write_config(&target.config, &format!("{text}\n"))
}

/// Removes the server entry (other servers and settings stay).
pub fn uninstall_file(target: &Target) -> Result<()> {
    if !target.config.exists() {
        return Ok(());
    }
    if target.client == Client::Codex {
        let mut doc = read_toml(&target.config).map_err(CoreError::Config)?;
        if let Some(servers) = doc.get_mut("mcp_servers").and_then(|t| t.as_table_mut()) {
            servers.remove(SERVER_NAME);
        }
        return write_config(&target.config, &doc.to_string());
    }
    let Some(mut map) = read_json(&target.config).map_err(CoreError::Config)? else {
        return Ok(());
    };
    let (key, _) = entry(target.client, Path::new(""));
    if let Some(Value::Object(servers)) = map.get_mut(key) {
        servers.remove(SERVER_NAME);
    }
    let text = serde_json::to_string_pretty(&Value::Object(map))
        .map_err(|e| CoreError::Config(e.to_string()))?;
    write_config(&target.config, &format!("{text}\n"))
}

/// What to paste by hand when the file can't be edited safely.
pub fn snippet(target: &Target, server: &Path) -> String {
    if target.client == Client::Codex {
        return format!(
            "[mcp_servers.{SERVER_NAME}]\ncommand = \"{}\"\nargs = []",
            server.display()
        );
    }
    let (key, value) = entry(target.client, server);
    let wrapped = json!({ key: { SERVER_NAME: value } });
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

fn cli_call(target: &Target, cli: &Path, args: Vec<String>) -> Option<CliCall> {
    let var = target.client.profile_var()?;
    Some(CliCall {
        program: cli.to_path_buf(),
        args,
        env: vec![(var.to_string(), target.profile_dir.clone())],
    })
}

pub fn cli_add(target: &Target, server: &Path, cli: &Path) -> Option<CliCall> {
    let server = server.display().to_string();
    let args: Vec<&str> = match target.client {
        Client::ClaudeCode => vec!["mcp", "add", "--scope", "user", SERVER_NAME, "--", &server],
        Client::Codex => vec!["mcp", "add", SERVER_NAME, "--", &server],
        _ => return None,
    };
    cli_call(target, cli, args.into_iter().map(String::from).collect())
}

pub fn cli_remove(target: &Target, cli: &Path) -> Option<CliCall> {
    let args: Vec<&str> = match target.client {
        Client::ClaudeCode => vec!["mcp", "remove", "--scope", "user", SERVER_NAME],
        Client::Codex => vec!["mcp", "remove", SERVER_NAME],
        _ => return None,
    };
    cli_call(target, cli, args.into_iter().map(String::from).collect())
}

async fn run_cli(call: &CliCall) -> Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new(&call.program);
    cmd.args(&call.args).stdin(std::process::Stdio::null());
    for (k, v) in &call.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let out = tokio::time::timeout(std::time::Duration::from_secs(30), cmd.output())
        .await
        .map_err(|_| CoreError::Config("CLI tidak selesai dalam 30 detik".into()))??;
    Ok(out)
}

/// Installs (or re-points) the server for `target`: through the client's
/// CLI when `cli` is given and supported, else by editing the file.
pub async fn install(target: &Target, server: &Path, cli: Option<&Path>) -> Result<()> {
    if let Some(cli) = cli
        && let Some(add) = cli_add(target, server, cli)
    {
        if status(target, server) != Status::NotInstalled
            && let Some(remove) = cli_remove(target, cli)
        {
            run_cli(&remove).await?;
        }
        let out = run_cli(&add).await?;
        if !out.status.success() {
            return Err(CoreError::Command {
                program: target.client.cli().unwrap_or_default().into(),
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        return Ok(());
    }
    install_file(target, server)
}

pub async fn uninstall(target: &Target, cli: Option<&Path>) -> Result<()> {
    if let Some(cli) = cli
        && let Some(remove) = cli_remove(target, cli)
    {
        let out = run_cli(&remove).await?;
        if !out.status.success() {
            return Err(CoreError::Command {
                program: target.client.cli().unwrap_or_default().into(),
                code: out.status.code(),
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        return Ok(());
    }
    uninstall_file(target)
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
        let server = Path::new("/Apps/PG Desk.app/Contents/MacOS/pgdesk-mcp");
        assert_eq!(status(t, server), Status::NotInstalled);
        install_file(t, server).unwrap();
        let text = fs::read_to_string(&t.config).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["numStartups"], 3);
        assert_eq!(
            v["mcpServers"]["pgdesk"]["command"],
            server.display().to_string()
        );
        assert_eq!(v["mcpServers"]["pgdesk"]["type"], "stdio");
        // Original key order kept, new key appended.
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["numStartups", "projects", "mcpServers"]);
        assert_eq!(status(t, server), Status::Installed);
        // Backed up once, before the first change.
        let backup = fs::read_to_string(backup_path(&t.config)).unwrap();
        assert_eq!(backup, r#"{"numStartups": 3, "projects": {}}"#);
        install_file(t, server).unwrap();
        assert_eq!(fs::read_to_string(backup_path(&t.config)).unwrap(), backup);

        // A different binary is reported, then replaced.
        let other = Path::new("/old/pgdesk-mcp");
        assert_eq!(
            status(t, other),
            Status::Outdated(server.display().to_string())
        );

        uninstall_file(t).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&t.config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("pgdesk").is_none());
        assert_eq!(v["numStartups"], 3);
        assert_eq!(status(t, server), Status::NotInstalled);
    }

    #[test]
    fn each_client_gets_its_own_entry_shape() {
        let server = "/bin/pgdesk-mcp";
        let shape = |c| entry(c, Path::new(server));
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
        install_file(t, Path::new("/bin/pgdesk-mcp")).unwrap();
        assert_eq!(status(t, Path::new("/bin/pgdesk-mcp")), Status::Installed);
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
        install_file(t, Path::new("/bin/pgdesk-mcp")).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert!(text.starts_with("# my model\nmodel = \"o3\""), "{text}");
        assert!(text.contains("[mcp_servers.other]"));
        assert!(
            text.contains("[mcp_servers.pgdesk]\ncommand = \"/bin/pgdesk-mcp\""),
            "{text}"
        );
        assert_eq!(status(t, Path::new("/bin/pgdesk-mcp")), Status::Installed);
        uninstall_file(t).unwrap();
        let text = fs::read_to_string(&config).unwrap();
        assert!(!text.contains("pgdesk"), "{text}");
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
            status(t, Path::new("/bin/x")),
            Status::Unreadable(_)
        ));
        let err = install_file(t, Path::new("/bin/x")).unwrap_err();
        assert!(err.to_string().contains("manual"), "{err}");
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert!(snippet(t, Path::new("/bin/x")).contains("context_servers"));
    }

    #[test]
    fn cli_targets_the_right_profile() {
        let h = home();
        let targets = discover(h.path(), None);
        let server = Path::new("/bin/pgdesk-mcp");
        let default = find(&targets, Client::ClaudeCode, "default");
        let inv = cli_add(default, server, Path::new("/usr/bin/claude")).unwrap();
        assert_eq!(
            inv.args,
            [
                "mcp",
                "add",
                "--scope",
                "user",
                "pgdesk",
                "--",
                "/bin/pgdesk-mcp"
            ]
        );
        // The default profile must not inherit the app's CLAUDE_CONFIG_DIR.
        assert_eq!(inv.env, vec![("CLAUDE_CONFIG_DIR".to_string(), None)]);
        let work = find(&targets, Client::ClaudeCode, "work");
        let inv = cli_add(work, server, Path::new("/usr/bin/claude")).unwrap();
        assert_eq!(
            inv.env,
            vec![(
                "CLAUDE_CONFIG_DIR".to_string(),
                Some(h.path().join(".claude-work"))
            )]
        );
        let codex = find(&targets, Client::Codex, "default");
        let inv = cli_add(codex, server, Path::new("/usr/bin/codex")).unwrap();
        assert_eq!(inv.args, ["mcp", "add", "pgdesk", "--", "/bin/pgdesk-mcp"]);
        assert_eq!(inv.env, vec![("CODEX_HOME".to_string(), None)]);
        // Clients without a CLI path aren't driven by one.
        let desktop = find(&targets, Client::ClaudeDesktop, "default");
        assert!(cli_add(desktop, server, Path::new("/x")).is_none());
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
        let server = Path::new("/bin/pgdesk-mcp");
        // SAFETY: test-only; mimics the app launched from a profile shell.
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", "/somewhere/else") };
        install(find(&targets, Client::ClaudeCode, "default"), server, Some(&cli))
            .await
            .unwrap();
        install(find(&targets, Client::ClaudeCode, "work"), server, Some(&cli))
            .await
            .unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        let lines: Vec<_> = calls.lines().collect();
        assert_eq!(lines[0], "unset mcp add --scope user pgdesk -- /bin/pgdesk-mcp");
        assert_eq!(
            lines[1],
            format!(
                "{} mcp add --scope user pgdesk -- /bin/pgdesk-mcp",
                h.path().join(".claude-work").display()
            )
        );
    }
}
