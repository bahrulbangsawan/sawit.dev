//! Installing and removing a server: through the client's CLI when it has one (the old entry
//! put back if the CLI fails or is stopped), else by editing the file.

use std::path::Path;

use serde_json::{Map, Value};

use super::cli::{CLI_TIMEOUT, cli_add, cli_error, cli_remove, run_cli};
use super::file::{
    codex_servers, install_file_locked, manual, uninstall_file_locked, write_config, write_json,
};
use super::lock::lock_config_async;
use super::merge::{merge_entry, update_codex_table};
use super::read::{Status, read_json, read_toml, status};
use super::shape::servers_key;
use super::{Client, LEGACY_SERVER_NAME, SERVER_NAME, Spec, Target};
use crate::{CoreError, Result};

/// A server's entry as the config had it before a CLI update.
#[derive(Debug, Clone)]
enum SavedEntry {
    Json(Value),
    Toml(toml_edit::Item),
}

fn saved_entry(target: &Target, name: &str) -> Option<SavedEntry> {
    if target.client == Client::Codex {
        let doc = read_toml(&target.config).ok()?;
        let item = doc.get("mcp_servers")?.get(name)?;
        return Some(SavedEntry::Toml(item.clone()));
    }
    let map = read_json(&target.config).ok()??;
    let value = map.get(servers_key(target.client))?.get(name)?;
    Some(SavedEntry::Json(value.clone()))
}

/// Writes `saved` back as `name`'s entry when the config has none: a CLI
/// update removes the entry before adding the new one, and when the add
/// fails or is stopped the old one must not be lost.
fn put_back(target: &Target, name: &str, saved: &SavedEntry) -> Result<()> {
    match saved {
        SavedEntry::Toml(item) => {
            let mut doc = read_toml(&target.config).map_err(CoreError::Config)?;
            let Some(servers) = codex_servers(&mut doc) else {
                return Ok(());
            };
            if servers.contains_key(name) {
                return Ok(());
            }
            servers.insert(name, item.clone());
            write_config(&target.config, &doc.to_string())
        }
        SavedEntry::Json(value) => {
            let mut map = read_json(&target.config)
                .map_err(CoreError::Config)?
                .unwrap_or_default();
            let servers = map
                .entry(servers_key(target.client))
                .or_insert_with(|| Value::Object(Map::new()));
            let Value::Object(servers) = servers else {
                return Ok(());
            };
            if servers.contains_key(name) {
                return Ok(());
            }
            servers.insert(name.to_string(), value.clone());
            write_json(&target.config, map)
        }
    }
}

/// After a CLI update: the user's own keys of the entry it replaced, laid
/// back over the one the CLI wrote, like an update by file does.
fn keep_user_keys(target: &Target, spec: &Spec, saved: &SavedEntry) -> Result<()> {
    match saved {
        SavedEntry::Toml(item) => {
            let mut doc = read_toml(&target.config).map_err(CoreError::Config)?;
            let Some(servers) = doc
                .get_mut("mcp_servers")
                .and_then(|t| t.as_table_like_mut())
            else {
                return Ok(());
            };
            if !servers.contains_key(&spec.name) {
                return Ok(());
            }
            let mut kept = item.clone();
            let Some(table) = kept.as_table_like_mut() else {
                return Ok(());
            };
            update_codex_table(table, spec);
            servers.insert(&spec.name, kept);
            write_config(&target.config, &doc.to_string())
        }
        SavedEntry::Json(old) => {
            let Some(mut map) = read_json(&target.config).map_err(CoreError::Config)? else {
                return Ok(());
            };
            let Some(Value::Object(servers)) = map.get_mut(servers_key(target.client)) else {
                return Ok(());
            };
            let Some(now) = servers.get(&spec.name) else {
                return Ok(());
            };
            let merged = merge_entry(old, now.clone());
            if &merged == now {
                return Ok(());
            }
            servers.insert(spec.name.clone(), merged);
            write_json(&target.config, map)
        }
    }
}

/// Puts the saved entry back when dropped armed: a CLI update that fails,
/// times out or is stopped (its future dropped) between `remove` and `add`.
struct PutBack<'a> {
    target: &'a Target,
    name: &'a str,
    saved: Option<SavedEntry>,
}

impl Drop for PutBack<'_> {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take() {
            let _ = put_back(self.target, self.name, &saved);
        }
    }
}

/// Installs (or re-points) the server for `target`: through the client's
/// CLI when `cli` is given and supported, else by editing the file. The CLI
/// can only remove an entry and add it again: the old entry comes back if
/// the add fails or is stopped, and the user's own keys of it are kept.
pub async fn install(target: &Target, spec: &Spec, cli: Option<&Path>) -> Result<()> {
    if !target.client.editable() {
        return Err(manual(target, spec, "YAML/script format"));
    }
    // Held to the end: the guard below, made after it, puts back first.
    let _lock = lock_config_async(&target.config).await?;
    if let Some(cli) = cli
        && let Some(add) = cli_add(target, spec, cli)
    {
        let saved = saved_entry(target, &spec.name);
        let mut guard = PutBack {
            target,
            name: &spec.name,
            saved: saved.clone(),
        };
        if status(target, spec) != Status::NotInstalled
            && let Some(remove) = cli_remove(target, cli, &spec.name)
        {
            run_cli(&remove, CLI_TIMEOUT).await?;
        }
        let out = run_cli(&add, CLI_TIMEOUT).await?;
        if !out.status.success() {
            return Err(cli_error(target, &out));
        }
        guard.saved = None;
        if let Some(saved) = &saved {
            // Best effort: the update itself worked.
            let _ = keep_user_keys(target, spec, saved);
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
    install_file_locked(target, spec)
}

/// Whether the config still has the pre-rename (PG Desk) entry.
pub(super) fn has_legacy(target: &Target) -> bool {
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
    let _lock = lock_config_async(&target.config).await?;
    if let Some(cli) = cli
        && let Some(remove) = cli_remove(target, cli, name)
    {
        let out = run_cli(&remove, CLI_TIMEOUT).await?;
        if !out.status.success() {
            return Err(cli_error(target, &out));
        }
        return Ok(());
    }
    uninstall_file_locked(target, name)
}
