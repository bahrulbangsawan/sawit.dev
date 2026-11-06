//! Sessions and Continue On…, usage analytics, limit alerts and the suggestion, Diagnostics: in
//! temp folders with made-up accounts and conversation files.

use std::collections::BTreeMap;

use super::account;
use crate::agent_accounts::usage::{Snapshot, Window};
use crate::agent_accounts::{Accounts, advice, analytics, sessions};

const SID: &str = "4f4ea72c-d657-4352-a139-ba18b373c441";

#[test]
fn sessions_are_read_from_claude_codes_registry_and_handed_off() {
    let tmp = tempfile::tempdir().unwrap();
    let (w, p) = (
        tmp.path().join(".claude-work"),
        tmp.path().join(".claude-personal"),
    );
    std::fs::create_dir_all(w.join("sessions")).unwrap();
    std::fs::write(
        w.join("sessions/4242.json"),
        format!(r#"{{"pid": 4242, "sessionId": "{SID}", "cwd": "/Users/me/api", "status": "idle", "updatedAt": 2000, "startedAt": 1000}}"#),
    )
    .unwrap();
    std::fs::write(
        w.join("sessions/99.json"),
        r#"{"pid": 1, "sessionId": "x", "cwd": "/"}"#,
    )
    .unwrap();
    let (wa, pa) = (account("work", &w, false), account("personal", &p, false));
    let found = sessions::sessions(&wa, &|pid, _| pid == 4242);
    assert_eq!(
        found.len(),
        1,
        "a pid that doesn't match its file is skipped"
    );
    assert_eq!(found[0].folder(), "api");
    assert!(!found[0].busy());
    assert!(
        sessions::sessions(&wa, &|_, _| false).is_empty(),
        "a dead process isn't listed"
    );

    // The conversation and its folder are copied; a copy already there is renamed aside.
    let proj = w.join("projects/-Users-me-api");
    std::fs::create_dir_all(proj.join(SID)).unwrap();
    std::fs::write(proj.join(SID).join("sub.jsonl"), "{}\n").unwrap();
    std::fs::write(proj.join(format!("{SID}.jsonl")), "{\"a\":1}\n{\"partial").unwrap();
    let plan = sessions::plan_handoff(&found[0], &wa, &pa).unwrap();
    assert_eq!(plan.destination, p.join("projects/-Users-me-api"));
    std::fs::create_dir_all(&plan.destination).unwrap();
    std::fs::write(plan.destination.join(format!("{SID}.jsonl")), "older").unwrap();
    let aside = sessions::copy_handoff(&plan, 1_793_961_302).unwrap();
    assert_eq!(aside.len(), 1);
    let copied = std::fs::read_to_string(plan.destination.join(format!("{SID}.jsonl"))).unwrap();
    assert_eq!(
        copied, "{\"a\":1}\n",
        "the half-written last line is left out"
    );
    assert!(plan.destination.join(SID).join("sub.jsonl").is_file());
    assert!(
        sessions::plan_handoff(&found[0], &wa, &wa).is_err(),
        "same account"
    );
}

#[test]
fn analytics_count_each_response_once_with_its_final_numbers() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, w) = (tmp.path().join("sawit"), tmp.path().join(".claude-work"));
    let proj = w.join("projects/x");
    std::fs::create_dir_all(&proj).unwrap();
    let line = |out: i64, t: &str| {
        format!(
            r#"{{"type":"assistant","timestamp":"{t}","requestId":"r1","message":{{"id":"m1","model":"claude-opus","content":[{{"text":"secret prose"}}],"usage":{{"input_tokens":10,"output_tokens":{out},"cache_creation_input_tokens":5,"cache_read_input_tokens":100}}}}}}"#
        )
    };
    let text = [
        line(3, "2026-11-06T10:00:01Z"),
        line(250, "2026-11-06T10:00:02Z"),
        r#"{"type":"user","message":{"content":"hi"}}"#.to_string(),
    ]
    .join("\n")
        + "\n";
    std::fs::write(proj.join("a.jsonl"), text).unwrap();
    let a = account("work", &w, false);
    let mut db = analytics::open(&root).unwrap();
    analytics::scan(&mut db, &[&a], 1_793_961_302).unwrap();
    // Scanning again reads nothing new.
    assert_eq!(analytics::scan(&mut db, &[&a], 1_793_961_302).unwrap(), 0);
    let t = analytics::totals(&db, 0, i64::MAX).unwrap();
    assert_eq!(t.len(), 1);
    let (acc, model, n) = &t[0];
    assert_eq!(
        (acc.as_str(), model.as_str()),
        (a.id.as_str(), "claude-opus")
    );
    assert_eq!(
        (n.requests, n.output, n.input, n.total()),
        (1, 250, 10, 365)
    );
    let stored = std::fs::read(analytics::index_path(&root)).unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains("secret prose"));
}

