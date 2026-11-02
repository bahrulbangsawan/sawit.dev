//! Installing and removing by editing the config file (copied beside it first), and the
//! snippet to paste where Sawit can't edit it.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::lock::lock_config;
use super::merge::{merge_entry, update_codex_table};
use super::read::{read_json, read_toml};
use super::shape::{codex_table, entry, servers_key};
use super::{Client, LEGACY_SERVER_NAME, SERVER_NAME, Spec, Target};
use crate::files::write_text_atomic;
use crate::{CoreError, Result};

/// `<config>.sawit-backup`: the file as it was before Sawit's last change to
/// it, written again before every change.
pub fn backup_path(config: &Path) -> PathBuf {
    let mut name = config.file_name().unwrap_or_default().to_os_string();
    name.push(".sawit-backup");
    config.with_file_name(name)
}

pub(super) fn write_config(config: &Path, text: &str) -> Result<()> {
    if let Some(dir) = config.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if config.exists() {
        std::fs::copy(config, backup_path(config))?;
    }
    write_text_atomic(config, text)
}

pub(super) fn manual(target: &Target, spec: &Spec, why: &str) -> CoreError {
    CoreError::Config(format!(
        "{} can't be changed automatically ({why}). Add it manually:\n{}",
        target.config.display(),
        snippet(target, spec)
    ))
}

/// Codex's `[mcp_servers]` table, made when the config has none.
pub(super) fn codex_servers(doc: &mut toml_edit::DocumentMut) -> Option<&mut toml_edit::Table> {
    doc.entry("mcp_servers")
        .or_insert_with(|| {
            let mut t = toml_edit::Table::new();
            t.set_implicit(true);
            toml_edit::Item::Table(t)
        })
        .as_table_mut()
}

/// Adds or updates the server by editing the config file. An update changes
/// only how the server is reached ([`merge_entry`], [`update_codex_table`]).
pub fn install_file(target: &Target, spec: &Spec) -> Result<()> {
    if !target.client.editable() {
        return Err(manual(target, spec, "YAML/script format"));
    }
    let _lock = lock_config(&target.config)?;
    install_file_locked(target, spec)
}

/// [`install_file`] while the config's lock is held.
pub(super) fn install_file_locked(target: &Target, spec: &Spec) -> Result<()> {
    if !target.client.editable() {
        return Err(manual(target, spec, "YAML/script format"));
    }
    let legacy = spec.name == SERVER_NAME;
    if target.client == Client::Codex {
        let mut doc = read_toml(&target.config).map_err(|e| manual(target, spec, &e))?;
        let servers = codex_servers(&mut doc)
            .ok_or_else(|| manual(target, spec, "mcp_servers is not a table"))?;
        match servers
            .get_mut(spec.name.as_str())
            .and_then(|i| i.as_table_like_mut())
        {
            Some(table) => update_codex_table(table, spec),
            None => {
                servers.insert(&spec.name, toml_edit::Item::Table(codex_table(spec)));
            }
        }
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
    let value = match servers.get(&spec.name) {
        Some(old) => merge_entry(old, value),
        None => value,
    };
    servers.insert(spec.name.clone(), value);
    if legacy {
        // Shifting, not swapping: the other servers keep their order.
        servers.shift_remove(LEGACY_SERVER_NAME);
    }
    write_json(&target.config, map)
}

/// Writes `map` as the config, indented as the file was (two spaces for a
/// new one).
pub(super) fn write_json(config: &Path, map: Map<String, Value>) -> Result<()> {
    use serde::Serialize as _;
    let text = std::fs::read_to_string(config).unwrap_or_default();
    let indent = json_indent(&text);
    let mut out = Vec::new();
    let mut writer = serde_json::Serializer::with_formatter(
        &mut out,
        serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes()),
    );
    Value::Object(map)
        .serialize(&mut writer)
        .map_err(|e| CoreError::Config(e.to_string()))?;
    out.push(b'\n');
    write_config(config, &String::from_utf8_lossy(&out))
}

/// The indent of a JSON file's first indented line: its own spaces or tabs.
fn json_indent(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let rest = line.trim_start_matches([' ', '\t']);
            (rest.len() < line.len() && !rest.is_empty())
                .then(|| line[..line.len() - rest.len()].to_string())
        })
        .unwrap_or_else(|| "  ".into())
}

/// Removes the server entry (other servers and settings stay).
pub fn uninstall_file(target: &Target, name: &str) -> Result<()> {
    let _lock = lock_config(&target.config)?;
    uninstall_file_locked(target, name)
}

/// [`uninstall_file`] while the config's lock is held.
pub(super) fn uninstall_file_locked(target: &Target, name: &str) -> Result<()> {
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
        servers.shift_remove(name);
    }
    write_json(&target.config, map)
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
