//! Usage tracking, opt-in per account: the account's `settings.json` `statusLine` points at the
//! collector, after the user saw the value before and after. The original value is kept in the
//! account (and its command keeps running, chained), `settings.json` is backed up first (the last
//! 10 kept), and turning tracking off gives the original back exactly, unless the user has since
//! put another status line there, which is then left alone.
//!
//! Also "Check Usage Now": Claude Code itself sends one tiny request as the account and reports
//! the windows back (`rate_limit_event`), which go through the collector like a session's report.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::collector;
use super::store::private_dir;
use super::{Account, usage};
use crate::{CoreError, Result};

/// The account's `settings.json`.
pub fn settings_path(account: &Account) -> PathBuf {
    account.dir.join("settings.json")
}

/// The settings as an object: a missing file is `{}`; anything but an object is an error, and
/// nothing is written then.
pub fn read_settings(path: &Path) -> Result<Map<String, Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(CoreError::Config(format!(
            "{} isn't a JSON object, so Sawit changes nothing in it.",
            path.display()
        ))),
    }
}

/// Copies `path` into `accounts/backups/<id>/` before a change (the last 10 kept). Returns the
/// copy, or `None` when there was no file.
pub fn backup(root: &Path, account_id: &str, path: &Path, now: i64) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    let dir = super::store::dir(root).join("backups").join(account_id);
    private_dir(&dir)?;
    let stamp = crate::github::stamp(now).replace([':', '-'], "");
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut to = dir.join(format!("{name}-{stamp}"));
    let mut n = 2;
    while to.exists() {
        to = dir.join(format!("{name}-{stamp}-{n}"));
        n += 1;
    }
    std::fs::copy(path, &to)?;
    let mut all: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name()
                .is_some_and(|f| f.to_string_lossy().starts_with(&*name))
        })
        .collect();
    all.sort();
    for old in all.iter().take(all.len().saturating_sub(10)) {
        let _ = std::fs::remove_file(old);
    }
    Ok(Some(to))
}

/// The `statusLine` before and after turning tracking on, as the confirmation shows them.
pub fn preview(root: &Path, account: &Account) -> Result<(Option<Value>, Value)> {
    let settings = read_settings(&settings_path(account))?;
    let before = settings.get("statusLine").cloned();
    Ok((before.clone(), ours(root, account, before.as_ref())))
}

/// The collector's `statusLine` for `account`, keeping the old one's `padding`.
fn ours(root: &Path, account: &Account, before: Option<&Value>) -> Value {
    let mut v = json!({ "type": "command", "command": collector::command(root, &account.id) });
    if let Some(p) = before.and_then(|b| b.get("padding")) {
        v["padding"] = p.clone();
    }
    v
}

/// Turns tracking on: backup, keep the original, point `statusLine` at the collector. Every
/// other key stays as it was, in its place. Returns the backup.
pub fn enable(root: &Path, account: &mut Account, now: i64) -> Result<Option<PathBuf>> {
    let path = settings_path(account);
    let mut settings = read_settings(&path)?;
    let saved = backup(root, &account.id, &path, now)?;
    let before = settings.get("statusLine").cloned();
    // An earlier collector line (a re-enable) isn't the user's own.
    if before.as_ref().and_then(collector::is_ours).is_none() {
        account.original_status_line = before.clone();
    }
    settings.insert("statusLine".into(), ours(root, account, before.as_ref()));
    write_settings(&path, &settings)?;
    account.tracking = true;
    Ok(saved)
}

/// Turns tracking off: backup, then the original `statusLine` back (or none), unless the current
/// one isn't the collector's any more, which stays.
pub fn disable(root: &Path, account: &mut Account, now: i64) -> Result<Option<PathBuf>> {
    let path = settings_path(account);
    let mut settings = read_settings(&path)?;
    let current = settings.get("statusLine").cloned();
    let saved = backup(root, &account.id, &path, now)?;
    if current.as_ref().and_then(collector::is_ours).is_some() {
        match account.original_status_line.clone() {
            Some(v) => settings.insert("statusLine".into(), v),
            None => settings.remove("statusLine"),
        };
        write_settings(&path, &settings)?;
    }
    account.tracking = false;
    account.original_status_line = None;
    Ok(saved)
}

/// Where tracking stands for an account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tracking {
    Off,
    On,
    /// On in Sawit, but `statusLine` no longer runs the collector.
    Overridden,
}

pub fn state(account: &Account) -> Tracking {
    if !account.tracking {
        return Tracking::Off;
    }
    let ours = read_settings(&settings_path(account))
        .ok()
        .and_then(|s| s.get("statusLine").and_then(collector::is_ours));
    match ours {
        Some(id) if id == account.id => Tracking::On,
        _ => Tracking::Overridden,
    }
}

fn write_settings(path: &Path, settings: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text =
        serde_json::to_string_pretty(settings).map_err(|e| CoreError::Parse(e.to_string()))?;
    crate::files::write_atomic(path, format!("{text}\n").as_bytes())
}

/// The arguments of "Check Usage Now": one tiny Haiku request, no tools, MCP servers, settings
/// files (so no hooks or plugins) or saved session. It costs a few hundred tokens of the account.
pub const PROBE_ARGS: &[&str] = &[
    "-p",
    "Reply with OK.",
    "--system-prompt",
    "Reply with OK.",
    "--model",
    "haiku",
    "--output-format",
    "stream-json",
    "--verbose",
    "--no-session-persistence",
    "--tools",
    "",
    "--strict-mcp-config",
    "--setting-sources",
    "",
    "--disable-slash-commands",
];

/// The windows a probe's output reports: only `rate_limit_info.unifiedWindows` of
/// `rate_limit_event` lines is read; other lines are skipped before they are parsed.
pub fn parse_probe(output: &str) -> Option<usage::Report> {
    let windows = output
        .lines()
        .filter(|l| l.contains("rate_limit_event"))
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|v| v["type"] == "rate_limit_event")
        .filter_map(|v| v["rate_limit_info"]["unified_windows"].as_object().cloned())
        .next_back()?;
    let window = |key: &str| {
        let w = windows.get(key)?;
        Some(usage::Window {
            used_percentage: (w["utilization"].as_f64()? * 100.0).round(),
            resets_at: w["resets_at"].as_f64()?,
        })
    };
    let report = usage::Report {
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
        version: None,
        session_key: None,
    };
    report.has_window().then_some(report)
}
