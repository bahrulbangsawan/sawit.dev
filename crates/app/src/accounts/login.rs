//! Signing an account in from the page: Claude Code's own `claude auth login`, its links shown
//! with Open in Browser and Copy Link, a field for the code, a 10-minute limit, Cancel and Start
//! Again. When it ends, the sign-in is checked again with `claude auth status`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui_kit::*;
use sawit_core::agent_accounts::login::{LIFETIME_SECS, Login};

use super::AccountsModel;

/// Where a sign-in is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Starting,
    Waiting,
    Submitted,
    Done,
    Failed(String),
    Expired,
}

pub struct LoginState {
    pub account: String,
    pub phase: Phase,
    pub manual_url: Option<String>,
    pub callback_url: Option<String>,
    pub started: Instant,
    login: Arc<Mutex<Option<Login>>>,
    _poll: Task<()>,
}

impl LoginState {
    /// Seconds left before the sign-in expires.
    pub fn left(&self) -> u64 {
        LIFETIME_SECS.saturating_sub(self.started.elapsed().as_secs())
    }
}

impl AccountsModel {
    /// Starts Claude Code's sign-in for `id`.
    pub fn start_login(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(account) = self.accounts.get(id).cloned() else {
            return;
        };
        let Some(bin) = self.claude.clone() else {
            self.error = Some("Claude Code isn't installed (no `claude` on PATH).".into());
            cx.notify();
            return;
        };
        self.cancel_login(cx);
        let started = match Login::start(&bin, &account, &self.home, &self.root, None) {
            Ok(l) => l,
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let login = Arc::new(Mutex::new(Some(started)));
        let poll = cx.spawn({
            let login = login.clone();
            async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(250))
                        .await;
                    let keep = this
                        .update(cx, |m, cx| m.poll_login(&login, cx))
                        .unwrap_or(false);
                    if !keep {
                        return;
                    }
                }
            }
        });
        self.login = Some(LoginState {
            account: id.into(),
            phase: Phase::Starting,
            manual_url: None,
            callback_url: None,
            started: Instant::now(),
            login,
            _poll: poll,
        });
        cx.notify();
    }

    /// Reads where the sign-in is; false once it has ended.
    fn poll_login(&mut self, handle: &Arc<Mutex<Option<Login>>>, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.login.as_mut() else {
            return false;
        };
        let Ok(mut guard) = handle.lock() else {
            return false;
        };
        let Some(login) = guard.as_mut() else {
            return false;
        };
        if state.manual_url.is_none() {
            state.manual_url = login.manual_url();
        }
        if state.callback_url.is_none() {
            state.callback_url = login.callback_url();
        }
        if state.phase == Phase::Starting
            && (state.manual_url.is_some() || state.callback_url.is_some())
        {
            state.phase = Phase::Waiting;
        }
        let finished = login.finished().ok().flatten();
        let id = state.account.clone();
        let ended = match finished {
            Some(0) => {
                state.phase = Phase::Done;
                true
            }
            Some(_) => {
                state.phase = Phase::Failed(
                    login
                        .failure()
                        .unwrap_or_else(|| "Claude Code's sign-in didn't finish.".into()),
                );
                true
            }
            None if state.left() == 0 => {
                login.cancel();
                state.phase = Phase::Expired;
                true
            }
            None => false,
        };
        if ended {
            *guard = None;
            drop(guard);
            self.check(&id, cx);
        }
        cx.notify();
        !ended
    }

    /// Sends the pasted code to Claude Code; the field is emptied by the page.
    pub fn submit_code(&mut self, code: &str, cx: &mut Context<Self>) {
        let Some(state) = self.login.as_mut() else {
            return;
        };
        let result = state
            .login
            .lock()
            .ok()
            .and_then(|mut g| g.as_mut().map(|l| l.submit(code)));
        match result {
            Some(Ok(())) => state.phase = Phase::Submitted,
            Some(Err(e)) => self.error = Some(e.to_string()),
            None => {}
        }
        cx.notify();
    }

    pub fn cancel_login(&mut self, cx: &mut Context<Self>) {
        if let Some(state) = self.login.take()
            && let Ok(mut g) = state.login.lock()
            && let Some(mut l) = g.take()
        {
            l.cancel();
        }
        cx.notify();
    }

    /// Where the sign-in is, for tests.
    #[cfg(test)]
    pub(crate) fn login_phase(&self) -> Option<Phase> {
        self.login.as_ref().map(|l| l.phase.clone())
    }
}
