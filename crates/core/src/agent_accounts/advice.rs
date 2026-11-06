//! Limit alerts and the "most room" suggestion (both off by default, and neither ever switches by
//! itself), and the Diagnostics checks.

use std::collections::BTreeMap;
use std::path::Path;

use super::claude::{AuthStatus, OVERRIDING_VARS};
use super::usage::{Snapshot, Window};
use super::{Account, Accounts, AgentKind, collector, shell, tracking};

/// A value older than this doesn't count for alerts.
pub const FRESH_SECS: f64 = 15.0 * 60.0;

/// One alert to send.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alert {
    pub account: String,
    pub window: &'static str,
    /// The percentage reached: the threshold, or 100.
    pub level: u8,
    pub text: String,
}

fn fresh(s: &Snapshot, now: f64) -> bool {
    s.observed_at.is_some_and(|t| now - t <= FRESH_SECS)
}

/// Alerts not sent yet for these windows: one per window at the threshold and one at 100%, per
/// reset period. `sent` (account·window·reset → level) is updated; `quiet` records without
/// alerting (the values present at launch).
pub fn alerts(
    accounts: &[&Account],
    usage: &BTreeMap<String, Snapshot>,
    threshold: u8,
    sent: &mut BTreeMap<String, u8>,
    now: f64,
    quiet: bool,
) -> Vec<Alert> {
    let mut out = Vec::new();
    for a in accounts {
        let Some(s) = usage.get(&a.id).filter(|s| fresh(s, now)) else {
            continue;
        };
        for (name, w) in [("5-hour", s.five_hour), ("7-day", s.seven_day)] {
            let Some(w) = w.filter(|w| w.resets_at > now) else {
                continue;
            };
            let used = w.used_percentage.round() as u8;
            let level = if used >= 100 {
                100
            } else if used >= threshold {
                threshold
            } else {
                continue;
            };
            let key = format!("{}·{name}·{}", a.id, w.resets_at as i64);
            if sent.get(&key).is_some_and(|l| *l >= level) {
                continue;
            }
            sent.insert(key, level);
            if !quiet {
                out.push(Alert {
                    account: a.id.clone(),
                    window: name,
                    level,
                    text: if level == 100 {
                        format!("{} reached its {name} limit.", a.name)
                    } else {
                        format!("{} has used {used}% of its {name} window.", a.name)
                    },
                });
            }
        }
    }
    sent.retain(|k, _| {
        k.rsplit('·')
            .next()
            .and_then(|r| r.parse::<f64>().ok())
            .is_some_and(|r| r > now)
    });
    out
}

/// The higher of an account's two windows now (`None` when neither is known).
pub fn load(s: &Snapshot, now: f64) -> Option<f64> {
    let cur = |w: Option<Window>| w.filter(|w| w.resets_at > now).map(|w| w.used_percentage);
    match (cur(s.five_hour), cur(s.seven_day)) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0).max(b.unwrap_or(0.0))),
    }
}

/// While the active account is at or above `threshold`: the signed-in account with the most
/// room (fresh values first, then the user's order). Informs only.
pub fn suggestion<'a>(
    accounts: &[&'a Account],
    active: &str,
    usage: &BTreeMap<String, Snapshot>,
    signed_in: &dyn Fn(&str) -> bool,
    threshold: u8,
    now: f64,
) -> Option<&'a Account> {
    let busy = usage
        .get(active)
        .and_then(|s| load(s, now))
        .is_some_and(|l| l >= threshold as f64);
    if !busy {
        return None;
    }
    accounts
        .iter()
        .filter(|a| a.id != active && signed_in(&a.id))
        .filter_map(|a| {
            usage
                .get(&a.id)
                .and_then(|s| Some((*a, load(s, now)?, fresh(s, now))))
        })
        .filter(|(_, l, _)| *l < threshold as f64)
        .min_by(|x, y| {
            (!x.2, x.1, x.0.order)
                .partial_cmp(&(!y.2, y.1, y.0.order))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(a, _, _)| a)
}

/// How a check came out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Pass,
    Warn,
    Fail,
    Info,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    pub title: String,
    pub level: Level,
    pub detail: String,
    /// What to do about it.
    pub fix: Option<String>,
}

fn check(
    title: impl Into<String>,
    level: Level,
    detail: impl Into<String>,
    fix: Option<&str>,
) -> Check {
    Check {
        title: title.into(),
        level,
        detail: detail.into(),
        fix: fix.map(String::from),
    }
}

/// What the checks look at besides the files.
pub struct Probe<'a> {
    pub claude: Option<&'a Path>,
    pub login_shell: &'a str,
    /// Whether a variable is set for this process (names are reported, never values).
    pub env: &'a dyn Fn(&str) -> bool,
    /// Each account's last sign-in check.
    pub status: &'a dyn Fn(&str) -> Option<AuthStatus>,
    /// This app's executable, which the collector's link must point at.
    pub exe: Option<&'a Path>,
}

