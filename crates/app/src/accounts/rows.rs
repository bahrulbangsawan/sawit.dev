//! The pieces the page and the chip share: a usage bar with its reset time, and the words for a
//! window's state (no data, reset, stale, fresh). Nothing is estimated: a value is what Claude
//! Code reported, with when.

use sawit_core::agent_accounts::usage::{Snapshot, Window};
use sawit_core::github::ago;

/// A value older than this is "stale".
const STALE_SECS: f64 = 15.0 * 60.0;

/// `23%`.
pub fn pct(w: &Window) -> String {
    format!("{}%", w.used_percentage.round())
}

/// The window if it hasn't reset yet.
pub fn current(w: Option<Window>, now: f64) -> Option<Window> {
    w.filter(|w| w.resets_at > now)
}

/// "5h 23% used · 7d 41% used", or nothing to show.
pub fn short(s: Option<&Snapshot>, now: f64) -> Option<String> {
    let s = s?;
    let parts: Vec<String> = [("5h", s.five_hour), ("7d", s.seven_day)]
        .into_iter()
        .filter_map(|(l, w)| current(w, now).map(|w| format!("{l} {} used", pct(&w))))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// "resets in 2h 13m".
pub fn resets_in(w: &Window, now: f64) -> String {
    let secs = (w.resets_at - now).max(0.0) as i64;
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("resets in {m}m"),
        (0, _) => format!("resets in {h}h {m}m"),
        _ => format!("resets in {d}d {h}h"),
    }
}

/// How old the snapshot is, and whether that is stale.
pub fn observed(s: &Snapshot, now: f64) -> Option<(String, bool)> {
    let at = s.observed_at?;
    Some((
        format!("as of {}", ago(at as i64, now as i64)),
        now - at > STALE_SECS,
    ))
}
