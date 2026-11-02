//! The sign-in's tests: what GitHub says of a saved sign-in, the controls it turns on and off,
//! refusals, and avatars.

use std::sync::Arc;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use sawit_core::CoreError;
use sawit_core::github::{self, Account, TokenStore};

use super::{
    GitHubAccount, GlobalAccount, REVOKED_NOTICE, Session, image, note_refusal, saved_session,
};

fn me() -> Account {
    Account {
        login: "me".into(),
        name: None,
        avatar_url: None,
        scopes: Some(vec!["repo".into(), "workflow".into(), "read:user".into()]),
    }
}

fn account_with(token: Option<&str>, cx: &mut TestAppContext) -> Entity<GitHubAccount> {
    let store: Arc<dyn TokenStore> = match token {
        Some(t) => Arc::new(github::MemoryStore::with(t)),
        None => Arc::new(github::MemoryStore::default()),
    };
    cx.update(|cx| {
        let account = cx.new(|_| GitHubAccount::with_store(store));
        cx.set_global(GlobalAccount(account.clone()));
        account
    })
}

/// SMOKE-5: a saved sign-in that GitHub no longer accepts (401: revoked, expired) is let go,
/// so git stops sending it, and the app says so. One that couldn't be checked (no
/// connection) is kept: it may well still work for git. One GitHub accepts signs in.
#[test]
fn a_saved_sign_in_is_what_github_says_of_it() {
    let refused = Err(CoreError::GitHub(github::REVOKED.into()));
    let (session, said) = saved_session("revoked".into(), refused);
    assert_eq!(session, Session::SignedOut, "git still gets the token");
    assert_eq!(said, Some(REVOKED_NOTICE));

    let offline = "Unable to reach GitHub. Check your connection and try again.";
    let (session, said) = saved_session("saved".into(), Err(CoreError::GitHub(offline.into())));
    assert_eq!(
        session,
        Session::Unconfirmed {
            token: "saved".into(),
            error: Some(offline.into())
        }
    );
    assert_eq!(said, None);

    let (session, said) = saved_session("saved".into(), Ok(me()));
    assert_eq!(
        session,
        Session::SignedIn {
            token: "saved".into(),
            account: me()
        }
    );
    assert_eq!(said, None);
}

/// Every GitHub control asks the account why it can't be used: while signed out it says so,
/// naming what the control does; signed in (or holding a sign-in GitHub couldn't be asked
/// about) nothing stops it.
#[gpui_kit::test]
fn controls_are_off_until_signed_in(cx: &mut TestAppContext) {
    let account = account_with(None, cx);
    account.read_with(cx, |a, _| {
        assert_eq!(
            a.blocked("create a pull request").as_deref(),
            Some("Sign in to GitHub to create a pull request")
        );
        assert_eq!(
            a.describe(),
            serde_json::json!({"signed_in": false, "login": null})
        );
    });
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
    account.read_with(cx, |a, _| {
        assert_eq!(a.blocked("create a pull request"), None);
        assert_eq!(a.token(), Some("gho_1"));
        assert_eq!(
            a.describe(),
            serde_json::json!({"signed_in": true, "login": "me"})
        );
    });
    account.update(cx, |a, cx| {
        a.session = Session::Unconfirmed {
            token: "gho_1".into(),
            error: Some("offline".into()),
        };
        cx.notify();
    });
    account.read_with(cx, |a, _| assert_eq!(a.blocked("clone"), None));
}

/// One sign-in reaches everything that follows the account, at once: here two observers, as
/// two windows' views would be, both see the controls turn on after the one event.
#[gpui_kit::test]
fn one_sign_in_turns_on_every_follower(cx: &mut TestAppContext) {
    let account = account_with(None, cx);
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(usize, bool)>::new()));
    let followers: Vec<_> = (0..2)
        .map(|ix| {
            let seen = seen.clone();
            cx.update(|cx| {
                cx.observe(&account, move |account, cx| {
                    seen.borrow_mut()
                        .push((ix, account.read(cx).blocked("x").is_none()));
                })
            })
        })
        .collect();
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
    cx.run_until_parked();
    assert_eq!(*seen.borrow(), [(0, true), (1, true)]);
    // Signing out from the menu turns them all off again, and forgets the token.
    account.update(cx, |a, cx| a.sign_out(cx));
    cx.run_until_parked();
    assert_eq!(seen.borrow()[2..], [(0, false), (1, false)]);
    account.read_with(cx, |a, _| {
        assert_eq!(a.store.load().unwrap(), None, "the store still has it")
    });
    drop(followers);
}

