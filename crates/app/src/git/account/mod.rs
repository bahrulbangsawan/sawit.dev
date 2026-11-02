//! The app's one GitHub sign-in. Every GitHub control works on it: the GitHub page (pull
//! requests, issues, your repositories), Clone from GitHub, Save Shell Files to GitHub, and
//! push, pull and fetch over HTTPS to github.com. Signing in once, from any "Sign in", turns
//! them all on in every window at once; signing out from the account menu, or GitHub refusing
//! the sign-in, turns them all off again. The token is kept in the Keychain and nowhere else,
//! and nothing here signs in by itself: the device flow only starts from the user's click.

mod controls;

use std::sync::Arc;

use gpui_kit::*;
use sawit_core::github::{self, Account, DeviceCode, TokenStore};
use sawit_core::{CoreError, Result};

use crate::rt;

pub use controls::{account_control, account_menu, gate, sign_in_link};

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

#[cfg(test)]
mod tests;
