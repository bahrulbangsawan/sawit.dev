//! The collector, usage merging, tracking and the usage check: from Claude Pool's status line
//! fixtures, in temp folders, with a stand-in `claude`.

use std::path::Path;

use serde_json::json;

use super::account;
use crate::agent_accounts::usage::{self, Report, Window};
use crate::agent_accounts::{Accounts, collector, run, tracking};

const FULL: &str = include_str!("../fixtures/statusline-full.json");
const PARTIAL: &str = include_str!("../fixtures/statusline-partial.json");
const NO_LIMITS: &str = include_str!("../fixtures/statusline-no-ratelimits.json");

fn report(session: &str, five: f64, reset: f64) -> Report {
    Report {
        five_hour: Some(Window {
            used_percentage: five,
            resets_at: reset,
        }),
        seven_day: None,
        version: None,
        session_key: Some(session.into()),
    }
}

/// The collector stores only the windows (and the version): none of the paths, cost, model or
/// session id of the input reaches a file, and it prints the short summary.
#[test]
fn the_collector_keeps_only_the_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("sawit");
    let id = "11111111-1111-4111-8111-111111111111";
    let args = vec!["--account".to_string(), id.to_string()];
    let out = collector::run(&args, FULL.as_bytes(), None, &root, 1_790_990_000.0);
    assert_eq!(String::from_utf8(out).unwrap(), "5h 23% · 7d 41%");
    let snap = usage::load_snapshot(&root, id).unwrap();
    assert_eq!(snap.five_hour.unwrap().used_percentage, 23.4);
    assert_eq!(snap.seven_day.unwrap().resets_at, 1_791_500_000.0);
    assert_eq!(snap.claude_version.as_deref(), Some("2.1.296"));
    assert_eq!(usage::load_history(&root, id).len(), 1);
    for entry in walk(&root) {
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        for secret in [
            "secret-project",
            "4f4ea72c",
            "Opus",
            "0.4242",
            "security-reviewer",
            "other-private-dir",
        ] {
            assert!(!text.contains(secret), "{} holds {secret}", entry.display());
        }
    }
    // No windows: nothing observed, nothing printed but the empty summary.
    let out = collector::run(&args, NO_LIMITS.as_bytes(), None, &root, 1_790_990_100.0);
    assert!(out.is_empty());
    assert!(
        collector::run(&args, b"[1]", None, &root, 0.0).is_empty(),
        "not an object"
    );
    assert!(
        collector::run(&[], FULL.as_bytes(), None, &root, 0.0).is_empty(),
        "no account"
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A session started in the home folder runs the default account's status line: its own
/// CLAUDE_CONFIG_DIR says which account it really is.
#[test]
fn a_report_goes_to_the_account_of_its_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir_all(home.join(".claude-work")).unwrap();
    let mut all = Accounts::default();
    let d = all
        .add(account("default", &home.join(".claude"), true))
        .unwrap()
        .id
        .clone();
    let w = all
        .add(account("work", &home.join(".claude-work"), false))
        .unwrap()
        .id
        .clone();
    let work_dir = home.join(".claude-work").to_string_lossy().to_string();
    assert_eq!(collector::account_for(&all, Some(&work_dir), &d), w);
    assert_eq!(collector::account_for(&all, None, &w), d);
    assert_eq!(collector::account_for(&all, Some("/elsewhere"), &w), w);
}

/// An idle session repeating its last value never wins over a working one, and a working session
/// can lower the value (the server lowered it); a newer window replaces an older one.
#[test]
fn the_session_that_changed_last_counts() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let id = "A";
    let t0 = 1_000_000.0;
    usage::record(root, id, &report("a", 40.0, 2_000_000.0), t0).unwrap();
    usage::record(root, id, &report("b", 60.0, 2_000_000.0), t0 + 100.0).unwrap();
    // a repeats itself: b's newer change still counts.
    let s = usage::record(root, id, &report("a", 40.0, 2_000_000.0), t0 + 200.0).unwrap();
    assert_eq!(s.five_hour.unwrap().used_percentage, 60.0);
    // b's usage goes down: it changed last, so it counts.
    let s = usage::record(root, id, &report("b", 55.0, 2_000_000.0), t0 + 300.0).unwrap();
    assert_eq!(s.five_hour.unwrap().used_percentage, 55.0);
    // A newer window wins.
    let s = usage::record(root, id, &report("a", 3.0, 2_018_000.0), t0 + 400.0).unwrap();
    assert_eq!(s.five_hour.unwrap().resets_at, 2_018_000.0);
    // Unchanged values within 5 minutes add no history line.
    let n = usage::load_history(root, id).len();
    usage::record(root, id, &report("a", 3.0, 2_018_000.0), t0 + 450.0).unwrap();
    assert_eq!(usage::load_history(root, id).len(), n);
}

/// Without a session id (older Claude Code), a lower value for the same reset is a stale repeat,
/// unless no session reported for 2 minutes.
#[test]
fn without_a_session_id_a_lower_repeat_is_stale() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let mut r = report("x", 50.0, 2_000_000.0);
    r.session_key = None;
    usage::record(root, "B", &r, 1_000_000.0).unwrap();
    r.five_hour.as_mut().unwrap().used_percentage = 30.0;
    let s = usage::record(root, "B", &r, 1_000_010.0).unwrap();
    assert_eq!(s.five_hour.unwrap().used_percentage, 50.0);
    let s = usage::record(root, "B", &r, 1_000_500.0).unwrap();
    assert_eq!(
        s.five_hour.unwrap().used_percentage,
        30.0,
        "after a gap it is taken"
    );
    let p = Report::parse(PARTIAL).unwrap();
    assert!(p.five_hour.is_some() && p.seven_day.is_none());
}

