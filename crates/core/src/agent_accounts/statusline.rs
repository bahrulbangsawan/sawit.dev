//! Sawit's own status line, drawn by the collector for accounts that use it. (The layout and its
//! renderer come with the Status Line tab.)

use std::path::Path;

use super::Account;
use super::usage::Report;

/// The managed status line for `account`, when it uses Sawit's; `None` otherwise.
pub fn render_managed(
    _root: &Path,
    _account: Option<&Account>,
    _input: &[u8],
    _report: &Report,
    _now: f64,
) -> Option<String> {
    None
}
