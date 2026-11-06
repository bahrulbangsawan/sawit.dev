//! Why a GitHub sign-in ended, kept in a small file in Sawit's data folder: the user pressed
//! Sign Out, `GET /user` with the saved token answered 401, or the Keychain couldn't be read.
//! A report of "Sawit signed me out" can then show the cause. Never the token.

use std::io::Write as _;
use std::path::Path;

use super::time::now;

/// The file's name in Sawit's data folder.
pub const FILE: &str = "github-sign-in.log";
/// The lines kept: the newest, so the file never grows past a few kilobytes.
const KEEP: usize = 200;

/// Appends `why` with the time (UTC) to the log at `path`, keeping its last [`KEEP`] lines.
/// Best effort: a log that can't be written never stops the app.
pub fn note_session(path: &Path, why: &str) {
    let line = format!("{} {}\n", stamp(now()), why.replace('\n', " "));
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let mut lines: Vec<&str> = old.lines().collect();
    let drop = (lines.len() + 1).saturating_sub(KEEP);
    lines.drain(..drop);
    let mut text = lines.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&line);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(mut file) = std::fs::File::create(path) {
        let _ = file.write_all(text.as_bytes());
    }
}

/// `2026-11-06T10:35:02Z` for Unix seconds.
pub(super) fn stamp(secs: i64) -> String {
    let days = secs.div_euclid(86400);
    let rem = secs.rem_euclid(86400);
    // Civil from days (Howard Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