fn snap(id: &str, five: f64, observed: f64) -> Snapshot {
    Snapshot {
        schema: 1,
        profile_id: id.into(),
        observed_at: Some(observed),
        last_seen_at: observed,
        claude_version: None,
        five_hour: Some(Window {
            used_percentage: five,
            resets_at: 10_000.0,
        }),
        seven_day: None,
    }
}

#[test]
fn alerts_come_once_per_window_and_the_suggestion_only_informs() {
    let (a, b, c) = (
        account("a", std::path::Path::new("/a"), true),
        account("b", std::path::Path::new("/b"), false),
        account("c", std::path::Path::new("/c"), false),
    );
    let mut usage = BTreeMap::new();
    usage.insert(a.id.clone(), snap(&a.id, 92.0, 1_000.0));
    usage.insert(b.id.clone(), snap(&b.id, 40.0, 1_000.0));
    usage.insert(c.id.clone(), snap(&c.id, 10.0, 1_000.0));
    let mut sent = BTreeMap::new();
    let list = [&a, &b, &c];
    // At launch the values there are recorded quietly.
    assert!(advice::alerts(&list, &usage, 90, &mut sent, 1_100.0, true).is_empty());
    usage.insert(a.id.clone(), snap(&a.id, 100.0, 1_200.0));
    let out = advice::alerts(&list, &usage, 90, &mut sent, 1_300.0, false);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].level, 100);
    assert!(
        advice::alerts(&list, &usage, 90, &mut sent, 1_400.0, false).is_empty(),
        "once"
    );
    // Stale values never alert.
    usage.insert(b.id.clone(), snap(&b.id, 95.0, 0.0));
    assert!(advice::alerts(&list, &usage, 90, &mut sent, 1_400.0, false).is_empty());
    usage.insert(b.id.clone(), snap(&b.id, 40.0, 1_000.0));
    let best = advice::suggestion(&list, &a.id, &usage, &|_| true, 90, 1_300.0).unwrap();
    assert_eq!(best.name, "c");
    assert!(
        advice::suggestion(&list, &c.id, &usage, &|_| true, 90, 1_300.0).is_none(),
        "c has room"
    );
    assert!(
        advice::suggestion(&list, &a.id, &usage, &|id| id == a.id, 90, 1_300.0).is_none(),
        "none signed in"
    );
}

#[test]
fn diagnostics_name_variables_never_their_values() {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".zshrc"),
        "# >>> claude-pool >>>\nx\n# <<< claude-pool <<<\n",
    )
    .unwrap();
    let mut all = Accounts::default();
    all.add(account("default", &home.join(".claude"), true))
        .unwrap();
    all.add(account("gone", &home.join(".claude-gone"), false))
        .unwrap();
    let probe = advice::Probe {
        claude: None,
        login_shell: "/bin/zsh",
        env: &|v| v == "ANTHROPIC_API_KEY",
        status: &|_| None,
        exe: None,
    };
    let checks = advice::diagnostics(&root, &home, &all, &probe);
    let find = |t: &str| checks.iter().find(|c| c.title.starts_with(t)).unwrap();
    assert_eq!(find("Claude Code").level, advice::Level::Fail);
    assert!(find("Shell integration").detail.contains("Claude Pool"));
    assert_eq!(
        find("Shell environment").detail,
        "Set for Sawit: ANTHROPIC_API_KEY (names only)."
    );
    assert_eq!(find("Account gone").level, advice::Level::Fail);
}
