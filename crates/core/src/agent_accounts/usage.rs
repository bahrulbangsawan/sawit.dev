//! Usage as Claude Code reports it: the 5-hour and 7-day windows its status line JSON carries for
//! claude.ai sign-ins, kept per account in `accounts/usage/<id>.json` (the latest), `.history.jsonl`
//! (one line per change, at most every 5 minutes) and `.reporters.json` (each open session's last
//! report, by a hash of its id). The files have Claude Pool's format, so its history imports as is.
//! Nothing here is estimated: a value is what Claude Code sent, when it sent it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::store::{private_dir, write_private};
use crate::Result;

/// Reset times closer than this are the same window.
pub const SAME_WINDOW: f64 = 60.0;
/// With no report for this long, no session that reported before is assumed open.
pub const SESSION_GAP: f64 = 120.0;
/// A history line at most this often while nothing changes.
pub const HISTORY_EVERY: f64 = 300.0;

/// One window as reported.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    /// 0–100.
    pub used_percentage: f64,
    /// Unix seconds.
    pub resets_at: f64,
}

/// The latest usage of one account.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default = "one")]
    pub schema: u32,
    pub profile_id: String,
    /// When a window was last present in a report.
    #[serde(default)]
    pub observed_at: Option<f64>,
    /// When any status line run last happened.
    #[serde(default)]
    pub last_seen_at: f64,
    #[serde(default)]
    pub claude_version: Option<String>,
    #[serde(default)]
    pub five_hour: Option<Window>,
    #[serde(default)]
    pub seven_day: Option<Window>,
}

fn one() -> u32 {
    1
}

/// One line of the history.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub t: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fh: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fhr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdr: Option<f64>,
}

impl HistoryEntry {
    fn same_values(&self, o: &HistoryEntry) -> bool {
        self.fh == o.fh && self.fhr == o.fhr && self.sd == o.sd && self.sdr == o.sdr
    }
}

/// The part of the status line JSON read for usage: `rate_limits`, `version`, and `session_id`
/// hashed at once (only to tell sessions apart; the id is never kept). Nothing else is read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    pub version: Option<String>,
    pub session_key: Option<String>,
}