/// Everything switching and tracking depend on.
pub fn diagnostics(root: &Path, home: &Path, all: &Accounts, p: &Probe) -> Vec<Check> {
    let (claude, login_shell, env, status, exe) = (p.claude, p.login_shell, p.env, p.status, p.exe);
    let mut out = Vec::new();
    out.push(match claude {
        Some(p) => check("Claude Code", Level::Pass, p.display().to_string(), None),
        None => check(
            "Claude Code",
            Level::Fail,
            "No `claude` on PATH or in the usual places.",
            Some("Install Claude Code (Dev Tools page)."),
        ),
    });
    let file = shell::target(home, login_shell);
    let text = file
        .as_ref()
        .and_then(|f| std::fs::read_to_string(f).ok())
        .unwrap_or_default();
    out.push(match (file, shell::state(&text, root)) {
        (None, _) => check(
            "Shell integration",
            Level::Info,
            format!("{login_shell} isn't zsh or bash: use Open in Terminal or sawit account open."),
            None,
        ),
        (Some(_), shell::BlockState::Installed) => check(
            "Shell integration",
            Level::Pass,
            "A plain claude starts as the active account.",
            None,
        ),
        (Some(f), shell::BlockState::Outdated) => check(
            "Shell integration",
            Level::Warn,
            format!("The block in {} is from an older Sawit.", f.display()),
            Some("Accounts › Settings › Update."),
        ),
        (Some(f), shell::BlockState::Unbalanced) => check(
            "Shell integration",
            Level::Fail,
            format!("The block's markers in {} were edited.", f.display()),
            Some("Fix the file by hand."),
        ),
        (Some(f), shell::BlockState::Absent) if shell::has_pool_block(&text) => check(
            "Shell integration",
            Level::Warn,
            format!(
                "{} still has Claude Pool's block, which reads Claude Pool's pointer.",
                f.display()
            ),
            Some("Accounts › Settings › Replace Claude Pool's Block."),
        ),
        (Some(_), shell::BlockState::Absent) => check(
            "Shell integration",
            Level::Warn,
            "Not installed: a plain claude in a new terminal uses the default account.",
            Some("Accounts › Settings › Install."),
        ),
    });
    let set: Vec<&str> = OVERRIDING_VARS.iter().copied().filter(|v| env(v)).collect();
    out.push(if set.is_empty() {
        check(
            "Shell environment",
            Level::Pass,
            "No variable overrides the accounts' sign-ins.",
            None,
        )
    } else {
        check(
            "Shell environment",
            Level::Warn,
            format!("Set for Sawit: {} (names only).", set.join(", ")),
            Some("Remove these exports from your shell startup files."),
        )
    });
    let list = all.of(AgentKind::ClaudeCode);
    out.push(match super::read_active(root, AgentKind::ClaudeCode) {
        Some(d) if !list.iter().any(|a| a.dir == d) => check(
            "Active account",
            Level::Warn,
            format!("The pointer names {}, which isn't an account.", d.display()),
            Some("Set an account active."),
        ),
        _ => check(
            "Active account",
            Level::Pass,
            all.active(AgentKind::ClaudeCode, root)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "none".into()),
            None,
        ),
    });
    let link = collector::link_path(root);
    out.push(match (std::fs::read_link(&link).ok(), exe) {
        (Some(t), Some(e)) if t == e => check(
            "Usage collector",
            Level::Pass,
            link.display().to_string(),
            None,
        ),
        _ => check(
            "Usage collector",
            Level::Warn,
            format!("{} doesn't point at this Sawit.", link.display()),
            Some("Open Sawit again: it relinks at launch."),
        ),
    });
    for a in list {
        let title = format!("Account {}", a.name);
        if !a.dir.is_dir() {
            out.push(check(
                title,
                Level::Fail,
                format!("{} is missing.", a.dir.display()),
                Some("Remove the account, or create the folder."),
            ));
            continue;
        }
        match status(&a.id) {
            None => out.push(check(&title, Level::Info, "Sign-in not checked yet.", None)),
            Some(s) if !s.signed_in => out.push(check(
                &title,
                Level::Warn,
                "Signed out.",
                Some("Sign In on the Accounts page."),
            )),
            Some(s) if !s.reports_usage() => out.push(check(
                &title,
                Level::Warn,
                format!(
                    "Signed in with {}, not claude.ai: no usage windows.",
                    s.auth_method.unwrap_or_default()
                ),
                Some("Remove the overriding variable or apiKeyHelper, then sign in again."),
            )),
            Some(s) => {
                let isolated = match (&s.config_directory, a.env_value()) {
                    (Some(c), Some(d)) => Path::new(c) == d,
                    _ => true,
                };
                out.push(if isolated {
                    check(&title, Level::Pass, s.identity().unwrap_or_default(), None)
                } else {
                    check(
                        &title,
                        Level::Fail,
                        "Claude Code reports another folder for this account.",
                        Some("Look for a global CLAUDE_CONFIG_DIR or a wrapper around claude."),
                    )
                });
            }
        }
        if tracking::state(a) == tracking::Tracking::Overridden {
            out.push(check(
                format!("{} tracking", a.name),
                Level::Warn,
                "statusLine no longer runs Sawit.",
                Some("Accounts › Re-enable…"),
            ));
        }
    }
    out.push(check(
        "Not checked",
        Level::Info,
        "Workspace trust, disableAllHooks, managed policies or a project statusLine can stop usage reports in some projects.",
        None,
    ));
    out
}
