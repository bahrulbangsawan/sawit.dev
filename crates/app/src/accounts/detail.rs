//! The Accounts tab's detail (design: `docs/design/accounts.sawit`), capped at 720 px: the
//! header (avatar, name renamed in place, `cpN`, plan, who it is, Active or Set Active, Open in
//! Terminal, ⋯), the sign-in card while signed out, the usage limits as two meter cards with
//! when they were read and Check Usage, and the details (folder with Reveal and Copy Path, how
//! it runs, the sessions running as it).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::usage::{Snapshot, Window as UsageWindow};
use sawit_core::agent_accounts::{Account, tracking};

use super::SignIn;
use super::list::{account_menu, adopt_button};
use super::page::AccountsPage;
use super::rows;
use super::ui::{self, CONTENT_MAX};

/// One window's card: what it is, "% used", the bar, and when it resets, in words and on the
/// clock. Past warning a card also carries an icon beside its colour.
fn meter_card(
    id: &'static str,
    title: &'static str,
    w: Option<UsageWindow>,
    stale: bool,
    now: f64,
    cx: &App,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let card = v_flex()
        .id(id)
        .flex_1()
        .min_w(px(200.))
        .px_4()
        .py_3p5()
        .gap_2()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().sidebar);
    let head = |icon: Option<Lucide>, color: Hsla| {
        h_flex()
            .child(div().text_sm().text_color(muted).child(title))
            .child(div().flex_1())
            .when_some(icon, |el, i| {
                el.child(Icon::new(i).small().text_color(color))
            })
    };
    match w {
        Some(w) if w.resets_at > now => {
            let used = w.used_percentage;
            let color = ui::meter_color(used, cx);
            let spoken = format!(
                "{title}: {} used, {}",
                rows::pct(&w),
                rows::resets_in(&w, now)
            );
            card.role(Role::Label)
                .aria_label(spoken)
                .child(head(ui::level_icon(used), color))
                .child(
                    h_flex()
                        .items_baseline()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(28.))
                                .line_height(px(32.))
                                .font_medium()
                                .when(stale, |el| el.text_color(muted))
                                .child(rows::pct(&w)),
                        )
                        .child(div().text_sm().text_color(muted).child("used")),
                )
                .child(div().when(stale, |el| el.opacity(0.5)).child(ui::track(
                    (used / 100.0) as f32,
                    color,
                    6.,
                    cx,
                )))
                .child(
                    h_flex()
                        .text_xs()
                        .text_color(muted)
                        .child(capitalized(&rows::resets_in(&w, now)))
                        .child(div().flex_1())
                        .child(sawit_core::date::when_label(w.resets_at as i64, now as i64)),
                )
        }
        // The window ended after the last reading: no bar, which would read as 0% used.
        Some(w) => card
            .role(Role::Label)
            .aria_label(format!("{title}: no data since the reset"))
            .child(head(Some(Lucide::Clock), muted))
            .child(div().text_sm().child(format!(
                "No data since the reset at {}",
                sawit_core::date::when_label(w.resets_at as i64, now as i64)
            )))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("It fills in with the next session's first message."),
            ),
        None => card
            .role(Role::Label)
            .aria_label(format!("{title}: no data yet"))
            .child(head(None, muted))
            .child(div().text_sm().child("No data yet"))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("Start a claude session as this account and send one message."),
            ),
    }
}

