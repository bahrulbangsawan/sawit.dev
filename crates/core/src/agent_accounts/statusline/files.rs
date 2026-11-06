//! The status line's files: the layout in use (`accounts/statusline/config.json`, the ones before
//! in `history/` for Undo), each account's on/off and what it shows, Claude Pool's presets, and the
//! collector's drawing of it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::render::{self, Values};
use super::{Config, Kind, Powerline, Widget};
use crate::agent_accounts::Account;
use crate::agent_accounts::store::{private_dir, write_private};
use crate::agent_accounts::usage::Report;
use crate::{CoreError, Result};

/// Layouts to start from: Claude Pool's presets.
pub fn presets() -> Vec<(&'static str, Config)> {
    let w = |kind: Kind, label: bool, bold: bool| Widget {
        show_label: label,
        bold,
        ..Widget::new(kind)
    };
    let base = Config {
        lines: Vec::new(),
        theme: "classic".into(),
        ..Config::default()
    };
    vec![
        (
            "Essentials",
            Config {
                lines: vec![vec![
                    w(Kind::Model, false, true),
                    w(Kind::Cwd, false, false),
                    w(Kind::GitBranch, false, false),
                    w(Kind::ContextPercent, true, false),
                    w(Kind::FiveHourUsage, true, false),
                    w(Kind::Account, false, false),
                ]],
                ..base.clone()
            },
        ),
        (
            "Powerline",
            Config {
                lines: vec![vec![
                    w(Kind::Model, false, true),
                    w(Kind::Cwd, false, false),
                    w(Kind::GitBranch, false, false),
                    w(Kind::GitStatus, false, false),
                    w(Kind::ContextPercent, true, false),
                ]],
                theme: "ocean".into(),
                powerline: Powerline {
                    enabled: true,
                    ..Powerline::default()
                },
                ..base.clone()
            },
        ),
        ("Detailed (Two Lines)", Config::default()),
        (
            "Minimal",
            Config {
                lines: vec![vec![
                    w(Kind::Model, false, false),
                    w(Kind::ContextPercent, false, false),
                    w(Kind::FiveHourUsage, true, false),
                ]],
                theme: "minimal".into(),
                separator: " · ".into(),
                ..base
            },
        ),
    ]
}

/// An account's status line state: whether it uses Sawit's layout, and what it shows of the
/// account (written by the app; the collector reads it).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AccountLine {
    pub managed: bool,
    pub profile_name: Option<String>,
    pub account: Option<String>,
    pub plan: Option<String>,
}

pub fn dir(root: &Path) -> PathBuf {
    crate::agent_accounts::store::dir(root).join("statusline")
}

pub fn config_path(root: &Path) -> PathBuf {
    dir(root).join("config.json")
}

pub fn account_path(root: &Path, id: &str) -> PathBuf {
    dir(root).join(format!("profile-{id}.json"))
}

pub fn load_config(root: &Path) -> Option<Config> {
    serde_json::from_str(&std::fs::read_to_string(config_path(root)).ok()?).ok()
}

pub fn load_account(root: &Path, id: &str) -> Option<AccountLine> {
    serde_json::from_str(&std::fs::read_to_string(account_path(root, id)).ok()?).ok()
}

pub fn save_account(root: &Path, id: &str, line: &AccountLine) -> Result<()> {
    private_dir(&dir(root))?;
    write_private(
        &account_path(root, id),
        &serde_json::to_vec_pretty(line).unwrap_or_default(),
    )
}

/// Applies `config`: the one in use goes to `history/` first (the last 10 kept, for Undo).
pub fn apply(root: &Path, config: &Config, now: i64) -> Result<()> {
    let problems = config.problems();
    if !problems.is_empty() {
        return Err(CoreError::Config(problems.join(" ")));
    }
    let history = dir(root).join("history");
    private_dir(&history)?;
    if let Ok(old) = std::fs::read(config_path(root)) {
        let stamp = crate::github::stamp(now).replace(':', "");
        write_private(&history.join(format!("config-{stamp}.json")), &old)?;
        let mut all: Vec<PathBuf> = std::fs::read_dir(&history)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        all.sort();
        for gone in all.iter().take(all.len().saturating_sub(10)) {
            let _ = std::fs::remove_file(gone);
        }
    }
    write_private(
        &config_path(root),
        &serde_json::to_vec_pretty(config).unwrap_or_default(),
    )
}

/// Undo Last Apply: the newest layout in `history/` back in use. `false` when there is none.
pub fn undo(root: &Path) -> Result<bool> {
    let history = dir(root).join("history");
    let mut all: Vec<PathBuf> = std::fs::read_dir(&history)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    all.sort();
    let Some(last) = all.pop() else {
        return Ok(false);
    };
    let bytes = std::fs::read(&last)?;
    write_private(&config_path(root), &bytes)?;
    std::fs::remove_file(last)?;
    Ok(true)
}

/// The managed status line for `account`, when it uses Sawit's: the layout drawn with the
/// session's values (used for this one run, never written), the account's usage, its own status
/// line's first line when the layout shows it, and git when the layout shows it.
pub fn render_managed(
    root: &Path,
    account: Option<&Account>,
    input: &[u8],
    report: &Report,
    now: f64,
) -> Option<String> {
    let account = account?;
    let line = load_account(root, &account.id).filter(|l| l.managed)?;
    let config = load_config(root)?;
    let mut values = Values::from_payload(input, report, &line, now)?;
    if config.uses_git()
        && let Some(dir) = values.cwd.clone()
    {
        let (branch, status) = super::probe::git_probe(&dir);
        values.git_branch = branch;
        values.git_status = status;
    }
    if config.uses_original()
        && let Some(cmd) = account
            .original_status_line
            .as_ref()
            .and_then(|s| s.get("command"))
            .and_then(|c| c.as_str())
        && let Some(out) = crate::agent_accounts::collector::chain(cmd, input)
    {
        values.original = String::from_utf8_lossy(&out)
            .lines()
            .next()
            .map(String::from);
    }
    let text = render::ansi(&config, &values, super::probe::terminal_width());
    (!text.is_empty()).then_some(text)
}
