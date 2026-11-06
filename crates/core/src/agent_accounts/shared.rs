//! Shared settings: the same MCP servers, skills, plugins, hooks and permission rules in every
//! Claude Code account, so switching never loses one. (Claude Pool's "Shared Extensions".)
//!
//! - The default account holds the primary copy of `skills/`, `agents/`, `commands/`,
//!   `output-styles/` entries and `CLAUDE.md`; the others get a link per entry. One made inside
//!   another account is moved to the default one and linked back. A local entry of the same name
//!   is left alone; `synced/` (claude.ai's per-organisation bucket) stays per account.
//! - Keyed entries merge three ways: `mcpServers` in each account's global config, and
//!   `enabledPlugins`, `extraKnownMarketplaces`, `hooks` and `permissions.allow/deny/ask` in its
//!   `settings.json`. Added in any account, it spreads to all (on a conflict the default's value
//!   wins); removed from an account that held it at the last run, it goes everywhere.
//! - Never shared: the sign-in, `oauthAccount`, `statusLine`, theme, model, every other key.
//! - Every file changed is copied first to `accounts/backups/shared-sync/<time>/` (the last 10
//!   runs kept); a file that isn't JSON stops the run for that set, so nothing is read as deleted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::store::{dir as accounts_dir, private_dir, write_private};
use super::{Account, claude};
use crate::{CoreError, Result};

const LINKED_DIRS: &[&str] = &["skills", "agents", "commands", "output-styles"];
const LINKED_FILES: &[&str] = &["CLAUDE.md"];
const SKIPPED: &[&str] = &["synced"];
const MAX_BACKUPS: usize = 10;

/// account folder → category → keys it held at the last run.
type State = BTreeMap<String, BTreeMap<String, Vec<String>>>;

fn state_path(root: &Path) -> PathBuf {
    accounts_dir(root).join("shared-sync-state.json")
}

fn backups_dir(root: &Path) -> PathBuf {
    accounts_dir(root).join("backups").join("shared-sync")
}

/// What a run changed, one line each.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub changes: Vec<String>,
}

