//! The Accounts model and page in a temp home and data folder: no test runs Claude Code or
//! touches a real account folder.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use sawit_core::agent_accounts::{self as aa, AgentKind, tracking};

use super::page::terminal_command;
use super::{AccountsModel, GlobalAccounts};

fn model(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<AccountsModel>) {
    let tmp = tempfile::tempdir().unwrap();
    let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    let m = cx.update(|cx| {
        let m = cx.new(|cx| AccountsModel::new(root, home, cx));
        cx.set_global(GlobalAccounts(m.clone()));
        m
    });
    (tmp, m)
}

/// The existing ~/.claude is the default account; a new one gets its own folder (0700), can be
/// made active (the pointer names its folder), renamed, and removed with its folder kept.
#[gpui_kit::test]
fn accounts_are_added_switched_and_removed(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let (tmp, m) = model(cx);
    m.read_with(cx, |m, _| {
        assert_eq!(m.list().len(), 1);
        assert!(m.list()[0].is_default);
        assert_eq!(m.active().unwrap().name, "default");
    });
    let id = m.update(cx, |m, cx| m.add("My Work", None, cx)).unwrap();
    let dir = tmp.path().join("home/.claude-my-work");
    assert_eq!(
        std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    m.update(cx, |m, cx| m.set_active(&id, cx));
    m.read_with(cx, |m, _| {
        assert_eq!(m.active().unwrap().id, id);
        assert_eq!(
            aa::read_active(&m.root, AgentKind::ClaudeCode),
            Some(dir.clone())
        );
        let described = m.describe();
        assert_eq!(described[1]["name"], "my-work");
        assert_eq!(described[1]["active"], true);
    });
    m.update(cx, |m, cx| m.rename(&id, "Side", cx));
    m.update(cx, |m, cx| m.remove(&id, cx));
    m.read_with(cx, |m, _| assert_eq!(m.list().len(), 1));
    assert!(dir.is_dir(), "the folder stays");
}

/// Tracking from the model: on (backed up, the collector in statusLine), off (given back).
#[gpui_kit::test]
fn tracking_turns_on_and_off(cx: &mut TestAppContext) {
    let (tmp, m) = model(cx);
    let settings = tmp.path().join("home/.claude/settings.json");
    std::fs::write(&settings, r#"{"theme": "dark"}"#).unwrap();
    let id = m.read_with(cx, |m, _| m.list()[0].id.clone());
    m.update(cx, |m, cx| m.set_tracking(&id, true, cx));
    m.read_with(cx, |m, _| {
        let a = m.accounts.get(&id).unwrap();
        assert_eq!(tracking::state(a), tracking::Tracking::On);
        assert!(m.notice.as_deref().unwrap().contains("backed up"));
    });
    m.update(cx, |m, cx| m.set_tracking(&id, false, cx));
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap().trim(),
        "{\n  \"theme\": \"dark\"\n}"
    );
}

#[test]
fn terminals_run_the_default_account_with_the_variable_unset() {
    let home = std::path::Path::new("/Users/me");
    let mut a = super::new_account("work", home.join(".claude-work"), false);
    let (cmd, env) = terminal_command(&a);
    assert_eq!(cmd, "command claude");
    assert_eq!(
        env,
        [(
            "CLAUDE_CONFIG_DIR".to_string(),
            "/Users/me/.claude-work".to_string()
        )]
    );
    a.is_default = true;
    let (cmd, env) = terminal_command(&a);
    assert_eq!(cmd, "unset CLAUDE_CONFIG_DIR; command claude");
    assert!(env.is_empty());
}

/// The sign-in drives a stand-in `claude auth login`: its link shows, the pasted code goes to its
/// stdin, and it ends signed in; a bad code never reaches it.
#[gpui_kit::test]
fn a_sign_in_runs_claude_and_takes_the_code(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    cx.executor().allow_parking();
    let (tmp, m) = model(cx);
    let fake = tmp.path().join("claude");
    std::fs::write(
        &fake,
        "#!/bin/sh\nif [ \"$1 $2\" = \"auth login\" ]; then\n  echo 'Browser didn'\"'\"'t open? Use the url below to sign in:'\n  echo 'https://claude.ai/oauth/authorize?code=true&state=x'\n  read code\n  [ \"$code\" = \"abcdefgh#state\" ] && exit 0\n  echo 'Invalid code'; exit 1\nfi\necho '{\"loggedIn\": true, \"email\": \"w@example.com\"}'\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let id = m.update(cx, |m, cx| {
        m.claude = Some(fake.clone());
        m.list()[0].id.clone()
    });
    cx.update(|cx| cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start())));
    m.update(cx, |m, cx| m.start_login(&id, cx));
    let wait = |cx: &mut TestAppContext, want: &dyn Fn(Option<super::login::Phase>) -> bool| {
        for _ in 0..200 {
            cx.executor()
                .advance_clock(std::time::Duration::from_millis(300));
            cx.run_until_parked();
            if want(m.read_with(cx, |m, _| m.login_phase())) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!(
            "the sign-in never got there: {:?}",
            m.read_with(cx, |m, _| m.login_phase())
        );
    };
    wait(cx, &|p| p == Some(super::login::Phase::Waiting));
    m.read_with(cx, |m, _| {
        let url = m.login.as_ref().unwrap().manual_url.clone().unwrap();
        assert!(url.starts_with("https://claude.ai/oauth/authorize"));
    });
    m.update(cx, |m, cx| m.submit_code("not a code", cx));
    m.read_with(cx, |m, _| {
        assert!(m.error.is_some(), "a bad code is refused here")
    });
    m.update(cx, |m, cx| m.submit_code("abcdefgh#state", cx));
    wait(cx, &|p| p == Some(super::login::Phase::Done));
}