impl Report {
    /// `None` when the input isn't a JSON object.
    pub fn parse(json: &str) -> Option<Report> {
        let v: Value = serde_json::from_str(json).ok()?;
        let obj = v.as_object()?;
        let window = |key: &str| -> Option<Window> {
            let w = obj.get("rate_limits")?.get(key)?;
            let used = w.get("used_percentage")?.as_f64()?;
            let resets = w.get("resets_at")?.as_f64()?;
            (used.is_finite() && resets.is_finite() && resets > 0.0).then_some(Window {
                used_percentage: used,
                resets_at: resets,
            })
        };
        Some(Report {
            five_hour: window("five_hour"),
            seven_day: window("seven_day"),
            version: obj
                .get("version")
                .and_then(Value::as_str)
                .map(|s| s.chars().take(64).collect()),
            session_key: obj
                .get("session_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(session_key),
        })
    }

    pub fn has_window(&self) -> bool {
        self.five_hour.is_some() || self.seven_day.is_some()
    }
}

/// The first 8 bytes of a hash of the session id: enough to tell sessions apart, not to recover it.
fn session_key(id: &str) -> String {
    let full = crate::github::blob_sha(format!("sawit-accounts:{id}").as_bytes());
    full[..16].to_string()
}

/// The usage folder in Sawit's data folder `root`.
pub fn usage_dir(root: &Path) -> PathBuf {
    super::store::dir(root).join("usage")
}

pub fn snapshot_path(root: &Path, id: &str) -> PathBuf {
    usage_dir(root).join(format!("{id}.json"))
}

pub fn history_path(root: &Path, id: &str) -> PathBuf {
    usage_dir(root).join(format!("{id}.history.jsonl"))
}

fn reporters_path(root: &Path, id: &str) -> PathBuf {
    usage_dir(root).join(format!("{id}.reporters.json"))
}

pub fn load_snapshot(root: &Path, id: &str) -> Option<Snapshot> {
    serde_json::from_str(&std::fs::read_to_string(snapshot_path(root, id)).ok()?).ok()
}

/// The history, oldest first; lines that don't read are skipped.
pub fn load_history(root: &Path, id: &str) -> Vec<HistoryEntry> {
    std::fs::read_to_string(history_path(root, id))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

/// Each open session's last report for one account. Per window, the value that counts is the one
/// from the session whose report changed most recently: an idle session repeats its last values
/// unchanged, while a working one reports new ones (so usage can also go down when the server
/// lowers it).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Reporters {
    #[serde(default = "one")]
    schema: u32,
    since: f64,
    #[serde(default)]
    reporters: Vec<Reporter>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Reporter {
    k: String,
    fh: Option<f64>,
    fhr: Option<f64>,
    fc: Option<f64>,
    sd: Option<f64>,
    sdr: Option<f64>,
    sc: Option<f64>,
    s: f64,
}

const WARM_UP: f64 = 30.0;
const MAX_AGE: f64 = 8.0 * 86400.0;
const MAX_REPORTERS: usize = 64;

impl Reporters {
    /// Records `key`'s report; returns the windows that count, and whether this report gave one.
    fn record(
        &mut self,
        key: &str,
        r: &Report,
        now: f64,
    ) -> (Option<Window>, Option<Window>, bool) {
        self.reporters.retain(|x| now - x.s <= MAX_AGE);
        let first_seen = if now - self.since < WARM_UP { 0.0 } else { now };
        let mut me = self
            .reporters
            .iter()
            .find(|x| x.k == key)
            .cloned()
            .unwrap_or(Reporter {
                k: key.into(),
                s: now,
                ..Default::default()
            });
        let changed = |v: Option<f64>, reset: Option<f64>, w: &Window| {
            v != Some(w.used_percentage)
                || !reset.is_some_and(|x| (x - w.resets_at).abs() < SAME_WINDOW)
        };
        if let Some(w) = &r.five_hour {
            if changed(me.fh, me.fhr, w) {
                me.fc = Some(if me.fh.is_none() { first_seen } else { now });
            }
            (me.fh, me.fhr) = (Some(w.used_percentage), Some(w.resets_at));
        }
        if let Some(w) = &r.seven_day {
            if changed(me.sd, me.sdr, w) {
                me.sc = Some(if me.sd.is_none() { first_seen } else { now });
            }
            (me.sd, me.sdr) = (Some(w.used_percentage), Some(w.resets_at));
        }
        me.s = now;
        self.reporters.retain(|x| x.k != key);
        self.reporters.push(me);
        if self.reporters.len() > MAX_REPORTERS {
            self.reporters
                .sort_by(|a, b| b.s.partial_cmp(&a.s).unwrap_or(std::cmp::Ordering::Equal));
            self.reporters.truncate(MAX_REPORTERS);
        }
        let five = choose(
            self.reporters
                .iter()
                .filter_map(|x| Some((x.fh?, x.fhr?, x.fc.unwrap_or(0.0), &x.k))),
        );
        let seven = choose(
            self.reporters
                .iter()
                .filter_map(|x| Some((x.sd?, x.sdr?, x.sc.unwrap_or(0.0), &x.k))),
        );
        let mine = (r.five_hour.is_some() && five.as_ref().is_some_and(|(_, k)| k == key))
            || (r.seven_day.is_some() && seven.as_ref().is_some_and(|(_, k)| k == key));
        (five.map(|(w, _)| w), seven.map(|(w, _)| w), mine)
    }
}

/// The newest window (latest reset), and within it the report that changed most recently.
fn choose<'a>(
    reports: impl Iterator<Item = (f64, f64, f64, &'a String)>,
) -> Option<(Window, String)> {
    let all: Vec<_> = reports.collect();
    let newest = all.iter().map(|r| r.1).fold(f64::MIN, f64::max);
    all.into_iter()
        .filter(|r| newest - r.1 < SAME_WINDOW)
        .max_by(|a, b| {
            (a.2, a.0)
                .partial_cmp(&(b.2, b.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(v, reset, _, k)| {
            (
                Window {
                    used_percentage: v,
                    resets_at: reset,
                },
                k.clone(),
            )
        })
}

/// A report without a session id (older Claude Code): `incoming` unless it is a stale repeat of
/// the stored window (a lower value for the same reset while sessions keep reporting).
fn fresh(incoming: Window, stored: Option<Window>, now: f64, after_gap: bool) -> Option<Window> {
    let Some(stored) = stored.filter(|_| !after_gap) else {
        return Some(incoming);
    };
    if stored.resets_at <= now {
        return Some(incoming);
    }
    if (incoming.resets_at - stored.resets_at).abs() < SAME_WINDOW {
        return (incoming.used_percentage >= stored.used_percentage).then_some(incoming);
    }
    (incoming.resets_at > stored.resets_at).then_some(incoming)
}

/// Merges one status line report into account `id`'s snapshot, reporters and history.
pub fn record(root: &Path, id: &str, report: &Report, now: f64) -> Result<Snapshot> {
    private_dir(&super::store::dir(root))?;
    private_dir(&usage_dir(root))?;
    let stored = load_snapshot(root, id);
    let mut snap = stored.clone().unwrap_or(Snapshot {
        schema: 1,
        profile_id: id.into(),
        last_seen_at: now,
        ..Default::default()
    });
    snap.profile_id = id.into();
    let (five, seven, observed) = if !report.has_window() {
        (None, None, false)
    } else if let Some(key) = &report.session_key {
        let path = reporters_path(root, id);
        let mut log: Reporters = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Reporters {
                schema: 1,
                since: now,
                reporters: Vec::new(),
            });
        let out = log.record(key, report, now);
        write_private(&path, &serde_json::to_vec(&log).unwrap_or_default())?;
        out
    } else {
        let gap = stored
            .as_ref()
            .is_none_or(|s| now - s.last_seen_at > SESSION_GAP);
        let f = report
            .five_hour
            .and_then(|w| fresh(w, stored.as_ref().and_then(|s| s.five_hour), now, gap));
        let s = report
            .seven_day
            .and_then(|w| fresh(w, stored.as_ref().and_then(|s| s.seven_day), now, gap));
        (f, s, f.is_some() || s.is_some())
    };
    if let Some(w) = five {
        snap.five_hour = Some(w);
    }
    if let Some(w) = seven {
        snap.seven_day = Some(w);
    }
    snap.last_seen_at = now;
    if observed {
        snap.observed_at = Some(now);
    }
    if let Some(v) = &report.version {
        snap.claude_version = Some(v.clone());
    }
    write_private(
        &snapshot_path(root, id),
        &serde_json::to_vec(&snap).unwrap_or_default(),
    )?;
    if observed {
        let current =
            |w: Option<Window>, reported: bool| w.filter(|w| reported || w.resets_at > now);
        let f = current(snap.five_hour, report.five_hour.is_some());
        let s = current(snap.seven_day, report.seven_day.is_some());
        let entry = HistoryEntry {
            t: now.round(),
            fh: f.map(|w| w.used_percentage),
            fhr: f.map(|w| w.resets_at),
            sd: s.map(|w| w.used_percentage),
            sdr: s.map(|w| w.resets_at),
        };
        append_history(root, id, &entry)?;
    }
    Ok(snap)
}

fn append_history(root: &Path, id: &str, entry: &HistoryEntry) -> Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;
    if entry.fh.is_none() && entry.sd.is_none() {
        return Ok(());
    }
    if let Some(last) = load_history(root, id).last()
        && last.same_values(entry)
        && entry.t - last.t < HISTORY_EVERY
    {
        return Ok(());
    }
    let mut line = serde_json::to_vec(entry).unwrap_or_default();
    line.push(b'\n');
    // One small O_APPEND write lands as one line even with several sessions writing.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(history_path(root, id))?;
    f.write_all(&line)?;
    Ok(())
}

/// The history kept to the last 8 days and 5,000 lines.
pub fn prune_history(root: &Path, id: &str, now: f64) -> Result<()> {
    let all = load_history(root, id);
    let keep: Vec<_> = all.iter().filter(|e| now - e.t <= 8.0 * 86400.0).collect();
    let keep = &keep[keep.len().saturating_sub(5000)..];
    if keep.len() == all.len() {
        return Ok(());
    }
    let text: String = keep
        .iter()
        .map(|e| serde_json::to_string(e).unwrap_or_default() + "\n")
        .collect();
    write_private(&history_path(root, id), text.as_bytes())
}
