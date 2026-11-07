//! The detail's usage limits (two meter cards, when they were read, Check Usage) and its
//! details (the folder, how it runs, the sessions running as it).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::usage::Snapshot;
use sawit_core::agent_accounts::{Account, tracking};

use super::{capitalized, meter_card};
use crate::accounts::page::AccountsPage;
use crate::accounts::rows;
use crate::accounts::ui;

impl AccountsPage {
    /// The usage limits: a header with when they were read and Check Usage, then the two
    /// windows' cards side by side (stacked when narrow). Tracking off or overridden shows one
    /// card that says what tracking does, with the action that turns it on.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_limits(
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
    pub(super) fn render_details(
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
                                            .icon(crate::menu_icons::ACCOUNT)
                                            .on_click(move |_, _, cx| {
                                                page.update(cx, |this, cx| {
                                                    let opened = this.model.update(cx, |m, cx| {
                                                        m.hand_off(&s, &id, cx)
                                                    });
                                                    if let Some((title, command, env)) = opened {
                                                        cx.emit(
                                                            crate::accounts::AccountsEvent::OpenTerminal {
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
