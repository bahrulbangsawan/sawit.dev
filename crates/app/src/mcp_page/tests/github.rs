//! GitHub's server, which waits for Sawit's one GitHub sign-in and the scopes it needs.

use super::*;

fn github_account(scopes: &[&str]) -> sawit_core::github::Account {
    sawit_core::github::Account {
        login: "me".into(),
        name: None,
        avatar_url: None,
        scopes: Some(scopes.iter().map(|s| s.to_string()).collect()),
    }
}

/// GitHub's row follows the one sign-in: signed out, its Install is off ("Sign in to GitHub
/// to install", Sign In to GitHub beside it), ↩ doesn't open its sheet and nothing installs.
/// Signed in, it installs, and the config names the helper, never the token. Signing out
/// again uninstalls nothing: the row says so, and Manage still opens to remove it. The
/// sign-in here is the test's own, in memory.
#[gpui_kit::test]
fn github_installs_only_once_signed_in(cx: &mut TestAppContext) {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join(".claude.json");
    std::fs::write(&config, "{}").unwrap();
    let claude = target(Client::ClaudeCode, &config);
    let read = sawit_core::mcp_clients::read_entries(&claude);
    let gh = super::SERVERS
        .iter()
        .position(|s| s.id == sawit_core::mcp_catalog::GITHUB_ID)
        .unwrap();
    let (page, cx) = open(cx, vec![claude], |page| {
        page.entries = vec![read];
        page.recompute();
        page.server_exists = true;
    });
    cx.update(|_, cx| crate::catalog::init(cx));
    let account = cx.update(|_, cx| crate::git::account::global(cx));
    page.read_with(cx, |page, cx| {
        assert_eq!(
            page.github_gate(gh, cx).as_deref(),
            Some("Sign in to GitHub to install")
        );
        assert!(page.row_gated(gh, cx) && page.install_blocked(gh, cx));
        // Only GitHub's: the others install signed out.
        assert_eq!(page.github_gate(0, cx), None);
    });
    let open_selector: &'static str = format!("mcp-open-{gh}").leak();
    assert!(cx.debug_bounds(open_selector).is_some());
    assert!(
        cx.debug_bounds("mcp-github-sign-in").is_some(),
        "no Sign In beside it"
    );

    // Neither the keyboard nor an install by any other way gets past it.
    page.update_in(cx, |page, window, cx| {
        page.cursor = page.shown("").iter().position(|s| *s == gh);
        page.confirm(window, cx);
        assert!(page.sheet.is_none(), "↩ opened the sheet signed out");
        page.act((gh, 0), false, cx);
    });
    std::thread::sleep(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "{}");

    account.update(cx, |a, cx| {
        a.signed_in(
            "gho_secret".into(),
            github_account(&["repo", "workflow", "read:user"]),
            None,
            cx,
        )
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("mcp-github-sign-in").is_none());
    page.read_with(cx, |page, cx| {
        assert_eq!(page.github_gate(gh, cx), None);
        assert!(page.github_lacks(gh, cx).is_empty());
    });
    page.update(cx, |page, cx| page.act((gh, 0), false, cx));
    let installed = (0..50).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        cx.run_until_parked();
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("headersHelper")
    });
    assert!(installed, "it didn't install signed in");
    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("github token --header"), "{text}");
    assert!(
        !text.contains("gho_secret") && !text.contains("Bearer"),
        "{text}"
    );

    // Signed out again: nothing is uninstalled, the row says why its clients can't connect,
    // and Manage (not Install) still opens to remove it.
    account.update(cx, |a, cx| a.sign_out(cx));
    cx.run_until_parked();
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .contains("headersHelper")
    );
    page.update_in(cx, |page, window, cx| {
        assert!(!page.row_gated(gh, cx));
        page.open_install(gh, window, cx);
        assert!(page.install_blocked(gh, cx));
        assert_eq!(page.selected_for(true), [0], "it can't be removed");
    });
}

/// A sign-in from before Sawit asked for `workflow` lacks what GitHub's server needs: its
/// row says so with Sign In Again. Other rows say nothing of it.
#[gpui_kit::test]
fn github_says_what_the_sign_in_lacks(cx: &mut TestAppContext) {
    let gh = super::SERVERS
        .iter()
        .position(|s| s.id == sawit_core::mcp_catalog::GITHUB_ID)
        .unwrap();
    let (page, cx) = open(cx, vec![], |_| {});
    let account = cx.update(|_, cx| crate::git::account::global(cx));
    assert!(cx.debug_bounds("mcp-github-sign-in-again").is_none());
    account.update(cx, |a, cx| {
        a.signed_in(
            "gho_1".into(),
            github_account(&["repo", "read:user"]),
            None,
            cx,
        )
    });
    cx.run_until_parked();
    page.read_with(cx, |page, cx| {
        assert_eq!(page.github_lacks(gh, cx), ["workflow"]);
        assert!(page.github_lacks(0, cx).is_empty());
        assert_eq!(page.github_gate(gh, cx), None);
    });
    assert!(cx.debug_bounds("mcp-github-sign-in-again").is_some());
}
