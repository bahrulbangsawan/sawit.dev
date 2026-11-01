//! The app's one GitHub sign-in. Every GitHub control works on it: the GitHub page (pull
//! requests, issues, your repositories), Clone from GitHub, Save Shell Files to GitHub, and
//! push, pull and fetch over HTTPS to github.com. Signing in once, from any "Sign in", turns
//! them all on in every window at once; signing out from the account menu, or GitHub refusing
//! the sign-in, turns them all off again. The token is kept in the Keychain and nowhere else,
//! and nothing here signs in by itself: the device flow only starts from the user's click.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::github::{self, Account, DeviceCode, TokenStore};
use sawit_core::{CoreError, Result};

use crate::rt;

actions!(
    github,
    [
        /// Signs in to GitHub, the one sign-in every GitHub control uses, and shows the code to
        /// enter on the GitHub page. While a sign-in is under way it only shows the page.
        SignInToGitHub,
        /// Shows the GitHub page.
        OpenGitHub
    ]
);

/// Where the device flow is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SignIn {
    #[default]
    Idle,
    /// Asking GitHub for a code.
    Starting,
    /// Waiting for the user to enter the code on github.com.
    Waiting(DeviceCode),
    Failed(String),
}

/// Whose sign-in the app holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Session {
    /// The Keychain hasn't answered yet (a moment at launch).
    Loading,
    SignedOut,
    /// A saved sign-in GitHub hasn't confirmed: being checked (`error` none), or GitHub couldn't
    /// be reached (`error` why). It is kept and used: it most likely still works, and git may
    /// well reach GitHub when its API didn't.
    Unconfirmed {
        token: String,
        error: Option<String>,
    },
    SignedIn {
        token: String,
        account: Account,
    },
}

/// What the app says, once, when GitHub stops accepting the sign-in (revoked, expired): every
/// GitHub control is off again, and the sign-in card and the title bar say why.
pub const REVOKED_NOTICE: &str = "GitHub no longer accepts the sign-in Sawit saved (it was revoked or expired), so Sawit signed you out. Sign in again to use GitHub.";

pub struct GitHubAccount {
    store: Arc<dyn TokenStore>,
    session: Session,
    pub sign_in: SignIn,
    avatar: Option<Arc<Image>>,
    /// [`REVOKED_NOTICE`], until a new sign-in or the user dismisses it.
    notice: Option<String>,
    _flow: Task<()>,
}

struct GlobalAccount(Entity<GitHubAccount>);

impl Global for GlobalAccount {}

/// Where the app keeps the sign-in: the user's Keychain item, except in tests, which keep theirs
/// in memory, so no test ever signs the user in or out.
fn default_store() -> Arc<dyn TokenStore> {
    #[cfg(test)]
    {
        Arc::new(github::MemoryStore::default())
    }
    #[cfg(not(test))]
    {
        Arc::new(github::Keychain::user())
    }
}

/// The app's sign-in, made on first use.
pub fn global(cx: &mut App) -> Entity<GitHubAccount> {
    if let Some(account) = cx.try_global::<GlobalAccount>() {
        return account.0.clone();
    }
    let account = cx.new(|_| GitHubAccount::new(default_store(), Session::SignedOut));
    cx.set_global(GlobalAccount(account.clone()));
    account
}

/// Reads the saved sign-in at launch and asks GitHub whose it is. It never starts a sign-in.
pub fn init(cx: &mut App) {
    let account = global(cx);
    account.update(cx, |a, cx| a.load(cx));
}

/// When `error` is GitHub refusing `token` itself ([`github::is_revoked`]): signs out everywhere
/// ([`GitHubAccount::refused`]) and returns true, so the caller shows nothing more of its own,
/// the sign-in card taking over.
pub fn note_refusal(error: &CoreError, token: &str, cx: &mut App) -> bool {
    if !github::is_revoked(error) {
        return false;
    }
    global(cx).update(cx, |a, cx| {
        a.refused(token, cx);
    });
    true
}

