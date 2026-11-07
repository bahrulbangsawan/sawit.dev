//! The Accounts page's Usage tab (token analytics per account and model, and the running
//! sessions with Continue On… and End…) and Diagnostics tab; and the model's limit alerts.

use std::collections::BTreeMap;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::advice::{self, Level};
use sawit_core::agent_accounts::analytics::{self, Totals};
use sawit_core::agent_accounts::sessions::{self, Session};

use super::page::AccountsPage;
use super::ui::{self, Tone};
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
        let accounts: BTreeMap<String, (String, String)> = m
            .list()
            .iter()
            .map(|a| (a.id.clone(), (a.name.clone(), a.color.clone())))
            .collect();
        let period = self.period;
        let period_name = PERIODS
            .iter()
            .find(|(_, s)| *s == period)
            .map(|(l, _)| l.to_lowercase())
            .unwrap_or_default();
        let versus = format!("vs the {period_name} before");
        let periods = ButtonGroup::new("accounts-period")
            .outline()
            .compact()
            .small()
            .children(PERIODS.iter().map(|&(label, secs)| {
                Button::new(SharedString::from(format!("period-{label}")))
                    .label(label)
                    .selected(period == secs)
            }))
            .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                let Some(&ix) = picked.first() else { return };
                let secs = PERIODS[ix.min(PERIODS.len() - 1)].1;
                this.period = secs;
                this.model.update(cx, |m, cx| m.refresh_analytics(secs, cx));
            }));
        let tile =
            |id: &'static str, label: &'static str, value: String, change: String, cx: &App| {
                v_flex()
                    .id(id)
                    .role(Role::Label)
                    .aria_label(format!("{label}: {value}, {change}"))
                    .flex_1()
                    .min_w(px(160.))
                    .px_4()
                    .py_3()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().sidebar)
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(div().text_2xl().font_medium().child(value))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(change),
                    )
            };
        let analytics: AnyElement = match &m.analytics {
            None => h_flex()
                .gap_2()
                .py_6()
                .text_sm()
                .text_color(muted)
                .child(Icon::new(Lucide::LoaderCircle).small())
                .child("Reading Claude Code's conversation files…")
                .into_any_element(),
            Some((now_rows, before_rows)) if now_rows.is_empty() => ui::empty_state(
                Lucide::ChartColumn,
                format!("No usage in the last {period_name}"),
                "Token counts come from the conversation files Claude Code keeps on this Mac. Run a session, or pick a longer period.",
                cx,
            )
            .when(before_rows.is_empty(), |el| el)
            .into_any_element(),
            Some((now_rows, before_rows)) => {
                let now_all = Totals::sum(now_rows.iter().map(|(_, _, t)| t));
                let before_all = Totals::sum(before_rows.iter().map(|(_, _, t)| t));
                let change = |now: i64, before: i64| match analytics::change_pct(now, before) {
                    Some(p) => format!("{p:+.0}% {versus}"),
                    None => "Nothing to compare with yet".into(),
                };
                let hit = |t: &Totals| t.cache_hit_rate().map(|r| r * 100.0);
                let hit_change = match (hit(&now_all), hit(&before_all)) {
                    (Some(a), Some(b)) => format!("{:+.0} points {versus}", a - b),
                    _ => "Nothing to compare with yet".into(),
                };
                let tiles = h_flex()
                    .flex_wrap()
                    .gap_3()
                    .child(tile("usage-tokens", "Tokens", short(now_all.total()), change(now_all.total(), before_all.total()), cx))
                    .child(tile("usage-requests", "Requests", now_all.requests.to_string(), change(now_all.requests, before_all.requests), cx))
                    .child(tile(
                        "usage-cache",
                        "Cache hit rate",
                        hit(&now_all).map(|r| format!("{r:.0}%")).unwrap_or_else(|| "—".into()),
                        hit_change,
                        cx,
                    ));
                // One row per account and model: numbers right-aligned and tabular.
                let num = |t: String| div().w(px(84.)).flex_none().text_right().child(t);
                let head = h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child(div().flex_1().min_w_0().child("Account"))
                    .child(div().w(px(120.)).flex_none().child("Model"))
                    .child(num("Input".into()))
                    .child(num("Output".into()))
                    .child(num("Cache read".into()))
                    .child(num("Requests".into()));
                let mut table: Vec<AnyElement> = vec![head.into_any_element()];
                for (id, model, t) in now_rows {
                    let (name, color) = accounts
                        .get(id)
                        .cloned()
                        .unwrap_or_else(|| (id.clone(), "#888888".into()));
                    table.push(
                        h_flex()
                            .px_4()
                            .py_2()
                            .gap_2()
                            .text_sm()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_2()
                                    .child(ui::avatar(&name, &color, 20., cx))
                                    .child(div().truncate().child(name)),
                            )
                            .child(div().w(px(120.)).flex_none().truncate().text_color(muted).child(model.clone()))
                            .child(num(short(t.input)))
                            .child(num(short(t.output)))
                            .child(num(short(t.cache_read)))
                            .child(num(t.requests.to_string()))
                            .into_any_element(),
                    );
                }
                v_flex()
                    .gap_6()
                    .child(tiles)
                    .child(
                        v_flex()
                            .child(ui::section_header(Lucide::ChartColumn, "By account and model", cx))
                            .child(ui::group_box(cx).children(ui::rows(table, cx))),
                    )
                    .into_any_element()
            }
        };
        let list = m.sessions();
        let others: Vec<(String, String)> = m
            .list()
            .iter()
            .map(|a| (a.id.clone(), a.name.clone()))
            .collect();
        let session_rows: Vec<AnyElement> =
            list.into_iter()
                .map(|s| {
                    let s2 = s.clone();
                    let targets: Vec<(String, String)> = others
                        .iter()
                        .filter(|(id, _)| *id != s.account)
                        .cloned()
                        .collect();
                    let page = cx.entity();
                    let (name, color) = accounts
                        .get(&s.account)
                        .cloned()
                        .unwrap_or_else(|| (String::new(), "#888888".into()));
                    let state = match s.status.as_deref() {
                        Some("busy") => "Working".to_string(),
                        Some("idle") | Some("waiting") => "Waiting for you".to_string(),
                        Some(other) => other.to_string(),
                        None => String::new(),
                    };
                    h_flex()
                        .px_4()
                        .py_2()
                        .gap_2()
                        .child(ui::avatar(&name, &color, 20., cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_sm().truncate().child(s.folder().to_string()))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .truncate()
                                        .child(format!("{name} · {state} · pid {}", s.pid)),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("hand-off-{}", s.pid)))
                                .xsmall()
                                .outline()
                                .label("Continue On…")
                                .disabled(targets.is_empty())
                                .dropdown_menu(move |menu, _, _| {
                                    targets.iter().fold(menu, |menu, (id, name)| {
                                        let (page, id, s) = (page.clone(), id.clone(), s2.clone());
                                        menu.item(
                                            PopupMenuItem::new(name.clone())
                                                .icon(crate::menu_icons::ACCOUNT)
                                                .on_click(move |_, _, cx| {
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
                                                }),
                                        )
                                    })
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("end-{}", s.pid)))
                                .xsmall()
                                .ghost()
                                .icon(Lucide::CircleStop)
                                .label("End…")
                                .disabled(s.busy())
                                .tooltip(if s.busy() {
                                    "Busy: end it in its terminal"
                                } else {
                                    "Stop this session (SIGTERM)"
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_end(s.clone(), window, cx)
                                })),
                        )
                        .into_any_element()
                })
                .collect();
        let session_count = session_rows.len();
        let sessions = v_flex()
            .child(
                ui::section_header(Lucide::SquareTerminal, "Running sessions", cx).child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(session_count.to_string()),
                ),
            )
            .child(if session_count == 0 {
                ui::group_box(cx)
                    .child(ui::setting_row(
                        Lucide::SquareTerminal,
                        "No claude session is running",
                        "Open in Terminal on the Accounts tab starts one as any account.",
                        div(),
                        false,
                        cx,
                    ))
                    .into_any_element()
            } else {
                ui::group_box(cx)
                    .children(ui::rows(session_rows, cx))
                    .into_any_element()
            });
        let (scroll, content) = ui::tab_body("accounts-usage", px(960.));
        scroll
            .child(
                content
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .child(periods)
                            .child(div().flex_1())
                            .child(div().text_xs().text_color(muted).child(
                                "Tokens Claude Code recorded on this Mac. Plan limits are on the Accounts tab.",
                            )),
                    )
                    .child(analytics)
                    .child(sessions),
            )
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
        let count = |l: Level| checks.iter().filter(|c| c.level == l).count();
        let (pass, warn, fail, info) = (
            count(Level::Pass),
            count(Level::Warn),
            count(Level::Fail),
            count(Level::Info),
        );
        let report: String = checks
            .iter()
            .map(|c| {
                let mark = match c.level {
                    Level::Pass => "pass",
                    Level::Warn => "warning",
                    Level::Fail => "fail",
                    Level::Info => "note",
                };
                match &c.fix {
                    Some(f) => format!("[{mark}] {}: {} {f}", c.title, c.detail),
                    None => format!("[{mark}] {}: {}", c.title, c.detail),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let plural = |n: usize, one: &str, many: &str| {
            if n == 1 {
                format!("1 {one}")
            } else {
                format!("{n} {many}")
            }
        };
        let summary = h_flex()
            .flex_wrap()
            .gap_2()
            .child(ui::pill(
                "diag-pass",
                Tone::Good,
                Some(Lucide::CircleCheck),
                plural(pass, "passed", "passed"),
                cx,
            ))
            .when(warn > 0, |el| {
                el.child(ui::pill(
                    "diag-warn",
                    Tone::Warn,
                    Some(Lucide::TriangleAlert),
                    plural(warn, "warning", "warnings"),
                    cx,
                ))
            })
            .when(fail > 0, |el| {
                el.child(ui::pill(
                    "diag-fail",
                    Tone::Bad,
                    Some(Lucide::CircleX),
                    plural(fail, "failed", "failed"),
                    cx,
                ))
            })
            .when(info > 0, |el| {
                el.child(ui::pill(
                    "diag-info",
                    Tone::Neutral,
                    Some(Lucide::Info),
                    plural(info, "note", "notes"),
                    cx,
                ))
            })
            .child(div().flex_1())
            .child(
                Button::new("diag-again")
                    .xsmall()
                    .outline()
                    .icon(Lucide::RefreshCw)
                    .label("Run Again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| m.check_all(cx));
                        cx.notify();
                    })),
            )
            .child(
                Button::new("diag-copy")
                    .xsmall()
                    .outline()
                    .icon(Lucide::Copy)
                    .label("Copy Report")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(report.clone()))
                    }),
            );
        let rows: Vec<AnyElement> = checks
            .into_iter()
            .enumerate()
            .map(|(ix, c)| {
                // GPUI shows assistive technology no plain text: each check says itself.
                let word = match c.level {
                    Level::Pass => "Passed",
                    Level::Warn => "Warning",
                    Level::Fail => "Failed",
                    Level::Info => "Note",
                };
                let spoken = match &c.fix {
                    Some(f) => format!("{word}: {}: {} {f}", c.title, c.detail),
                    None => format!("{word}: {}: {}", c.title, c.detail),
                };
                let (icon, color) = match c.level {
                    Level::Pass => (Lucide::CircleCheck, cx.theme().success),
                    Level::Warn => (Lucide::TriangleAlert, cx.theme().warning),
                    Level::Fail => (Lucide::CircleX, cx.theme().danger),
                    Level::Info => (Lucide::Info, cx.theme().muted_foreground),
                };
                h_flex()
                    .id(("accounts-check", ix))
                    .role(Role::Label)
                    .aria_label(spoken)
                    .gap_3()
                    .px_4()
                    .py_2p5()
                    .items_start()
                    .child(
                        div()
                            .pt_0p5()
                            .child(Icon::new(icon).small().text_color(color)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(div().text_sm().font_medium().child(c.title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .whitespace_normal()
                                    .child(c.detail),
                            )
                            .when_some(c.fix, |el, f| {
                                el.child(
                                    h_flex()
                                        .mt_1()
                                        .gap_1()
                                        .text_xs()
                                        .child(
                                            Icon::new(Lucide::Wrench)
                                                .xsmall()
                                                .text_color(cx.theme().muted_foreground),
                                        )
                                        .child(div().whitespace_normal().child(f)),
                                )
                            }),
                    )
                    .into_any_element()
            })
            .collect();
        let (scroll, content) = ui::tab_body("accounts-diagnostics", ui::CONTENT_MAX);
        scroll
            .child(
                content
                    .gap_4()
                    .child(summary)
                    .child(ui::group_box(cx).children(ui::rows(rows, cx))),
            )
            .into_any_element()
    }
}
