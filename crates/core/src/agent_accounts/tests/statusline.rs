//! The status line: Claude Pool's layout format read as is, drawn like Claude Pool draws it, and
//! the collector drawing it for an account that uses it.

use serde_json::json;

use super::account;
use crate::agent_accounts::statusline::{self as sl, AccountLine, Config, Kind, Values};
use crate::agent_accounts::usage::{Report, Window};
use crate::agent_accounts::{Accounts, collector};

const FULL: &str = include_str!("../fixtures/statusline-full.json");

/// A layout in Claude Pool's own format (made-up ids), with a component from a newer version
/// that is dropped instead of failing the file.
fn pool_layout() -> serde_json::Value {
    json!({
        "alignment": "left", "colorLevel": "truecolor", "globalBold": false, "padding": 1,
        "powerline": {"enabled": false, "endCap": "\u{e0b0}", "separator": "\u{e0b0}", "startCap": ""},
        "schema": 1, "separator": " │ ", "theme": "nord",
        "lines": [
            [{"bold": true, "id": "A", "kind": "model", "showLabel": false},
             {"bold": false, "id": "B", "kind": "cwd", "showLabel": false},
             {"bold": false, "id": "C", "kind": "somethingNew", "showLabel": false}],
            [{"bold": false, "id": "D", "kind": "account", "showLabel": false},
             {"bold": false, "id": "E", "kind": "fiveHourUsage", "showLabel": true},
             {"bold": false, "id": "F", "kind": "sessionCost", "showLabel": true}]
        ]
    })
}

#[test]
fn claude_pools_layout_reads_and_draws_the_same() {
    let c: Config = serde_json::from_value(pool_layout()).unwrap();
    assert_eq!(c.lines[0].len(), 2, "the unknown component is dropped");
    assert!(c.problems().is_empty());
    let report = Report::parse(FULL).unwrap();
    let line = AccountLine {
        managed: true,
        account: Some("w@example.com".into()),
        ..Default::default()
    };
    let v = Values::from_payload(FULL.as_bytes(), &report, &line, 1_790_990_000.0).unwrap();
    let lines = sl::segments(&c, &v, None);
    let text: Vec<String> = lines
        .iter()
        .map(|l| l.iter().map(|s| s.text.clone()).collect())
        .collect();
    assert_eq!(
        text,
        [
            "Opus │ secret-project",
            "w@example.com │ 5h 23% │ Cost $0.42"
        ]
    );
    let ansi = sl::ansi(&c, &v, None);
    assert!(
        ansi.contains("\u{1b}[1;38;2;136;192;208mOpus\u{1b}[0m"),
        "{ansi:?}"
    );
    // 256 and 16 colours, Powerline segments with the theme's backgrounds.
    let mut c256 = c.clone();
    c256.color_level = "256".into();
    assert!(sl::ansi(&c256, &v, None).contains("38;5;"));
    let mut pl = c.clone();
    pl.theme = "ocean".into();
    pl.powerline.enabled = true;
    assert!(sl::ansi(&pl, &v, None).contains("\u{e0b0}"));
}

#[test]
fn components_say_what_claude_code_sent() {
    let v = Values {
        context_tokens: Some(45_200),
        context_size: Some(200_000),
        duration_ms: Some(2_460_000.0),
        git_status: Some(sl::GitStatus {
            staged: 1,
            untracked: 3,
            ahead: 2,
            ..Default::default()
        }),
        five_hour: Some(Window {
            used_percentage: 23.4,
            resets_at: 2_000.0,
        }),
        output_style: Some("default".into()),
        now: 1_000.0,
        ..Default::default()
    };
    let t = |k: Kind, label: bool| {
        sl::text_for(
            &sl::Widget {
                show_label: label,
                ..sl::Widget::new(k)
            },
            &v,
        )
    };
    assert_eq!(
        t(Kind::ContextTokens, true).as_deref(),
        Some("Ctx 45.2k/200k")
    );
    assert_eq!(t(Kind::SessionDuration, false).as_deref(), Some("41m"));
    assert_eq!(t(Kind::GitStatus, false).as_deref(), Some("+1 ?3 ↑2"));
    assert_eq!(t(Kind::FiveHourReset, false).as_deref(), Some("16m"));
    assert_eq!(
        t(Kind::OutputStyle, false),
        None,
        "the default style shows nothing"
    );
    assert_eq!(t(Kind::Model, false), None);
    let (branch, status) = sl::parse_git("# branch.head main\n# branch.ab +1 -2\n1 M. N...\n? x\n");
    assert_eq!(branch.as_deref(), Some("main"));
    assert_eq!(status.unwrap().behind, 2);
}

/// Apply keeps the layout before for Undo; a layout with a problem isn't applied.
#[test]
fn layouts_apply_and_undo() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let first = Config::default();
    sl::apply(root, &first, 1).unwrap();
    let mut second = first.clone();
    second.theme = "mono".into();
    sl::apply(root, &second, 2).unwrap();
    assert_eq!(sl::load_config(root).unwrap().theme, "mono");
    assert!(sl::undo(root).unwrap());
    assert_eq!(sl::load_config(root).unwrap().theme, "nord");
    let mut bad = first.clone();
    bad.theme = "nope".into();
    assert!(sl::apply(root, &bad, 3).is_err());
    assert_eq!(sl::presets().len(), 4);
}

/// The collector draws Sawit's layout for an account that uses it, with its name from Sawit and
/// its usage from this very report.
#[test]
fn the_collector_draws_the_managed_line() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("sawit");
    let dir = tmp.path().join(".claude-work");
    std::fs::create_dir_all(&dir).unwrap();
    let a = account("work", &dir, false);
    let mut all = Accounts::default();
    all.accounts.push(a.clone());
    all.save(&root).unwrap();
    let c: Config = serde_json::from_value(pool_layout()).unwrap();
    sl::apply(&root, &c, 1).unwrap();
    sl::save_account(
        &root,
        &a.id,
        &AccountLine {
            managed: true,
            profile_name: Some("work".into()),
            account: Some("w@example.com".into()),
            plan: None,
        },
    )
    .unwrap();
    let args = vec!["--account".to_string(), a.id.clone()];
    let d = dir.to_string_lossy().to_string();
    let out = String::from_utf8(collector::run(
        &args,
        FULL.as_bytes(),
        Some(&d),
        &root,
        1_790_990_000.0,
    ))
    .unwrap();
    assert!(
        out.contains("Opus") && out.contains("5h 23%") && out.contains('\n'),
        "{out:?}"
    );
    sl::save_account(&root, &a.id, &AccountLine::default()).unwrap();
    let out = String::from_utf8(collector::run(
        &args,
        FULL.as_bytes(),
        Some(&d),
        &root,
        1_790_990_000.0,
    ))
    .unwrap();
    assert_eq!(out, "5h 23% · 7d 41%", "not managed: the summary");
}