impl GitHubAccount {
    fn new(store: Arc<dyn TokenStore>, session: Session) -> Self {
        Self {
            store,
            session,
            sign_in: SignIn::Idle,
            avatar: None,
            notice: None,
            _flow: Task::ready(()),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_store(store: Arc<dyn TokenStore>) -> Self {
        Self::new(store, Session::SignedOut)
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The token every GitHub control works with: a confirmed sign-in, or a saved one GitHub
    /// couldn't be asked about.
    pub fn token(&self) -> Option<&str> {
        match &self.session {
            Session::SignedIn { token, .. } | Session::Unconfirmed { token, .. } => Some(token),
            Session::Loading | Session::SignedOut => None,
        }
    }

    pub fn account(&self) -> Option<&Account> {
        match &self.session {
            Session::SignedIn { account, .. } => Some(account),
            _ => None,
        }
    }

    pub fn login(&self) -> Option<&str> {
        self.account().map(|a| a.login.as_str())
    }

    pub fn is_signed_in(&self) -> bool {
        self.token().is_some()
    }

    pub fn avatar(&self) -> Option<Arc<Image>> {
        self.avatar.clone()
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn flow_under_way(&self) -> bool {
        matches!(self.sign_in, SignIn::Starting | SignIn::Waiting(_))
    }

    /// The scopes this sign-in lacks of what Sawit asks for: a sign-in from before Sawit asked
    /// for them. Unknown scopes lack nothing.
    pub fn missing_scopes(&self) -> Vec<&'static str> {
        self.account()
            .and_then(|a| a.scopes.as_deref())
            .map(github::missing_scopes)
            .unwrap_or_default()
    }

    /// Why a GitHub control can't be used yet, as its tooltip says it: `to` is what the control
    /// does ("create a pull request"). `None` once signed in.
    pub fn blocked(&self, to: &str) -> Option<SharedString> {
        (!self.is_signed_in()).then(|| format!("Sign in to GitHub to {to}").into())
    }

    /// Reads the saved sign-in, then asks GitHub whose it is. Found, it is used at once: the
    /// controls don't wait on GitHub's answer.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.session = Session::Loading;
        cx.notify();
        let store = self.store.clone();
        let rx = rt::spawn_blocking(cx, move || store.load());
        cx.spawn(async move |this, cx| {
            let saved = rx.await.ok().and_then(|r| r.ok()).flatten();
            let _ = this.update(cx, |this, cx| {
                if this.session != Session::Loading {
                    return;
                }
                match saved {
                    Some(token) => {
                        this.session = Session::Unconfirmed {
                            token: token.clone(),
                            error: None,
                        };
                        this.confirm(token, cx);
                    }
                    None => this.session = Session::SignedOut,
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Asks GitHub again about a saved sign-in it couldn't be asked about.
    pub fn check_again(&mut self, cx: &mut Context<Self>) {
        if let Session::Unconfirmed { token, .. } = &self.session {
            let token = token.clone();
            self.session = Session::Unconfirmed {
                token: token.clone(),
                error: None,
            };
            self.confirm(token, cx);
            cx.notify();
        }
    }

    /// Asks GitHub whose `token` is, with the account's picture.
    fn confirm(&mut self, token: String, cx: &mut Context<Self>) {
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move {
                let account = github::account(&token).await;
                let avatar = fetch_avatar(&account).await;
                (account, avatar)
            }
        });
        cx.spawn(async move |this, cx| {
            let Ok((account, avatar)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.confirmed(token, account, avatar, cx));
        })
        .detach();
    }

    /// What GitHub said of the saved sign-in `token`. A sign-in or sign-out since it was asked
    /// wins over the answer.
    fn confirmed(
        &mut self,
        token: String,
        account: Result<Account>,
        avatar: Option<Vec<u8>>,
        cx: &mut Context<Self>,
    ) {
        if self.token() != Some(token.as_str()) || self.account().is_some() {
            return;
        }
        let (session, notice) = saved_session(token, account);
        if notice.is_some() {
            // Revoked for good: the next launch starts signed out, without saying it again.
            let _ = self.store.delete();
            self.avatar = None;
        } else {
            self.avatar = avatar.as_deref().and_then(image);
        }
        self.session = session;
        self.notice = notice.map(String::from);
        cx.notify();
    }

    /// Starts the device flow; the GitHub page shows the code to enter. Already under way, it
    /// changes nothing. Signed in already, a new sign-in replaces the old one once approved (to
    /// grant a scope it lacks, say).
    pub fn sign_in(&mut self, cx: &mut Context<Self>) {
        if self.flow_under_way() {
            return;
        }
        self.sign_in = SignIn::Starting;
        self.notice = None;
        cx.notify();
        let rx = rt::spawn(cx, async move { github::start_sign_in().await });
        self._flow = cx.spawn(async move |this, cx| {
            let device = match rx.await {
                Ok(Ok(device)) => device,
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.sign_in = SignIn::Failed(e.to_string());
                        cx.notify();
                    });
                    return;
                }
                Err(_) => return,
            };
            let Ok(rx) = this.update(cx, |this, cx| {
                this.sign_in = SignIn::Waiting(device.clone());
                cx.notify();
                rt::spawn(cx, async move {
                    let token = github::wait_for_token(&device).await?;
                    let account = github::account(&token).await;
                    let avatar = fetch_avatar(&account).await;
                    Ok::<_, CoreError>((token, account?, avatar))
                })
            }) else {
                return;
            };
            // Canceled, this task is dropped before the answer: the token is never kept.
            let result = rx.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(Ok((token, account, avatar))) => this.signed_in(token, account, avatar, cx),
                Ok(Err(e)) => {
                    this.sign_in = SignIn::Failed(e.to_string());
                    cx.notify();
                }
                Err(_) => {
                    this.sign_in = SignIn::Idle;
                    cx.notify();
                }
            });
        });
    }

    /// A sign-in GitHub approved: kept in the store, then on for every control at once.
    pub(crate) fn signed_in(
        &mut self,
        token: String,
        account: Account,
        avatar: Option<Vec<u8>>,
        cx: &mut Context<Self>,
    ) {
        if let Err(e) = self.store.save(&token) {
            self.sign_in = SignIn::Failed(format!("The Keychain didn't keep the sign-in. {e}"));
            cx.notify();
            return;
        }
        self.session = Session::SignedIn { token, account };
        self.avatar = avatar.as_deref().and_then(image);
        self.sign_in = SignIn::Idle;
        self.notice = None;
        cx.notify();
    }

    pub fn cancel_sign_in(&mut self, cx: &mut Context<Self>) {
        self._flow = Task::ready(());
        self.sign_in = SignIn::Idle;
        cx.notify();
    }

    /// Forgets the sign-in, in the store too: every GitHub control is off again.
    pub fn sign_out(&mut self, cx: &mut Context<Self>) {
        self._flow = Task::ready(());
        self.sign_in = match self.store.delete() {
            Ok(()) => SignIn::Idle,
            Err(e) => SignIn::Failed(e.to_string()),
        };
        self.session = Session::SignedOut;
        self.avatar = None;
        self.notice = None;
        cx.notify();
    }

    /// GitHub refused `token` itself (401: revoked, expired). When it is the sign-in in use, the
    /// app returns to signed out everywhere and says so, once: the requests still out with it
    /// find it gone and say nothing. A refusal of a sign-in since replaced changes nothing.
    pub fn refused(&mut self, token: &str, cx: &mut Context<Self>) -> bool {
        if self.token() != Some(token) {
            return false;
        }
        let _ = self.store.delete();
        self.session = Session::SignedOut;
        self.avatar = None;
        self.notice = Some(REVOKED_NOTICE.into());
        cx.notify();
        true
    }

    pub fn dismiss_notice(&mut self, cx: &mut Context<Self>) {
        self.notice = None;
        if matches!(self.sign_in, SignIn::Failed(_)) {
            self.sign_in = SignIn::Idle;
        }
        cx.notify();
    }

    /// For `app_state`: whether agents' GitHub work can go ahead, and as whom. Never the token.
    pub fn describe(&self) -> serde_json::Value {
        serde_json::json!({
            "signed_in": self.is_signed_in(),
            "login": self.login(),
        })
    }
}

/// What the Keychain's sign-in `token` is once GitHub was asked whose it is (`account`), and
/// what to say of it:
///
/// - GitHub knows it: signed in.
/// - GitHub couldn't be reached: kept, unconfirmed, with why. It may well still work for git.
/// - GitHub no longer accepts it (revoked, expired): signed out, saying so, so git goes back to
///   the user's own credentials instead of sending a token GitHub refuses (which read as an
///   account without access to the remote; SMOKE-5).
fn saved_session(token: String, account: Result<Account>) -> (Session, Option<&'static str>) {
    match account {
        Ok(account) => (Session::SignedIn { token, account }, None),
        Err(e) if github::is_revoked(&e) => (Session::SignedOut, Some(REVOKED_NOTICE)),
        Err(e) => (
            Session::Unconfirmed {
                token,
                error: Some(e.to_string()),
            },
            None,
        ),
    }
}

async fn fetch_avatar(account: &Result<Account>) -> Option<Vec<u8>> {
    let url = account.as_ref().ok()?.avatar_url.clone()?;
    github::avatar(&url).await.ok()
}

/// The picture GitHub sent, by its first bytes: a PNG, JPEG, GIF or WebP.
fn image(bytes: &[u8]) -> Option<Arc<Image>> {
    let format = if bytes.starts_with(b"\x89PNG") {
        ImageFormat::Png
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        ImageFormat::Jpeg
    } else if bytes.starts_with(b"GIF8") {
        ImageFormat::Gif
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(&b"WEBP"[..]) {
        ImageFormat::Webp
    } else {
        return None;
    };
    Some(Arc::new(Image::from_bytes(format, bytes.to_vec())))
}

// ---------------------------------------------------------------- controls

/// A GitHub control as the sign-in allows it: as built while signed in; while signed out
/// disabled, its tooltip saying why. Its label stays, so does its accessible name, and its text
/// is muted rather than faded, so it stays legible. Put [`sign_in_link`] beside it.
pub fn gate(button: Button, account: &GitHubAccount, to: &str, cx: &App) -> Button {
    match account.blocked(to) {
        None => button,
        Some(why) => button
            .disabled(true)
            .tooltip(why)
            .text_color(cx.theme().muted_foreground),
    }
}

/// The way on beside a GitHub control while signed out: the one sign-in.
pub fn sign_in_link(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(Lucide::Github)
        .label("Sign In to GitHub")
        .tooltip("One Sign-In for Every GitHub Feature in Sawit")
        .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx))
}

