//! The Accounts page's Usage tab (token analytics per account and model, and the running
//! sessions with Continue On… and End…) and Diagnostics tab; and the model's limit alerts.

use std::collections::BTreeMap;

use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::advice::{self, Level};
use sawit_core::agent_accounts::analytics::{self, Totals};
use sawit_core::agent_accounts::sessions::{self, Session};

use super::page::AccountsPage;
use super::{AccountsModel, SignIn};
use crate::rt;

/// A terminal to open: its title, command and environment.
pub type NewTerminal = (String, String, Vec<(String, String)>);

/// The analytics' periods.
pub const PERIODS: &[(&str, i64)] = &[
    ("24 Hours", 86_400),
    ("7 Days", 7 * 86_400),
    ("30 Days", 30 * 86_400),
];

fn short(n: i64) -> String {
    match n {
        i64::MIN..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1}M", n as f64 / 1e6),
        _ => format!("{:.2}B", n as f64 / 1e9),
    }
}

impl AccountsModel {
    /// Every account's running sessions.
    pub fn sessions(&self) -> Vec<Session> {
        self.list()
            .iter()
            .flat_map(|a| sessions::sessions(a, &sessions::process_alive))
            .collect()
    }

    /// Indexes what is new in the conversation files, then sums the period and the one before.
    pub fn refresh_analytics(&mut self, period: i64, cx: &mut Context<Self>) {
        let root = self.root.clone();
        let accounts: Vec<_> = self.list().into_iter().cloned().collect();
        let rx = rt::spawn_blocking(cx, move || {
            let mut db = analytics::open(&root)?;
            let refs: Vec<_> = accounts.iter().collect();
            let now = sawit_core::github::now();
            analytics::scan(&mut db, &refs, now)?;
            Ok::<_, sawit_core::CoreError>((
                analytics::totals(&db, now - period, now)?,
                analytics::totals(&db, now - 2 * period, now - period)?,
            ))
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |m, cx| {
                match result {
                    Ok(t) => m.analytics = Some(t),
                    Err(e) => m.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Copies the session's conversation into `target` and opens a terminal there running
    /// `claude --resume`; the session itself keeps running.
    pub fn hand_off(
        &mut self,
        session: &Session,
        target: &str,
        cx: &mut Context<Self>,
    ) -> Option<NewTerminal> {
        let (Some(from), Some(to)) = (
            self.accounts.get(&session.account).cloned(),
            self.accounts.get(target).cloned(),
        ) else {
            return None;
        };
        let done = sessions::plan_handoff(session, &from, &to)
            .and_then(|p| sessions::copy_handoff(&p, sawit_core::github::now()));
        match done {
            Ok(_) => {
                self.notice = Some(format!(
                    "Copied the conversation to {}. The session on {} keeps running until you end it.",
                    to.name, from.name
                ));
                cx.notify();
                let (_, env) = super::page::terminal_command(&to);
                let unset = if to.is_default {
                    "unset CLAUDE_CONFIG_DIR; "
                } else {
                    ""
                };
                let cmd = format!(
                    "cd {} && {unset}command claude --resume {}",
                    sawit_core::terminal::shell_quote(std::path::Path::new(&session.cwd)),
                    session.session_id
                );
                Some((format!("claude · {}", to.name), cmd, env))
            }
            Err(e) => {
                self.error = Some(e.to_string());
                cx.notify();
                None
            }
        }
    }

    /// The limit alerts due now (Settings: on, threshold), sent once each; the first run after
    /// launch records what is there without alerting.
    pub fn check_alerts(&mut self, cx: &mut Context<Self>) {
        let s = &self.accounts.settings;
        if !s.limit_alerts {
            return;
        }
        let now = sawit_core::github::now() as f64;
        let usage: BTreeMap<_, _> = self.usage.clone().into_iter().collect();
        let list: Vec<_> = self.list().into_iter().cloned().collect();
        let refs: Vec<_> = list.iter().collect();
        let quiet = !self.alerts_primed;
        self.alerts_primed = true;
        let out = advice::alerts(
            &refs,
            &usage,
            s.limit_threshold,
            &mut self.alerts_sent,
            now,
            quiet,
        );
        for a in out {
            notify(&a.text);
            self.notice = Some(a.text);
        }
        cx.notify();
    }

    /// The account with the most room while the active one is near a limit (Settings: on).
    pub fn suggested(&self) -> Option<String> {
        let s = &self.accounts.settings;
        if !s.suggest_account {
            return None;
        }
        let active = self.active()?.id.clone();
        let usage: BTreeMap<_, _> = self.usage.clone().into_iter().collect();
        let list = self.list();
        let signed_in =
            |id: &str| matches!(self.sign_in.get(id), Some(SignIn::Known(s)) if s.signed_in);
        advice::suggestion(
            &list,
            &active,
            &usage,
            &signed_in,
            s.limit_threshold,
            sawit_core::github::now() as f64,
        )
        .map(|a| a.id.clone())
    }
}

/// A macOS notification, best effort (`osascript`; macOS may ask once).
fn notify(text: &str) {
    let script = format!(
        "display notification \"{}\" with title \"Sawit\"",
        text.replace(['"', '\\'], "")
    );
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args(["-e", &script])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

impl AccountsPage {
    pub(super) fn render_usage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let muted = cx.theme().muted_foreground;
        let names: BTreeMap<String, String> = m
            .list()
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        let period = self.period;
        let card = |title: String| {
            v_flex()
                .gap_2()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().font_semibold().child(title))
        };
        let totals = |rows: &[(String, String, Totals)]| {
            let mut by: BTreeMap<String, Totals> = BTreeMap::new();
            for (a, _, t) in rows {
                let e = by.entry(a.clone()).or_default();
                e.input += t.input;
                e.output += t.output;
                e.cache_write += t.cache_write;
                e.cache_read += t.cache_read;
                e.requests += t.requests;
            }
            by
        };
        let analytics_rows: Vec<AnyElement> = match &m.analytics {
            None => vec![
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Reading Claude Code's conversation files…")
                    .into_any_element(),
            ],
            Some((now_rows, before_rows)) => {
                let (now_t, before_t) = (totals(now_rows), totals(before_rows));
                if now_t.is_empty() {
                    vec![
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("No usage recorded on this Mac in this period.")
                            .into_any_element(),
                    ]
                } else {
                    now_t
                        .iter()
                        .map(|(id, t)| {
                            let prev = before_t.get(id).map(|p| p.total()).unwrap_or(0);
                            let change = if prev > 0 {
                                format!(
                                    " ({:+.0}% on the period before)",
                                    (t.total() - prev) as f64 * 100.0 / prev as f64
                                )
                            } else {
                                String::new()
                            };
                            let models: Vec<String> = now_rows
                                .iter()
                                .filter(|(a, _, _)| a == id)
                                .map(|(_, mdl, x)| format!("{mdl} {}", short(x.total())))
                                .collect();
                            v_flex()
                                .child(div().text_sm().child(format!(
                                    "{}: {} tokens{change} · {} requests",
                                    names.get(id).cloned().unwrap_or_else(|| id.clone()),
                                    short(t.total()),
                                    t.requests
                                )))
                                .child(div().text_xs().text_color(muted).child(format!(
                                    "in {} · out {} · cache write {} · cache read {} · {}",
                                    short(t.input),
                                    short(t.output),
                                    short(t.cache_write),
                                    short(t.cache_read),
                                    models.join(", ")
                                )))
                                .into_any_element()
                        })
                        .collect()
                }
            }
        };
        let list = m.sessions();
        let others: Vec<(String, String)> = m
            .list()
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        let session_rows: Vec<AnyElement> = list
            .into_iter()
            .map(|s| {
                let s2 = s.clone();
                let targets: Vec<(String, String)> = others
                    .iter()
                    .filter(|(id, _)| *id != s.account)
                    .cloned()
                    .collect();
                let page = cx.entity();
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().truncate().text_sm().child(format!(
                        "{} · {} · {} · pid {}",
                        names.get(&s.account).cloned().unwrap_or_default(),
                        s.folder(),
                        s.status.clone().unwrap_or_default(),
                        s.pid
                    )))
                    .child(
                        Button::new(SharedString::from(format!("hand-off-{}", s.pid)))
                            .xsmall()
                            .outline()
                            .label("Continue On…")
                            .disabled(targets.is_empty())
                            .dropdown_menu(move |menu, _, _| {
                                targets.iter().fold(menu, |menu, (id, name)| {
                                    let (page, id, s) = (page.clone(), id.clone(), s2.clone());
                                    menu.item(PopupMenuItem::new(name.clone()).on_click(
                                        move |_, _, cx| {
                                            page.update(cx, |this, cx| {
                                                let opened = this
                                                    .model
                                                    .update(cx, |m, cx| m.hand_off(&s, &id, cx));
                                                if let Some((title, command, env)) = opened {
                                                    cx.emit(super::AccountsEvent::OpenTerminal {
                                                        title,
                                                        command,
                                                        env,
                                                    });
                                                }
                                            })
                                        },
                                    ))
                                })
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("end-{}", s.pid)))
                            .xsmall()
                            .ghost()
                            .label("End…")
                            .disabled(s.busy())
                            .tooltip(if s.busy() {
                                "Busy: End It in Its Terminal"
                            } else {
                                "Stop This Session (SIGTERM)"
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.confirm_end(s.clone(), window, cx)
                            })),
                    )
                    .into_any_element()
            })
            .collect();
        let session_count = session_rows.len();
        v_flex()
            .id("accounts-usage")
            .flex_1()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(
                h_flex().gap_1().children(PERIODS.iter().map(|&(label, secs)| {
                    Button::new(SharedString::from(format!("period-{label}"))).small().ghost().label(label).selected(period == secs).on_click(cx.listener(move |this, _, _, cx| {
                        this.period = secs;
                        this.model.update(cx, |m, cx| m.refresh_analytics(secs, cx));
                    }))
                })),
            )
            .child(card("Tokens Claude Code Recorded on This Mac".into()).children(analytics_rows).child(div().text_xs().text_color(muted).child("Token counts aren't subscription quota: the 5-hour and 7-day windows are on the Accounts tab.")))
            .child(card(format!("Running Sessions · {session_count}")).children(session_rows).when(session_count == 0, |el| el.child(div().text_sm().text_color(muted).child("No claude session is running."))))
            .into_any_element()
    }

    fn confirm_end(&mut self, s: Session, window: &mut Window, cx: &mut Context<Self>) {
        let answer = crate::confirm::Confirm::new(format!("End the session in {}?", s.folder()), "End Session")
            .thing(s.folder().to_string())
            .detail(format!("Claude Code (pid {}) stops as if you quit it; its conversation stays and can be resumed.", s.pid))
            .ask(window, cx);
        cx.spawn(async move |_, _| {
            if answer.await {
                sessions::end(s.pid);
            }
        })
        .detach();
    }

    pub(super) fn render_diagnostics(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let status = |id: &str| m.status(id).cloned();
        let exe = std::env::current_exe().ok();
        let shell = AccountsModel::login_shell();
        let env = |v: &str| std::env::var_os(v).is_some();
        let probe = advice::Probe {
            claude: m.claude.as_deref(),
            login_shell: &shell,
            env: &env,
            status: &status,
            exe: exe.as_deref(),
        };
        let checks = advice::diagnostics(&m.root, &m.home, &m.accounts, &probe);
        v_flex()
            .id("accounts-diagnostics")
            .flex_1()
            .overflow_y_scroll()
            .p_6()
            .gap_2()
            .children(checks.into_iter().map(|c| {
                let (mark, color) = match c.level {
                    Level::Pass => ("✓", cx.theme().success),
                    Level::Warn => ("!", cx.theme().warning),
                    Level::Fail => ("✕", cx.theme().danger),
                    Level::Info => ("i", cx.theme().muted_foreground),
                };
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(div().w(px(16.)).text_color(color).child(mark))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().font_semibold().child(c.title))
                            .child(div().text_xs().whitespace_normal().child(c.detail))
                            .when_some(c.fix, |el, f| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(f),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }
}
