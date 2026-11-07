//! Codex's limits, read from its own session logs: each `rollout-*.jsonl` under
//! `$CODEX_HOME/sessions` records `token_count` events with the account's `rate_limits` (the
//! primary, about five hours, and the secondary, weekly, each with the share used and when it
//! resets). Read-only; Codex keeps its sign-in itself. A Codex account is a `CODEX_HOME`
//! folder: `~/.codex`, and `~/.codex-<name>` for more.

use std::path::{Path, PathBuf};

use super::pool::{Limit, LimitKind};

/// The Codex folders in `home`: `~/.codex` first, then `~/.codex-*`, by name.
pub fn discover(home: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let main = home.join(".codex");
    if main.is_dir() {
        out.push(main);
    }
    let mut more: Vec<PathBuf> = std::fs::read_dir(home)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(".codex-") && n.len() > 7)
        })
        .collect();
    more.sort();
    out.extend(more);
    out
}

/// The account's name from its folder: `default` for `~/.codex`, `work` for `~/.codex-work`.
pub fn name_of(dir: &Path) -> String {
    match dir.file_name().and_then(|n| n.to_str()) {
        Some(".codex") | None => "default".into(),
        Some(n) => n.trim_start_matches(".codex-").to_string(),
    }
}

/// What the newest session log says: the limits and when they were read.
#[derive(Debug, Clone, PartialEq)]
pub struct CodexReading {
    pub limits: Vec<Limit>,
    pub observed_at: f64,
    /// The plan, when the log names it.
    pub plan: Option<String>,
}

/// The newest rollout files under `dir/sessions`, newest first, at most `n`.
fn newest_logs(dir: &Path, n: usize) -> Vec<PathBuf> {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    let mut stack = vec![dir.join("sessions")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
                && let Ok(m) = e.metadata().and_then(|m| m.modified())
            {
                files.push((m, p));
            }
        }
    }
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().take(n).map(|(_, p)| p).collect()
}

/// The latest limits the account's logs recorded, if any.
pub fn read(dir: &Path) -> Option<CodexReading> {
    newest_logs(dir, 5)
        .into_iter()
        .find_map(|f| std::fs::read_to_string(f).ok().and_then(|t| parse_log(&t)))
}

/// The last `rate_limits` of a rollout log's text.
pub fn parse_log(text: &str) -> Option<CodexReading> {
    text.lines().rev().find_map(parse_line)
}

/// One log line's limits, if it is an event carrying them.
pub fn parse_line(line: &str) -> Option<CodexReading> {
    if !line.contains("rate_limits") {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let payload = v.get("payload").unwrap_or(&v);
    let limits = payload
        .get("rate_limits")
        .or_else(|| payload.get("info").and_then(|i| i.get("rate_limits")))?;
    let at = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_rfc3339)?;
    let one = |key: &str| -> Option<Limit> {
        let w = limits.get(key)?;
        let used = w.get("used_percent")?.as_f64()?;
        let minutes = w.get("window_minutes").and_then(|m| m.as_f64());
        let resets_at = match (w.get("resets_at"), w.get("resets_in_seconds")) {
            (Some(r), _) if r.is_number() => r.as_f64()?,
            (_, Some(s)) => at + s.as_f64()?,
            _ => return None,
        };
        // The window's length says which limit it is; without it, the key does.
        let kind = match minutes {
            Some(m) if m <= 24.0 * 60.0 => LimitKind::FiveHour,
            Some(_) => LimitKind::Weekly,
            None if key == "primary" => LimitKind::FiveHour,
            None => LimitKind::Weekly,
        };
        Some(Limit {
            kind,
            used_pct: used,
            resets_at,
        })
    };
    let found: Vec<Limit> = ["primary", "secondary"]
        .into_iter()
        .filter_map(one)
        .collect();
    (!found.is_empty()).then(|| CodexReading {
        limits: found,
        observed_at: at,
        plan: limits
            .get("plan_type")
            .and_then(|p| p.as_str())
            .map(str::to_string),
    })
}

