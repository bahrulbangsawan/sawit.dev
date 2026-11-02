//! Reading a client's config once: what each server entry in it points at, and whether a
//! server is installed there ([`Status`]).

use std::path::Path;

use serde_json::{Map, Value};

use super::shape::{HELPER_MARK, servers_key};
use super::{Client, Spec, Target};

/// Every command, argument, URL and header helper in a JSON entry, space-separated.
fn signature(v: &Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    for key in [
        "command",
        "args",
        "url",
        "httpUrl",
        "serverUrl",
        "headersHelper",
    ] {
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
    for key in ["command", "args", "url", "http_headers_helper"] {
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

pub(super) fn read_json(path: &Path) -> std::result::Result<Option<Map<String, Value>>, String> {
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

pub(super) fn read_toml(path: &Path) -> std::result::Result<toml_edit::DocumentMut, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.to_string()),
    };
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| toml_error(&e))
}

/// Where a config's TOML goes wrong, without the line itself: toml_edit
/// quotes the line it stopped at, and in a client's config that line can
/// hold an API key or a bearer token, which would reach the MCP page, an
/// agent's tool result and `sawit list mcp`.
fn toml_error(e: &toml_edit::TomlError) -> String {
    let text = e.to_string();
    match text.lines().next() {
        Some(first) if first.starts_with("TOML parse error at line ") => first.to_string(),
        _ => "TOML parse error".into(),
    }
}

/// A target's config, read once: what every server in it points at, so
/// the statuses of the whole catalog come from one read of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entries {
    /// A YAML or script config, searched as text.
    Manual(String),
    /// Each server's command, arguments and URL, space-separated, by name.
    Servers(std::collections::HashMap<String, String>),
    /// Why the config can't be read safely (comments, syntax errors).
    Unreadable(String),
}

/// Reads `target`'s config once: see [`status_in`].
pub fn read_entries(target: &Target) -> Entries {
    if !target.client.editable() {
        return Entries::Manual(std::fs::read_to_string(&target.config).unwrap_or_default());
    }
    if target.client == Client::Codex {
        return match read_toml(&target.config) {
            Ok(doc) => Entries::Servers(
                doc.get("mcp_servers")
                    .and_then(|t| t.as_table_like())
                    .map(|t| {
                        t.iter()
                            .map(|(name, item)| (name.to_string(), toml_signature(item)))
                            .collect()
                    })
                    .unwrap_or_default(),
            ),
            Err(e) => Entries::Unreadable(e),
        };
    }
    match read_json(&target.config) {
        Ok(map) => Entries::Servers(
            map.as_ref()
                .and_then(|m| m.get(servers_key(target.client)))
                .and_then(|s| s.as_object())
                .map(|servers| {
                    servers
                        .iter()
                        .map(|(name, v)| (name.clone(), signature(v)))
                        .collect()
                })
                .unwrap_or_default(),
        ),
        Err(e) => Entries::Unreadable(e),
    }
}

/// Whether `spec` is installed in a config already read.
pub fn status_in(entries: &Entries, spec: &Spec) -> Status {
    let wanted = spec.key();
    let found = match entries {
        Entries::Manual(text) => {
            return if text.contains(&spec.name) && text.contains(&wanted) {
                Status::Installed
            } else {
                Status::Manual
            };
        }
        Entries::Unreadable(e) => return Status::Unreadable(e.clone()),
        Entries::Servers(servers) => servers.get(&spec.name),
    };
    match found {
        None => Status::NotInstalled,
        // Handed the sign-in by another `sawit` (an older build, an app since moved), whose
        // helper may no longer be there: installing again points it at this one.
        Some(sig)
            if sig.contains(&wanted)
                && spec
                    .token_helper
                    .as_deref()
                    .is_some_and(|sawit| sig.contains(HELPER_MARK) && !sig.contains(sawit)) =>
        {
            Status::Outdated(sig.clone())
        }
        Some(sig) if sig.contains(&wanted) => Status::Installed,
        Some(sig) => Status::Outdated(sig.clone()),
    }
}

/// Whether `spec` is installed for `target`.
pub fn status(target: &Target, spec: &Spec) -> Status {
    status_in(&read_entries(target), spec)
}