/// GitHub refusing the sign-in in use signs out everywhere, forgets it and says so once;
/// refusals of the same token still on their way change nothing more, and nor does one of a
/// token since replaced.
#[gpui_kit::test]
fn a_revoked_sign_in_returns_to_signed_out_once(cx: &mut TestAppContext) {
    let account = account_with(Some("gho_old"), cx);
    account.update(cx, |a, cx| a.signed_in("gho_old".into(), me(), None, cx));
    let refused = CoreError::GitHub(github::REVOKED.into());
    let first = cx.update(|cx| note_refusal(&refused, "gho_old", cx));
    assert!(first);
    account.read_with(cx, |a, _| {
        assert!(!a.is_signed_in());
        assert_eq!(a.notice(), Some(REVOKED_NOTICE));
        assert_eq!(a.store.load().unwrap(), None);
    });
    account.update(cx, |a, cx| a.dismiss_notice(cx));
    // The same refusal again (another view's request): handled, said nothing more.
    assert!(cx.update(|cx| note_refusal(&refused, "gho_old", cx)));
    account.read_with(cx, |a, _| assert_eq!(a.notice(), None));

    // A newer sign-in isn't undone by a refusal of the old token.
    account.update(cx, |a, cx| a.signed_in("gho_new".into(), me(), None, cx));
    assert!(!account.update(cx, |a, cx| a.refused("gho_old", cx)));
    account.read_with(cx, |a, _| assert_eq!(a.token(), Some("gho_new")));

    // Any other failure isn't a refusal.
    let offline = CoreError::GitHub("Unable to reach GitHub.".into());
    assert!(!cx.update(|cx| note_refusal(&offline, "gho_new", cx)));
    account.read_with(cx, |a, _| assert!(a.is_signed_in()));
}

/// A saved sign-in found revoked at launch is forgotten and said once; an answer about a
/// token the user has since replaced is dropped.
#[gpui_kit::test]
fn a_launch_check_follows_the_sign_in_it_was_about(cx: &mut TestAppContext) {
    let account = account_with(Some("gho_saved"), cx);
    account.update(cx, |a, cx| {
        a.session = Session::Unconfirmed {
            token: "gho_saved".into(),
            error: None,
        };
        let refused = Err(CoreError::GitHub(github::REVOKED.into()));
        a.confirmed("gho_saved".into(), refused, None, cx);
    });
    account.read_with(cx, |a, _| {
        assert_eq!(a.session, Session::SignedOut);
        assert_eq!(a.notice(), Some(REVOKED_NOTICE));
        assert_eq!(a.store.load().unwrap(), None);
    });
    account.update(cx, |a, cx| {
        a.signed_in("gho_new".into(), me(), None, cx);
        a.confirmed("gho_saved".into(), Ok(me()), None, cx);
    });
    account.read_with(cx, |a, _| assert_eq!(a.token(), Some("gho_new")));
}

/// A sign-in from before Sawit asked for `workflow` says what it lacks, for the account
/// menu's "Sign In Again"; one whose scopes GitHub didn't say lacks nothing.
#[gpui_kit::test]
fn an_older_sign_in_says_what_it_lacks(cx: &mut TestAppContext) {
    let account = account_with(None, cx);
    let mut older = me();
    older.scopes = Some(vec!["repo".into(), "read:user".into()]);
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), older, None, cx));
    account.read_with(cx, |a, _| assert_eq!(a.missing_scopes(), ["workflow"]));
    let mut unknown = me();
    unknown.scopes = None;
    account.update(cx, |a, cx| a.signed_in("gho_2".into(), unknown, None, cx));
    account.read_with(cx, |a, _| assert!(a.missing_scopes().is_empty()));
}

#[test]
fn avatars_are_known_by_their_first_bytes() {
    assert!(image(b"\x89PNG\r\n\x1a\n....").is_some());
    assert!(image(b"\xFF\xD8\xFF\xE0....").is_some());
    assert!(image(b"RIFF\0\0\0\0WEBPVP8 ").is_some());
    assert!(image(b"<html>").is_none());
}