/// `2025-10-29T14:05:00.123Z` (or with an offset) as Unix seconds.
pub fn parse_rfc3339(s: &str) -> Option<f64> {
    let (date, rest) = s.split_once('T')?;
    let mut d = date.split('-').map(|x| x.parse::<i64>().ok());
    let (y, mo, da) = (d.next()??, d.next()??, d.next()??);
    let (time, offset) = match rest.find(['Z', '+', '-']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "Z"),
    };
    let mut t = time.split(':');
    let h: f64 = t.next()?.parse().ok()?;
    let mi: f64 = t.next()?.parse().ok()?;
    let sec: f64 = t.next().unwrap_or("0").parse().ok()?;
    let off = match offset {
        "Z" | "" => 0.0,
        o => {
            let sign = if o.starts_with('-') { -1.0 } else { 1.0 };
            let o = &o[1..];
            let (oh, om) = o
                .split_once(':')
                .unwrap_or((o.get(..2)?, o.get(2..).unwrap_or("0")));
            sign * (oh.parse::<f64>().ok()? * 3600.0 + om.parse::<f64>().unwrap_or(0.0) * 60.0)
        }
    };
    let days = days_from_civil(y, mo as u32, da as u32);
    Some(days as f64 * 86400.0 + h * 3600.0 + mi * 60.0 + sec - off)
}

/// Days since 1970-01-01 of a calendar date (Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_parse_in_utc_and_with_offsets() {
        assert_eq!(parse_rfc3339("1970-01-02T00:00:00Z"), Some(86400.0));
        assert_eq!(
            parse_rfc3339("2025-10-29T14:05:00.5Z"),
            Some(20390.0 * 86400.0 + 50700.5)
        );
        assert_eq!(parse_rfc3339("1970-01-02T02:00:00+02:00"), Some(86400.0));
        assert_eq!(parse_rfc3339("nope"), None);
    }

    #[test]
    fn the_last_rate_limits_of_a_log_are_read() {
        let log = [
            r#"{"timestamp":"1970-01-02T00:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":10.0,"window_minutes":300,"resets_in_seconds":600}}}}"#,
            r#"{"timestamp":"1970-01-02T00:10:00Z","type":"response_item","payload":{"type":"message"}}"#,
            r#"{"timestamp":"1970-01-02T00:20:00Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"primary":{"used_percent":42.5,"window_minutes":300,"resets_at":100000},"secondary":{"used_percent":7.0,"window_minutes":10080,"resets_in_seconds":3600},"plan_type":"plus"}}}"#,
        ]
        .join("\n");
        let r = parse_log(&log).unwrap();
        assert_eq!(r.observed_at, 87600.0);
        assert_eq!(r.plan.as_deref(), Some("plus"));
        assert_eq!(
            r.limits,
            vec![
                Limit {
                    kind: LimitKind::FiveHour,
                    used_pct: 42.5,
                    resets_at: 100000.0
                },
                Limit {
                    kind: LimitKind::Weekly,
                    used_pct: 7.0,
                    resets_at: 87600.0 + 3600.0
                },
            ]
        );
        assert!(parse_log("{\"timestamp\":\"1970-01-02T00:00:00Z\"}").is_none());
    }

    #[test]
    fn folders_are_found_and_named() {
        let tmp = tempfile::tempdir().unwrap();
        for d in [".codex", ".codex-work", ".codexy", ".claude"] {
            std::fs::create_dir_all(tmp.path().join(d)).unwrap();
        }
        let found = discover(tmp.path());
        let names: Vec<String> = found.iter().map(|p| name_of(p)).collect();
        assert_eq!(names, ["default", "work"]);
        let sessions = tmp.path().join(".codex/sessions/2025/10/29");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("rollout-1.jsonl"),
            r#"{"timestamp":"1970-01-02T00:00:00Z","payload":{"rate_limits":{"primary":{"used_percent":5,"resets_in_seconds":60}}}}"#,
        )
        .unwrap();
        let r = read(&found[0]).unwrap();
        assert_eq!(r.limits[0].used_pct, 5.0);
        assert!(read(&found[1]).is_none());
    }
}