/// Tracking on: backup, the original kept and still run (chained), `statusLine` pointing at the
/// collector, every other key in its place. Off: the original back exactly. A status line the
/// user set since is left alone. A settings file that isn't an object is never written.
#[test]
fn tracking_is_reversible_and_backed_up() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, dir) = (tmp.path().join("sawit"), tmp.path().join(".claude-work"));
    std::fs::create_dir_all(&dir).unwrap();
    let original = r#"{"model": "opus", "statusLine": {"type": "command", "command": "printf mine", "padding": 2}, "theme": "dark"}"#;
    std::fs::write(dir.join("settings.json"), original).unwrap();
    let mut a = account("work", &dir, false);
    let (before, after) = tracking::preview(&root, &a).unwrap();
    assert_eq!(before.unwrap()["command"], "printf mine");
    assert_eq!(after["padding"], 2);
    let backup = tracking::enable(&root, &mut a, 1_793_961_302)
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), original);
    let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    let keys: Vec<&str> = ["model", "statusLine", "theme"].into_iter().collect();
    let at = |k: &str| text.find(&format!("\"{k}\"")).unwrap();
    assert!(
        keys.windows(2).all(|w| at(w[0]) < at(w[1])),
        "key order kept: {text}"
    );
    assert_eq!(tracking::state(&a), tracking::Tracking::On);
    assert!(text.contains(collector::PROGRAM) && text.contains(&a.id));

    // The collector runs the original with the same input.
    let mut all = Accounts::default();
    all.accounts.push(a.clone());
    all.save(&root).unwrap();
    let args = vec!["--account".to_string(), a.id.clone()];
    let dir_s = dir.to_string_lossy().to_string();
    let out = collector::run(&args, FULL.as_bytes(), Some(&dir_s), &root, 1_790_990_000.0);
    assert_eq!(out, b"mine");

    tracking::disable(&root, &mut a, 1_793_961_400).unwrap();
    let back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap()).unwrap();
    assert_eq!(
        back["statusLine"],
        json!({"type": "command", "command": "printf mine", "padding": 2})
    );
    assert_eq!(tracking::state(&a), tracking::Tracking::Off);

    // Overridden since: left alone on disable.
    tracking::enable(&root, &mut a, 1_793_961_500).unwrap();
    std::fs::write(
        dir.join("settings.json"),
        r#"{"statusLine": {"type": "command", "command": "new"}}"#,
    )
    .unwrap();
    assert_eq!(tracking::state(&a), tracking::Tracking::Overridden);
    tracking::disable(&root, &mut a, 1_793_961_600).unwrap();
    assert!(
        std::fs::read_to_string(dir.join("settings.json"))
            .unwrap()
            .contains("\"new\"")
    );

    std::fs::write(dir.join("settings.json"), "[]").unwrap();
    assert!(tracking::enable(&root, &mut a, 0).is_err());
    assert_eq!(
        std::fs::read_to_string(dir.join("settings.json")).unwrap(),
        "[]"
    );
}

#[test]
fn the_usage_check_reads_only_rate_limit_events() {
    let out = concat!(
        r#"{"type":"assistant","message":{"content":"rate_limit_event secret"}}"#,
        "\n",
        r#"{"type":"rate_limit_event","rate_limit_info":{"unified_windows":{"five_hour":{"utilization":0.234,"resets_at":1791000000},"seven_day":{"utilization":0.41,"resets_at":1791500000}}}}"#,
        "\n"
    );
    let r = tracking::parse_probe(out).unwrap();
    assert_eq!(r.five_hour.unwrap().used_percentage, 23.0);
    assert_eq!(r.seven_day.unwrap().used_percentage, 41.0);
    assert!(tracking::parse_probe("{}").is_none());
}

/// A stand-in `claude` answers like Claude Code: the sign-in check and the usage check run it
/// with the account's folder, and never the real one.
#[test]
fn claude_runs_as_the_account_through_a_stand_in() {
    use std::os::unix::fs::PermissionsExt as _;
    let tmp = tempfile::tempdir().unwrap();
    let (home, root) = (tmp.path().join("home"), tmp.path().join("sawit"));
    std::fs::create_dir_all(home.join(".claude-work")).unwrap();
    let fake = tmp.path().join("claude");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$CLAUDE_CONFIG_DIR $USER\" > {log}\nif [ \"$1\" = auth ]; then echo '{status}'; exit 0; fi\necho '{probe}'\n",
            log = tmp.path().join("log").display(),
            status = r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "w@example.com", "accessToken": "NEVER"}"#,
            probe = r#"{"type":"rate_limit_event","rate_limit_info":{"unified_windows":{"five_hour":{"utilization":0.5,"resets_at":1791000000}}}}"#,
        ),
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let a = account("work", &home.join(".claude-work"), false);
    let s = run::auth_status(&fake, &a, &home).unwrap();
    assert_eq!(s.email.as_deref(), Some("w@example.com"));
    let log = std::fs::read_to_string(tmp.path().join("log")).unwrap();
    assert!(log.starts_with(&home.join(".claude-work").to_string_lossy().to_string()));
    let snap = run::check_usage(&fake, &a, &home, &root).unwrap();
    assert_eq!(snap.five_hour.unwrap().used_percentage, 50.0);
}