fn capitalized(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

impl AccountsPage {
    pub(super) fn render_detail(
        &mut self,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let m = self.model.read(cx);
        let Some(a) = self.selected(m).cloned() else {
            return self.render_no_accounts(cx);
        };
        let now = sawit_core::github::now() as f64;
        let list = m.list();
        let ix = list.iter().position(|x| x.id == a.id).unwrap_or(0);
        let count = list.len();
        let is_active = m.active().map(|x| x.id.clone()) == Some(a.id.clone());
        let sign_in = m.sign_in.get(&a.id).cloned().unwrap_or(SignIn::Unknown);
        let plan = m.plan.get(&a.id).cloned();
        let snapshot = m.usage.get(&a.id).cloned();
        let login = m.login.as_ref().filter(|l| l.account == a.id).map(|l| {
            (
                l.phase.clone(),
                l.manual_url.clone(),
                l.callback_url.clone(),
                l.left(),
            )
        });
        let checking = m.checking_usage.as_deref() == Some(a.id.as_str());
        let sessions: Vec<_> = m
            .sessions()
            .into_iter()
            .filter(|s| s.account == a.id)
            .collect();
        let others: Vec<(String, String)> = list
            .iter()
            .filter(|x| x.id != a.id)
            .map(|x| (x.id.clone(), x.name.clone()))
            .collect();
        let home = m.home.to_string_lossy().to_string();
        let who = match &sign_in {
            SignIn::Known(s) if s.signed_in => s.identity().unwrap_or_else(|| "Signed in".into()),
            SignIn::Known(_) => "Signed out".into(),
            SignIn::Checking => "Checking the sign-in…".into(),
            SignIn::Failed(e) => e.clone(),
            SignIn::Unknown => String::new(),
        };
        let signed_out = matches!(&sign_in, SignIn::Known(s) if !s.signed_in);
        let header = self.render_detail_header(
            &a,
            DetailHead {
                ix,
                count,
                is_active,
                plan,
                who,
            },
            window,
            cx,
        );
        let usage =
            self.render_limits(&a, snapshot.as_ref(), checking, signed_out, narrow, now, cx);
        let details = self.render_details(&a, &home, sessions, others, cx);
        div()
            .id("account-detail")
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_y_scroll()
            .child(
                v_flex()
                    .w_full()
                    .max_w(CONTENT_MAX)
                    .p_6()
                    .gap_6()
                    .child(header)
                    .when(signed_out || login.is_some(), |el| {
                        el.child(self.render_login(&a, login, window, cx))
                    })
                    .child(usage)
                    .child(details),
            )
            .into_any_element()
    }

    /// No accounts: what an account is, and both ways to get one.
    fn render_no_accounts(&self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let adoptable = sawit_core::agent_accounts::discover(
            sawit_core::agent_accounts::AgentKind::ClaudeCode,
            &m.home,
        );
        div()
            .id("account-detail")
            .flex_1()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                ui::empty_state(
                    Lucide::Users,
                    "No Claude Code accounts yet",
                    "An account is a Claude Code folder with its own sign-in. Add one and sign in, or adopt a ~/.claude-* folder you already have.",
                    cx,
                )
                .child(
                    h_flex()
                        .mt_2()
                        .gap_2()
                        .child(
                            Button::new("account-add-empty")
                                .small()
                                .primary()
                                .icon(Lucide::Plus)
                                .label("Add Account")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.adding = true;
                                    this.new_name.read(cx).focus_handle(cx).focus(window, cx);
                                    cx.notify();
                                })),
                        )
                        .when(!adoptable.is_empty(), |el| el.child(adopt_button(adoptable))),
                ),
            )
            .into_any_element()
    }

    fn render_detail_header(
        &self,
        a: &Account,
        head: DetailHead,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let DetailHead {
            ix,
            count,
            is_active,
            plan,
            who,
        } = head;
        let name = if self.renaming {
            let id = a.id.clone();
            let save = cx.listener(move |this, _: &ClickEvent, _, cx| {
                let name = this.rename.read(cx).value().to_string();
                this.model.update(cx, |m, cx| m.rename(&id, &name, cx));
                this.renaming = false;
            });
            let _ = window;
            h_flex()
                .gap_1()
                .child(div().w(px(200.)).child(Input::new(&self.rename).small()))
                .child(
                    Button::new("account-rename-save")
                        .small()
                        .primary()
                        .label("Rename")
                        .on_click(save),
                )
                .child(
                    Button::new("account-rename-cancel")
                        .small()
                        .ghost()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.renaming = false;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_2()
                .min_w_0()
                .child(
                    div()
                        .text_lg()
                        .font_semibold()
                        .min_w_0()
                        .truncate()
                        .child(a.name.clone()),
                )
                .child(ui::badge(format!("cp{}", ix + 1), cx))
                .when_some(plan, |el, p| {
                    el.child(ui::pill("account-plan", ui::Tone::Neutral, None, p, cx))
                })
                .into_any_element()
        };
        let page = cx.entity();
        let menu_account = a.clone();
        let id = a.id.clone();
        let terminal_account = a.clone();
        h_flex()
            .flex_wrap()
            .gap_3()
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(220.))
                    .gap_3()
                    .child(ui::avatar(&a.name, &a.color, 40., cx))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .child(name)
                            .when(!who.is_empty(), |el| {
                                let tip = who.clone();
                                el.child(
                                    div()
                                        .id("account-who")
                                        .text_sm()
                                        .text_color(muted)
                                        .truncate()
                                        .child(who)
                                        .tooltip(move |window, cx| {
                                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                                .build(window, cx)
                                        }),
                                )
                            }),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(if is_active {
                        ui::pill(
                            "account-active",
                            ui::Tone::Good,
                            Some(Lucide::Check),
                            "Active",
                            cx,
                        )
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "New Claude Code sessions use this account",
                            )
                            .build(window, cx)
                        })
                        .into_any_element()
                    } else {
                        Button::new("account-set-active")
                            .small()
                            .outline()
                            .icon(Lucide::Check)
                            .label("Set Active")
                            .tooltip("New Claude Code sessions use this account")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.model.update(cx, |m, cx| m.set_active(&id, cx))
                            }))
                            .into_any_element()
                    })
                    .child(
                        Button::new("account-open-terminal")
                            .small()
                            .primary()
                            .icon(Lucide::SquareTerminal)
                            .label("Open in Terminal")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_terminal(&terminal_account, cx)
                            })),
                    )
                    .child(
                        Button::new("account-more")
                            .small()
                            .ghost()
                            .icon(Lucide::Ellipsis)
                            .accessibility_label(format!("More for {}", a.name))
                            .tooltip("More")
                            .dropdown_menu(move |menu, _, _| {
                                account_menu(menu, &page, &menu_account, ix, count, is_active)
                            }),
                    ),
            )
    }

    /// The usage limits: a header with when they were read and Check Usage, then the two
    /// windows' cards side by side (stacked when narrow). Tracking off or overridden shows one
    /// card that says what tracking does, with the action that turns it on.
    #[allow(clippy::too_many_arguments)]
    fn render_limits(
        &self,
        a: &Account,
        snapshot: Option<&Snapshot>,
        checking: bool,
        signed_out: bool,
        narrow: bool,
        now: f64,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let state = tracking::state(a);
        let observed = snapshot.and_then(|s| rows::observed(s, now));
        let stale = observed.as_ref().is_some_and(|(_, stale)| *stale);
        let id = a.id.clone();
        let header = ui::section_header(Lucide::Gauge, "Usage limits", cx)
            .when(state == tracking::Tracking::On, |el| {
                el.when_some(observed.clone(), |el, (when, stale)| {
                    el.child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(if stale { cx.theme().warning } else { muted })
                            .when(stale, |el| el.child(Icon::new(Lucide::Clock).xsmall()))
                            .child(capitalized(&when.replace("as of", "updated"))),
                    )
                })
                .child(div().flex_1())
                .child(
                    Button::new("account-check-usage")
                        .xsmall()
                        .ghost()
                        .icon(Lucide::RefreshCw)
                        .label(if checking { "Checking…" } else { "Check Usage" })
                        .loading(checking)
                        .disabled(checking || signed_out)
                        .tooltip(if signed_out {
                            "Sign in first"
                        } else {
                            "Claude Code sends one tiny request as this account (a few hundred tokens)"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.check_usage(&id, cx))
                        })),
                )
            });
        let off_card = |icon: Lucide,
                        title: &'static str,
                        text: &'static str,
                        action: &'static str,
                        cx: &mut Context<Self>| {
            let a = a.clone();
            h_flex()
                .gap_3()
                .px_4()
                .py_3()
                .rounded(cx.theme().radius_lg)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().sidebar)
                .child(ui::icon_tile(icon, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().font_medium().child(title))
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(text),
                        ),
                )
                .child(
                    Button::new("account-track")
                        .small()
                        .outline()
                        .label(action)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm_tracking(a.clone(), window, cx)
                        })),
                )
        };
        let body = match state {
            tracking::Tracking::Off => off_card(
                Lucide::Gauge,
                "Usage tracking is off",
                "Turned on, this account's status line reports its 5-hour and 7-day limits to Sawit. Nothing leaves this Mac.",
                "Turn On Tracking…",
                cx,
            )
            .into_any_element(),
            tracking::Tracking::Overridden => off_card(
                Lucide::TriangleAlert,
                "Tracking was overridden",
                "This account's statusLine no longer runs Sawit, so no usage arrives.",
                "Turn On Again…",
                cx,
            )
            .into_any_element(),
            tracking::Tracking::On => {
                let five = snapshot.and_then(|s| s.five_hour);
                let seven = snapshot.and_then(|s| s.seven_day);
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .gap_3()
                            .when(narrow, |el| el.flex_col())
                            .child(meter_card("account-five-hour", "5-hour window", five, stale, now, cx))
                            .child(meter_card("account-seven-day", "7-day window", seven, stale, now, cx)),
                    )
                    .when(stale, |el| {
                        el.child(div().text_xs().text_color(muted).child(
                            "Usage from claude.ai or other devices since then isn't included.",
                        ))
                    })
                    .into_any_element()
            }
        };
        v_flex().child(header).child(body)
    }

    /// The details box: the folder with Reveal and Copy Path, how terminals run the account,
    /// and the sessions running as it with Continue On….
    fn render_details(
        &self,
        a: &Account,
        home: &str,
        sessions: Vec<sawit_core::agent_accounts::sessions::Session>,
        others: Vec<(String, String)>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let mono = cx.theme().mono_font_family.clone();
        let short = a.dir.display().to_string().replacen(home, "~", 1);
        let key = |t: &'static str| {
            div()
                .w(px(120.))
                .flex_none()
                .text_sm()
                .text_color(muted)
                .child(t)
        };
        let value = |t: String| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_family(mono.clone())
                .text_xs()
                .child(t)
        };
        let (reveal, copy) = (a.dir.clone(), a.dir.display().to_string());
        let mut items: Vec<AnyElement> = vec![
            h_flex()
                .px_4()
                .py_2()
                .gap_2()
                .child(key("Folder"))
                .child(value(short.clone()))
                .child(
                    Button::new("account-reveal")
                        .xsmall()
                        .ghost()
                        .icon(Lucide::FolderOpen)
                        .label("Reveal")
                        .tooltip("Reveal in Finder")
                        .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                )
                .child(
                    Button::new("account-copy-path")
                        .xsmall()
                        .ghost()
                        .icon(Lucide::Copy)
                        .label("Copy Path")
                        .on_click(move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                        }),
                )
                .into_any_element(),
            h_flex()
                .px_4()
                .py_2p5()
                .gap_2()
                .child(key("Runs with"))
                .child(value(if a.is_default {
                    "CLAUDE_CONFIG_DIR unset (the default account)".into()
                } else {
                    format!("CLAUDE_CONFIG_DIR={short}")
                }))
                .into_any_element(),
            h_flex()
                .px_4()
                .py_2p5()
                .gap_2()
                .child(key("Sessions"))
                .child(div().flex_1().text_sm().child(match sessions.len() {
                    0 => "None running as this account".to_string(),
                    1 => "1 running as this account".to_string(),
                    n => format!("{n} running as this account"),
                }))
                .into_any_element(),
        ];
        let page = cx.entity();
        for s in sessions {
            let targets = others.clone();
            let (page, s2) = (page.clone(), s.clone());
            items.push(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .child(Icon::new(Lucide::SquareTerminal).small().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(s.folder().to_string()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(s.status.clone().unwrap_or_default()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("detail-hand-off-{}", s.pid)))
                            .xsmall()
                            .outline()
                            .label("Continue On…")
                            .disabled(targets.is_empty())
                            .dropdown_menu(move |menu, _, _| {
                                targets.iter().fold(menu, |menu, (id, name)| {
                                    let (page, id, s) = (page.clone(), id.clone(), s2.clone());
                                    menu.item(
                                        PopupMenuItem::new(name.clone())
                                            .icon(Lucide::Users)
                                            .on_click(move |_, _, cx| {
                                                page.update(cx, |this, cx| {
                                                    let opened = this.model.update(cx, |m, cx| {
                                                        m.hand_off(&s, &id, cx)
                                                    });
                                                    if let Some((title, command, env)) = opened {
                                                        cx.emit(
                                                            super::AccountsEvent::OpenTerminal {
                                                                title,
                                                                command,
                                                                env,
                                                            },
                                                        );
                                                    }
                                                })
                                            }),
                                    )
                                })
                            }),
                    )
                    .into_any_element(),
            );
        }
        v_flex()
            .child(ui::section_header(Lucide::Folder, "Details", cx))
            .child(ui::group_box(cx).children(ui::rows(items, cx)))
    }
}

/// What the detail's header shows besides the account itself.
struct DetailHead {
    ix: usize,
    count: usize,
    is_active: bool,
    plan: Option<String>,
    who: String,
}