/// Asks, then signs out of GitHub everywhere.
pub fn confirm_sign_out(window: &mut Window, cx: &mut App) {
    let answer = window.prompt(
        PromptLevel::Warning,
        "Sign out of GitHub?",
        Some(
            "Sawit forgets your sign-in, and its GitHub features turn off until you sign in again: pull requests and issues, cloning your repositories, saving shell files. Pushing and pulling keep working with your own Git credentials. Your repositories and local changes stay as they are.",
        ),
        &["Sign Out", "Cancel"],
        cx,
    );
    let account = global(cx);
    cx.spawn(async move |cx| {
        if answer.await == Ok(0) {
            account.update(cx, |a, cx| a.sign_out(cx));
        }
    })
    .detach();
}

/// The account menu, the one place to sign out: who is signed in, the page, a new sign-in when
/// this one lacks a scope Sawit asks for, and Sign Out.
pub fn account_menu(menu: PopupMenu, account: &GitHubAccount) -> PopupMenu {
    let mut menu = match (account.account(), account.session()) {
        (Some(a), _) => menu.label(match &a.name {
            Some(name) => format!("Signed in as @{} ({name})", a.login),
            None => format!("Signed in as @{}", a.login),
        }),
        (None, Session::Unconfirmed { error: Some(e), .. }) => menu.label(e.clone()),
        (None, _) => menu.label("Checking your GitHub sign-in…"),
    };
    let missing = account.missing_scopes();
    if !missing.is_empty() {
        menu = menu.item(
            PopupMenuItem::new(format!("Sign In Again to Allow {}…", missing.join(", ")))
                .icon(Lucide::KeyRound)
                .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx)),
        );
    }
    if matches!(
        account.session(),
        Session::Unconfirmed { error: Some(_), .. }
    ) {
        menu = menu.item(
            PopupMenuItem::new("Check Again")
                .icon(Lucide::RefreshCw)
                .on_click(|_, _, cx| global(cx).update(cx, |a, cx| a.check_again(cx))),
        );
    }
    menu.item(
        PopupMenuItem::new("Open GitHub Page")
            .icon(Lucide::Github)
            .on_click(|_, window, cx| window.dispatch_action(Box::new(OpenGitHub), cx)),
    )
    .separator()
    .item(
        PopupMenuItem::new("Sign Out of GitHub…")
            .icon(Lucide::LogOut)
            .on_click(|_, window, cx| confirm_sign_out(window, cx)),
    )
}

