//! GitHub's timestamps as Unix seconds, and how long ago they were.

/// Unix seconds from GitHub's `2024-01-02T03:04:05Z`.
pub fn parse_time(s: &str) -> Option<i64> {
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    // `…T10:00:00+07:00`: apply the offset.
    let (time, offset) = match time.find(['+', '-']) {
        Some(at) => {
            let (t, o) = time.split_at(at);
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let (oh, om) = o[1..].split_once(':').unwrap_or((&o[1..], "0"));
            (
                t,
                sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60),
            )
        }
        None => (time, 0),
    };
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time.splitn(3, ':').map(|p| p.parse::<i64>());
    let (h, min, sec) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);
    // Days from civil (Howard Hinnant).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + h * 3600 + min * 60 + sec - offset)
}

/// "just now", "5 min ago", "3 days ago", … for a Unix time.
pub fn ago(then: i64, now: i64) -> String {
    let s = (now - then).max(0);
    let (n, unit) = match s {
        0..60 => return "just now".into(),
        60..3600 => (s / 60, "min"),
        3600..86400 => (s / 3600, "hour"),
        86400..2_592_000 => (s / 86400, "day"),
        2_592_000..31_536_000 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    let plural = if n == 1 || unit == "min" { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}
