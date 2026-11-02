//! Signing in to remote servers through Claude Code's and Codex's own `mcp login`, one server
//! and profile after another.

use futures::StreamExt as _;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::mcp_auth::{self, LoginEvent, SignIn, is_callback_url, login_call};
use sawit_core::mcp_catalog::SERVERS;

use super::{McpPage, Pair, Phase, Sheet, SignInFlow};

impl McpPage {
    // Sign-in flow

    pub(super) fn start_sign_in(
        &mut self,
        queue: Vec<Pair>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if queue.is_empty() {
            return;
        }
        let address = cx.new(|cx| {
            InputState::new(window, cx).placeholder("http://localhost:…/callback?code=…")
        });
        let subscription =
            cx.subscribe_in(&address, window, |this, _, e: &InputEvent, _, cx| match e {
                InputEvent::PressEnter { .. } => this.submit_address(cx),
                InputEvent::Change => {
                    if let Some(Sheet::SignIn(flow)) = &mut this.sheet {
                        flow.problem = None;
                        cx.notify();
                    }
                }
                _ => {}
            });
        self.sheet = Some(Sheet::SignIn(SignInFlow {
            queue,
            pos: 0,
            done: Vec::new(),
            link: None,
            address,
            phase: Phase::Starting,
            problem: None,
            copied: false,
            login: None,
            _task: None,
            _subscription: subscription,
        }));
        self.focus.focus(window, cx);
        self.begin_current(window, cx);
    }

    /// Starts the client's sign-in for the flow's current server and profile.
    pub(super) fn begin_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &self.sheet else {
            return;
        };
        let Some(&(s, t)) = flow.queue.get(flow.pos) else {
            return;
        };
        let Some(target) = self.targets.get(t).cloned() else {
            return;
        };
        let cli = self.cli_for(target.client);
        let started = cli
            .and_then(|cli| login_call(&target, &cli, SERVERS[s].id))
            .ok_or_else(|| {
                format!(
                    "{} isn't installed, so it can't sign in here.",
                    target.client.name()
                )
            })
            .and_then(|call| {
                mcp_auth::start(&call).map_err(|e| format!("Couldn't start the sign-in: {e}"))
            });
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        flow.link = None;
        flow.problem = None;
        flow.copied = false;
        flow.address.update(cx, |s, cx| s.set_value("", window, cx));
        match started {
            Err(e) => {
                flow.phase = Phase::Failed(e);
                flow.login = None;
            }
            Ok((login, mut events)) => {
                flow.phase = Phase::Starting;
                flow.login = Some(login);
                flow._task = Some(cx.spawn_in(window, async move |this, cx| {
                    while let Some(event) = events.next().await {
                        let ok = this.update_in(cx, |this, window, cx| {
                            this.on_login_event((s, t), event, window, cx)
                        });
                        if ok.is_err() {
                            break;
                        }
                    }
                }));
            }
        }
        cx.notify();
    }

    fn on_login_event(
        &mut self,
        pair: Pair,
        event: LoginEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        if flow.queue.get(flow.pos) != Some(&pair) {
            return;
        }
        match event {
            LoginEvent::Link(url) => {
                flow.link = Some(url);
                flow.phase = Phase::Waiting;
            }
            LoginEvent::Message(line) => {
                if flow.phase == Phase::Finishing {
                    // The client rejected the address and asks again.
                    flow.problem = Some(line);
                    flow.phase = Phase::Waiting;
                }
            }
            LoginEvent::Exited { success, message } => {
                flow.login = None;
                if success {
                    let (s, t) = pair;
                    self.sign_ins
                        .entry(t)
                        .or_default()
                        .insert(SERVERS[s].id.to_string(), SignIn::SignedIn);
                    self.advance(window, cx);
                    return;
                }
                flow.phase = Phase::Failed(if message.is_empty() {
                    "The sign-in stopped before it finished.".into()
                } else {
                    message
                });
            }
        }
        cx.notify();
    }

    /// Moves to the next server and profile, or ends the flow.
    fn advance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        if let Some(&pair) = flow.queue.get(flow.pos)
            && flow.phase != Phase::Starting
            && !matches!(flow.phase, Phase::Failed(_))
        {
            flow.done.push(pair);
        }
        flow.login = None;
        flow.pos += 1;
        if flow.pos < flow.queue.len() {
            self.begin_current(window, cx);
        } else {
            flow.link = None;
            cx.notify();
        }
    }

    pub(super) fn skip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(Sheet::SignIn(flow)) = &mut self.sheet {
            flow.phase = Phase::Starting;
        }
        self.advance(window, cx);
    }

    pub(super) fn submit_address(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::SignIn(flow)) = &mut self.sheet else {
            return;
        };
        let address = flow.address.read(cx).value().trim().to_string();
        if address.is_empty() || flow.phase != Phase::Waiting {
            return;
        }
        if !is_callback_url(&address) {
            flow.problem = Some(
                "That isn't the address the browser ended on. It starts with http://localhost or http://127.0.0.1 and has code= in it."
                    .into(),
            );
        } else if let Some(login) = &flow.login {
            match login.submit(&address) {
                Ok(()) => {
                    flow.phase = Phase::Finishing;
                    flow.problem = None;
                }
                Err(e) => flow.problem = Some(format!("Couldn't pass the address on: {e}")),
            }
        }
        cx.notify();
    }

    pub(super) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        let rechecks: Vec<usize> = match self.sheet.take() {
            Some(Sheet::SignIn(flow)) => {
                let mut t: Vec<usize> = flow.queue.iter().map(|(_, t)| *t).collect();
                t.sort();
                t.dedup();
                t
            }
            _ => Vec::new(),
        };
        // Signed-in states come from the clients, not from our guess.
        if !rechecks.is_empty() {
            self.check_sign_ins(rechecks, cx);
        }
        self.refocus = true;
        cx.notify();
    }
}
