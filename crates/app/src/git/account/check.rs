//! Asking GitHub whose a saved sign-in is (`GET /user`), and what its answer means for the
//! session: the one call whose 401 ends a sign-in. Tests put an answer of their own in its place,
//! so no test reaches GitHub.

use std::sync::Arc;

use futures::future::BoxFuture;
use gpui_kit::*;
use sawit_core::Result;
use sawit_core::github::{self, Account};

use super::{GitHubAccount, REVOKED_NOTICE, Session};
use crate::rt;

/// `GET /user` with a token: who it belongs to, or why GitHub didn't say.
pub(crate) type Check = Arc<dyn Fn(String) -> BoxFuture<'static, Result<Account>> + Send + Sync>;

/// The app asks GitHub; tests never do: they answer "can't reach GitHub" unless they say
/// otherwise.
pub(super) fn default_check() -> Check {
    #[cfg(test)]
    {
        Arc::new(|_| {
            Box::pin(async {
                Err(sawit_core::CoreError::GitHub(
                    "Unable to reach GitHub (tests never ask it).".into(),
                ))
            })
        })
    }
    #[cfg(not(test))]
    {
        Arc::new(|token| Box::pin(async move { github::account(&token).await }))
    }
}

/// Where the app notes why a sign-in ended (`github-sign-in.log` in Sawit's data folder). Tests
/// note nothing unless they say where.
pub(super) fn default_log() -> Option<std::path::PathBuf> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        Some(
            sawit_core::instances::AppPaths::default_macos()
                .root
                .join(github::SESSION_LOG),
        )
    }
}

/// What the Keychain's sign-in `token` is once GitHub was asked whose it is (`account`), and
/// what to say of it:
///
/// - GitHub knows it: signed in.
/// - GitHub couldn't be reached, or refused for another reason: kept, unconfirmed, with why. It
///   may well still work for git.
/// - `GET /user` with this very token answered 401 (revoked, expired): signed out, saying so, so
///   git goes back to the user's own credentials instead of sending a token GitHub refuses
///   (which read as an account without access to the remote; SMOKE-5).
pub(super) fn saved_session(
    token: String,
    account: Result<Account>,
) -> (Session, Option<&'static str>) {
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

pub(super) async fn fetch_avatar(account: &Result<Account>) -> Option<Vec<u8>> {
    if cfg!(test) {
        return None;
    }
    let url = account.as_ref().ok()?.avatar_url.clone()?;
    github::avatar(&url).await.ok()
}

/// The picture GitHub sent, by its first bytes: a PNG, JPEG, GIF or WebP.
pub(super) fn image(bytes: &[u8]) -> Option<Arc<Image>> {
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

impl GitHubAccount {
    /// A call made with `token` answered 401. When it is the sign-in in use, `GET /user` is asked
    /// with the same token: only a 401 there signs out ([`Self::revoked`]); GitHub knowing the
    /// token, or not answering, keeps it. A burst of 401s asks once. A refusal of a token since
    /// replaced changes nothing; returns whether `token` is the one in use.
    pub fn refused(&mut self, token: &str, cx: &mut Context<Self>) -> bool {
        if self.token() != Some(token) {
            return false;
        }
        if self.rechecking.as_deref() == Some(token) {
            return true;
        }
        self.note("a GitHub call answered 401; asking GET /user with the same sign-in before anything is forgotten");
        self.rechecking = Some(token.to_string());
        let check = self.check.clone();
        let token = token.to_string();
        let rx = rt::spawn(cx, {
            let token = token.clone();
            async move { check(token).await }
        });
        cx.spawn(async move |this, cx| {
            let Ok(account) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.rechecked(token, account, cx));
        })
        .detach();
        true
    }

    /// What `GET /user` said of `token` after another call's 401.
    pub(crate) fn rechecked(
        &mut self,
        token: String,
        account: Result<Account>,
        cx: &mut Context<Self>,
    ) {
        if self.rechecking.as_deref() == Some(token.as_str()) {
            self.rechecking = None;
        }
        if self.token() != Some(token.as_str()) {
            return;
        }
        match account {
            Err(e) if github::is_revoked(&e) => {
                return self.revoked("GET /user answered 401 after another call's 401", cx);
            }
            Ok(account) => {
                self.note("kept the sign-in: GET /user still accepts it after another call's 401");
                if self.account().is_none() {
                    self.session = Session::SignedIn { token, account };
                }
            }
            Err(e) => {
                self.note(&format!(
                    "kept the sign-in: GET /user couldn't be asked after another call's 401: {e}"
                ));
                if let Session::Unconfirmed { error, .. } = &mut self.session {
                    *error = Some(e.to_string());
                }
            }
        }
        cx.notify();
    }

    /// `GET /user` with the sign-in in use answered 401: GitHub revoked it or it expired. The
    /// one case besides Sign Out that forgets it, in the store too, so the next launch starts
    /// signed out without saying it again.
    pub(super) fn revoked(&mut self, why: &str, cx: &mut Context<Self>) {
        let deleted = match self.store.delete() {
            Ok(()) => "the Keychain item was deleted".to_string(),
            Err(e) => format!("the Keychain item couldn't be deleted: {e}"),
        };
        self.note(&format!(
            "signed out: {why} (revoked or expired on GitHub); {deleted}"
        ));
        self.session = Session::SignedOut;
        self.avatar = None;
        self.rechecking = None;
        self.notice = Some(REVOKED_NOTICE.into());
        cx.notify();
    }
}