/// The title bar's account control: "Sign In to GitHub" while signed out (marked when GitHub
/// refused the last sign-in), the avatar and login once signed in, with the account menu.
pub fn account_control(account: &Entity<GitHubAccount>, cx: &App) -> AnyElement {
    let a = account.read(cx);
    if a.session == Session::Loading {
        return div().into_any_element();
    }
    if !a.is_signed_in() || a.flow_under_way() {
        let notice = a.notice.clone();
        let label = if a.flow_under_way() {
            "Finish Signing In"
        } else {
            "Sign In to GitHub"
        };
        return Button::new("github-account")
            .ghost()
            .xsmall()
            .icon(match notice {
                Some(_) => Icon::new(Lucide::TriangleAlert).text_color(cx.theme().warning),
                None => Icon::new(Lucide::Github),
            })
            .label(label)
            .tooltip(notice.unwrap_or_else(|| {
                "One sign-in for every GitHub feature: pull requests, issues, cloning and more"
                    .into()
            }))
            .on_click(|_, window, cx| window.dispatch_action(Box::new(SignInToGitHub), cx))
            .into_any_element();
    }
    let login = a.login().map(String::from);
    let unconfirmed = matches!(a.session, Session::Unconfirmed { error: Some(_), .. });
    let lacking = !a.missing_scopes().is_empty();
    let name = login
        .as_ref()
        .map(|l| format!("@{l}"))
        .unwrap_or_else(|| "GitHub".into());
    let avatar = match (a.avatar(), &login) {
        (Some(image), Some(login)) => Avatar::new().name(login.clone()).src(image),
        (None, Some(login)) => Avatar::new().name(login.clone()),
        _ => Avatar::new().placeholder(Lucide::Github),
    };
    let menu_account = account.clone();
    Button::new("github-account")
        .ghost()
        .xsmall()
        .child(avatar.xsmall())
        .child(div().child(name.clone()))
        .when(unconfirmed || lacking, |b| {
            b.child(
                Icon::new(Lucide::TriangleAlert)
                    .xsmall()
                    .text_color(cx.theme().warning),
            )
        })
        .accessibility_label(format!("GitHub Account {name}"))
        .tooltip(if unconfirmed {
            "Sawit couldn't reach GitHub to check your sign-in"
        } else if lacking {
            "Your GitHub sign-in lacks a permission Sawit asks for"
        } else {
            "GitHub Account"
        })
        .dropdown_caret(true)
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, cx| {
            account_menu(menu, menu_account.read(cx))
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
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
}