/// One keyed collection inside a JSON object: `(key, value)` pairs in file order.
#[derive(Clone, Copy)]
enum Category {
    Dict(&'static str),
    Hooks,
    Permissions(&'static str),
}

impl Category {
    fn name(self) -> String {
        match self {
            Category::Dict(k) => k.into(),
            Category::Hooks => "hooks".into(),
            Category::Permissions(k) => format!("permissions.{k}"),
        }
    }

    fn get(self, obj: &Map<String, Value>) -> Vec<(String, Value)> {
        match self {
            Category::Dict(k) => obj
                .get(k)
                .and_then(Value::as_object)
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default(),
            Category::Hooks => obj
                .get("hooks")
                .and_then(Value::as_object)
                .map(|hooks| {
                    hooks
                        .iter()
                        .flat_map(|(event, groups)| {
                            groups
                                .as_array()
                                .cloned()
                                .unwrap_or_default()
                                .into_iter()
                                .map(move |g| (format!("{event}\u{0}{}", canonical(&g)), g))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Category::Permissions(k) => obj
                .get("permissions")
                .and_then(|p| p.get(k))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(|s| (s.to_string(), Value::Bool(true)))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    fn set(self, obj: &mut Map<String, Value>, merged: &[(String, Value)]) {
        match self {
            Category::Dict(k) => {
                if merged.is_empty() {
                    obj.remove(k);
                } else {
                    obj.insert(k.into(), Value::Object(merged.iter().cloned().collect()));
                }
            }
            Category::Hooks => {
                if merged.is_empty() {
                    obj.remove("hooks");
                    return;
                }
                let mut hooks = Map::new();
                for (key, group) in merged {
                    let event = key.split('\u{0}').next().unwrap_or_default().to_string();
                    let list = hooks
                        .entry(event)
                        .or_insert_with(|| Value::Array(Vec::new()));
                    if let Value::Array(a) = list {
                        a.push(group.clone());
                    }
                }
                obj.insert("hooks".into(), Value::Object(hooks));
            }
            Category::Permissions(k) => {
                let mut perms = obj
                    .get("permissions")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                if merged.is_empty() {
                    perms.remove(k);
                } else {
                    perms.insert(
                        k.into(),
                        Value::Array(
                            merged
                                .iter()
                                .map(|(s, _)| Value::String(s.clone()))
                                .collect(),
                        ),
                    );
                }
                if perms.is_empty() {
                    obj.remove("permissions");
                } else {
                    obj.insert("permissions".into(), Value::Object(perms));
                }
            }
        }
    }
}

/// A value as sorted, compact JSON: hook groups are told apart by it.
fn canonical(v: &Value) -> String {
    fn sort(v: &Value) -> Value {
        match v {
            Value::Object(m) => {
                let sorted: BTreeMap<_, _> = m.iter().map(|(k, v)| (k.clone(), sort(v))).collect();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(a) => Value::Array(a.iter().map(sort).collect()),
            v => v.clone(),
        }
    }
    sort(v).to_string()
}

fn label(key: &str) -> &str {
    key.split('\u{0}').next().unwrap_or(key)
}

struct Run<'a> {
    root: &'a Path,
    stamp: String,
    changes: Vec<String>,
}

impl Run<'_> {
    fn backup(&self, file: &Path) -> Result<()> {
        let dir = backups_dir(self.root).join(&self.stamp);
        private_dir(&dir)?;
        let name = file.to_string_lossy().replace('/', "_");
        let to = dir.join(name);
        if !to.exists() {
            std::fs::copy(file, to)?;
        }
        Ok(())
    }

    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| !n.starts_with('.') && !SKIPPED.contains(&n.as_str()))
            .collect();
        names.sort();
        names
    }

    fn link_entries(&mut self, name: &str, primary: &Path, others: &[PathBuf]) -> Result<()> {
        let source = primary.join(name);
        let targets: Vec<PathBuf> = others
            .iter()
            .map(|d| d.join(name))
            .filter(|d| !d.is_symlink())
            .collect();
        for dir in &targets {
            for entry in Self::entries(dir) {
                let path = dir.join(&entry);
                if let Ok(dest) = std::fs::read_link(&path) {
                    // A link made here whose primary entry went: it goes too.
                    if !path.exists() && dest.starts_with(&source) {
                        std::fs::remove_file(&path)?;
                        self.changes
                            .push(format!("Removed {entry} from {}", dir.display()));
                    }
                    continue;
                }
                let dest = source.join(&entry);
                if dest.symlink_metadata().is_ok() {
                    continue; // a local entry of the primary's name stays
                }
                std::fs::create_dir_all(&source)?;
                std::fs::rename(&path, &dest)?;
                std::os::unix::fs::symlink(&dest, &path)?;
                self.changes
                    .push(format!("Shared {entry} from {}", dir.display()));
            }
        }
        for entry in Self::entries(&source) {
            let dest = source.join(&entry);
            if !dest.exists() {
                continue; // broken in the primary too
            }
            for dir in &targets {
                let path = dir.join(&entry);
                if path.symlink_metadata().is_ok() {
                    continue;
                }
                std::fs::create_dir_all(dir)?;
                std::os::unix::fs::symlink(&dest, &path)?;
                self.changes
                    .push(format!("Linked {name}/{entry} into {}", dir.display()));
            }
        }
        Ok(())
    }

    fn link_file(&mut self, name: &str, primary: &Path, others: &[PathBuf]) -> Result<()> {
        let source = primary.join(name);
        if source.symlink_metadata().is_err()
            && let Some(local) = others
                .iter()
                .map(|d| d.join(name))
                .find(|p| p.is_file() && !p.is_symlink())
        {
            std::fs::rename(&local, &source)?;
            std::os::unix::fs::symlink(&source, &local)?;
            self.changes.push(format!(
                "Shared {name} from {}",
                local.parent().unwrap_or(Path::new("")).display()
            ));
        }
        if !source.exists() {
            return Ok(());
        }
        for dir in others {
            let path = dir.join(name);
            if path.symlink_metadata().is_err() {
                std::os::unix::fs::symlink(&source, &path)?;
                self.changes
                    .push(format!("Linked {name} into {}", dir.display()));
            }
        }
        Ok(())
    }

    fn sync_files(
        &mut self,
        files: &[PathBuf],
        keys: &[String],
        state: &mut State,
        categories: &[Category],
    ) -> Result<()> {
        let mut originals = Vec::new();
        for f in files {
            originals.push(read_object(f)?);
        }
        let mut objects: Vec<Map<String, Value>> = originals
            .iter()
            .map(|o| o.clone().unwrap_or_default())
            .collect();
        for &cat in categories {
            let name = cat.name();
            let current: Vec<Vec<(String, Value)>> = objects.iter().map(|o| cat.get(o)).collect();
            let mut deleted = BTreeSet::new();
            for (i, values) in current.iter().enumerate() {
                if let Some(prev) = state.get(&keys[i]).and_then(|s| s.get(&name)) {
                    let now: BTreeSet<&String> = values.iter().map(|(k, _)| k).collect();
                    deleted.extend(prev.iter().filter(|k| !now.contains(k)).cloned());
                }
            }
            let mut merged: Vec<(String, Value)> = Vec::new();
            for values in &current {
                for (k, v) in values {
                    if !merged.iter().any(|(m, _)| m == k) {
                        merged.push((k.clone(), v.clone()));
                    }
                }
            }
            for k in &deleted {
                if merged.iter().any(|(m, _)| m == k) {
                    self.changes
                        .push(format!("Removed {} ({name}) from every account", label(k)));
                }
            }
            merged.retain(|(k, _)| !deleted.contains(k));
            let kept: BTreeSet<&String> = merged.iter().map(|(k, _)| k).collect();
            for (i, values) in current.iter().enumerate() {
                let held: BTreeSet<&String> = values.iter().map(|(k, _)| k).collect();
                let added: Vec<&str> = merged
                    .iter()
                    .map(|(k, _)| k)
                    .filter(|k| !held.contains(k))
                    .map(|k| label(k))
                    .collect();
                if !added.is_empty() {
                    self.changes.push(format!(
                        "Added {} ({name}) to {}",
                        added.join(", "),
                        files[i].parent().unwrap_or(Path::new("")).display()
                    ));
                }
                cat.set(&mut objects[i], &merged);
                // Only what was on disk counts as held: an entry written now is confirmed by the
                // next run, so a running session writing an older file isn't read as a removal.
                state.entry(keys[i].clone()).or_default().insert(
                    name.clone(),
                    values
                        .iter()
                        .map(|(k, _)| k.clone())
                        .filter(|k| kept.contains(k))
                        .collect(),
                );
            }
        }
        for (i, obj) in objects.iter().enumerate() {
            let before = originals[i].clone().unwrap_or_default();
            if *obj == before {
                continue;
            }
            if originals[i].is_some() {
                self.backup(&files[i])?;
            }
            let text =
                serde_json::to_string_pretty(obj).map_err(|e| CoreError::Parse(e.to_string()))?;
            if let Some(d) = files[i].parent() {
                std::fs::create_dir_all(d)?;
            }
            crate::files::write_atomic(&files[i], format!("{text}\n").as_bytes())?;
        }
        Ok(())
    }
}

/// The object in `path`; `None` when there is no file (or it is empty). Not JSON: an error.
fn read_object(path: &Path) -> Result<Option<Map<String, Value>>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) if t.trim().is_empty() => return Ok(None),
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str(&text) {
        Ok(Value::Object(m)) => Ok(Some(m)),
        _ => Err(CoreError::Config(format!(
            "{} isn't valid JSON, so shared settings left every account as it was.",
            path.display()
        ))),
    }
}

/// One sync of `accounts` (the default first). Needs the default and one other.
pub fn sync(root: &Path, home: &Path, accounts: &[&Account], now: i64) -> Result<Report> {
    let mut list: Vec<&Account> = accounts
        .iter()
        .copied()
        .filter(|a| a.dir.is_dir())
        .collect();
    list.sort_by_key(|a| !a.is_default);
    if list.len() < 2 || !list[0].is_default {
        return Ok(Report::default());
    }
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let dirs: Vec<PathBuf> = list.iter().map(|a| canon(&a.dir)).collect();
    let keys: Vec<String> = dirs
        .iter()
        .map(|d| d.to_string_lossy().to_string())
        .collect();
    let mut run = Run {
        root,
        stamp: crate::github::stamp(now).replace(':', ""),
        changes: Vec::new(),
    };
    let (primary, others) = (&dirs[0], &dirs[1..]);
    for name in LINKED_DIRS {
        run.link_entries(name, primary, others)?;
    }
    for name in LINKED_FILES {
        run.link_file(name, primary, others)?;
    }
    let mut state: State = std::fs::read_to_string(state_path(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let globals: Vec<PathBuf> = list
        .iter()
        .map(|a| claude::global_config(a, home))
        .collect();
    run.sync_files(&globals, &keys, &mut state, &[Category::Dict("mcpServers")])?;
    let settings: Vec<PathBuf> = dirs.iter().map(|d| d.join("settings.json")).collect();
    run.sync_files(
        &settings,
        &keys,
        &mut state,
        &[
            Category::Dict("enabledPlugins"),
            Category::Dict("extraKnownMarketplaces"),
            Category::Hooks,
            Category::Permissions("allow"),
            Category::Permissions("deny"),
            Category::Permissions("ask"),
        ],
    )?;
    let live: BTreeSet<&String> = keys.iter().collect();
    state.retain(|k, _| live.contains(k));
    private_dir(&accounts_dir(root))?;
    write_private(
        &state_path(root),
        serde_json::to_string_pretty(&state)
            .unwrap_or_default()
            .as_bytes(),
    )?;
    prune_backups(root);
    Ok(Report {
        changes: run.changes,
    })
}

fn prune_backups(root: &Path) {
    let dir = backups_dir(root);
    let runs = Run::entries(&dir);
    for old in runs.iter().take(runs.len().saturating_sub(MAX_BACKUPS)) {
        let _ = std::fs::remove_dir_all(dir.join(old));
    }
}
