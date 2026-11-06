//! The sign-in's tests: what GitHub says of a saved sign-in, the controls it turns on and off,
//! refusals, Keychain errors, restarts, and avatars.

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
            serde_json::json!({"signed_in": false, "login": null, "state": "signed_out"})
        );
    });
    account.update(cx, |a, cx| a.signed_in("gho_1".into(), me(), None, cx));
    account.read_with(cx, |a, _| {
        assert_eq!(a.blocked("create a pull request"), None);
        assert_eq!(a.token(), Some("gho_1"));
        assert_eq!(
            a.describe(),
            serde_json::json!({"signed_in": true, "login": "me", "state": "signed_in"})
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

/// A 401 from any call but `GET /user` never signs out by itself: the app asks `GET /user` with
/// the same token. GitHub still knowing it, or not answering, keeps it (in the store too) and
/// the caller shows its error; only a 401 there forgets it, says so once and notes why. A
/// refusal of a token since replaced is stale: it changes nothing and says nothing.
#[gpui_kit::test]
fn only_a_401_from_get_user_forgets_the_sign_in(cx: &mut TestAppContext) {
    runtime(cx);
    let account = account_with(Some("gho_old"), cx);
    let log = tempfile::tempdir().unwrap();
    let log_file = log.path().join(github::SESSION_LOG);
    account.update(cx, |a, cx| {
        a.log = Some(log_file.clone());
        a.check = answers(Ok(me()));
        a.signed_in("gho_old".into(), me(), None, cx)
    });
    let refused = CoreError::GitHub(github::REVOKED.into());

    // A 401 from another endpoint; GET /user still accepts the token: kept.
    assert!(
        !cx.update(|cx| note_refusal(&refused, "gho_old", cx)),
        "the caller shows its error"
    );
    settle(cx, &account, |a| a.rechecking.is_none());
    account.read_with(cx, |a, _| {
        assert_eq!(a.token(), Some("gho_old"));
        assert_eq!(a.store.load().unwrap().as_deref(), Some("gho_old"));
        assert_eq!(a.notice(), None);
    });

    // GET /user can't be reached: kept too.
    account.update(cx, |a, _| {
        a.check = answers(Err(CoreError::GitHub("offline".into())))
    });
    cx.update(|cx| note_refusal(&refused, "gho_old", cx));
    settle(cx, &account, |a| a.rechecking.is_none());
    account.read_with(cx, |a, _| {
        assert_eq!(a.store.load().unwrap().as_deref(), Some("gho_old"))
    });

    // GET /user with the same token answers 401: forgotten, said once, noted.
    account.update(cx, |a, _| {
        a.check = answers(Err(CoreError::GitHub(github::REVOKED.into())))
    });
    cx.update(|cx| note_refusal(&refused, "gho_old", cx));
    settle(cx, &account, |a| !a.is_signed_in());
    account.read_with(cx, |a, _| {
        assert_eq!(a.session, Session::SignedOut);
        assert_eq!(a.notice(), Some(REVOKED_NOTICE));
        assert_eq!(a.store.load().unwrap(), None);
    });
    let noted = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        noted.contains("signed out: GET /user answered 401"),
        "{noted}"
    );
    assert!(!noted.contains("gho_old"), "the log never holds the token");

    // The same refusal again (another view's request): stale, says nothing more.
    account.update(cx, |a, cx| a.dismiss_notice(cx));
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

/// A store whose reads fail, as a locked Keychain or a denied access prompt does, and that
/// counts what deletes it.
struct Unreadable {
    token: std::sync::Mutex<Option<String>>,
    readable: std::sync::atomic::AtomicBool,
    deletes: std::sync::atomic::AtomicUsize,
}

impl TokenStore for Unreadable {
    fn load(&self) -> sawit_core::Result<Option<String>> {
        if !self.readable.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(CoreError::Config(
                "Keychain: User interaction is not allowed.".into(),
            ));
        }
        Ok(self.token.lock().unwrap().clone())
    }
    fn save(&self, token: &str) -> sawit_core::Result<()> {
        *self.token.lock().unwrap() = Some(token.into());
        Ok(())
    }
    fn delete(&self) -> sawit_core::Result<()> {
        self.deletes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        *self.token.lock().unwrap() = None;
        Ok(())
    }
}

/// A Keychain read error is not "signed out": the item is kept (nothing deletes it), the app
/// says it couldn't read it and notes why, and Try Again reads it once the Keychain answers.
#[gpui_kit::test]
fn a_keychain_error_keeps_the_sign_in(cx: &mut TestAppContext) {
    runtime(cx);
    let store = Arc::new(Unreadable {
        token: std::sync::Mutex::new(Some("gho_saved".into())),
        readable: false.into(),
        deletes: 0.into(),
    });
    let log = tempfile::tempdir().unwrap();
    let log_file = log.path().join(github::SESSION_LOG);
    let account = cx.update(|cx| {
        let account = cx.new(|_| GitHubAccount::with_store(store.clone()));
        cx.set_global(GlobalAccount(account.clone()));
        account
    });
    account.update(cx, |a, cx| {
        a.log = Some(log_file.clone());
        a.check = answers(Ok(me()));
        a.load(cx)
    });
    settle(cx, &account, |a| a.session != Session::Loading);
    account.read_with(cx, |a, _| {
        assert_eq!(
            a.session,
            Session::Unreadable("Keychain: User interaction is not allowed.".into())
        );
        assert!(a.unreadable().is_some());
        assert!(!a.is_signed_in());
        assert_eq!(a.notice(), None, "no revoked notice");
        assert!(a.blocked("clone").unwrap().starts_with(super::UNREADABLE));
        assert_eq!(a.describe()["state"], "keychain_error");
    });
    assert_eq!(store.deletes.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(store.token.lock().unwrap().as_deref(), Some("gho_saved"));
    assert!(
        std::fs::read_to_string(&log_file)
            .unwrap()
            .contains("couldn't read the sign-in from the Keychain")
    );

    // Try Again, the Keychain answering now: signed in as before.
    store
        .readable
        .store(true, std::sync::atomic::Ordering::SeqCst);
    account.update(cx, |a, cx| a.load(cx));
    settle(cx, &account, |a| a.account().is_some());
    account.read_with(cx, |a, _| assert_eq!(a.token(), Some("gho_saved")));
}

/// A restart with a saved sign-in is signed in: confirmed when GitHub answers, and kept,
/// unconfirmed, when it can't be reached (offline): never signed out.
#[gpui_kit::test]
fn a_restart_with_a_saved_sign_in_is_signed_in(cx: &mut TestAppContext) {
    runtime(cx);
    for (answer, confirmed) in [
        (Ok(me()), true),
        (
            Err(CoreError::GitHub("Unable to reach GitHub.".into())),
            false,
        ),
        (
            Err(CoreError::GitHub(
                "GitHub refused (502): Bad gateway.".into(),
            )),
            false,
        ),
    ] {
        let account = account_with(Some("gho_saved"), cx);
        account.update(cx, |a, cx| {
            a.check = answers(answer);
            a.load(cx)
        });
        settle(cx, &account, |a| match &a.session {
            Session::SignedIn { .. } => true,
            Session::Unconfirmed { error, .. } => error.is_some(),
            _ => false,
        });
        account.read_with(cx, |a, _| {
            assert_eq!(a.token(), Some("gho_saved"));
            assert_eq!(a.account().is_some(), confirmed);
            assert_eq!(a.store.load().unwrap().as_deref(), Some("gho_saved"));
        });
    }
}

/// The runtime the store and `GET /user` answer on; its threads wake the app's tasks.
fn runtime(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start())));
    cx.executor().allow_parking();
}

/// `GET /user` answering `answer` every time it is asked.
fn answers(answer: sawit_core::Result<Account>) -> super::Check {
    let answer = std::sync::Arc::new(answer.map_err(|e| e.to_string()));
    Arc::new(move |_| {
        let answer = answer.clone();
        Box::pin(async move {
            match &*answer {
                Ok(account) => Ok(account.clone()),
                Err(e) => Err(CoreError::GitHub(e.clone())),
            }
        })
    })
}

/// Runs the app and the runtime until `done`, for at most two seconds.
fn settle(
    cx: &mut TestAppContext,
    account: &Entity<GitHubAccount>,
    done: impl Fn(&GitHubAccount) -> bool,
) {
    for _ in 0..200 {
        cx.run_until_parked();
        if account.read_with(cx, |a, _| done(a)) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the account never settled");
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
