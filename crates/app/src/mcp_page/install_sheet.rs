//! The install sheet: a server's clients grouped by app, each with its status, and Install,
//! Remove and Sign In for the ticked ones.

use std::collections::HashSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::mcp_auth::SignIn;
use sawit_core::mcp_catalog::{Auth, SERVERS};
use sawit_core::mcp_clients::{Category, Client, Spec, Status, snippet};
use sawit_core::package_manager::{PackageManager, Use};

use super::logos::{client_logo, server_logo};
use super::{InstallSheet, McpPage, Pair, Sheet, handover_note};
use crate::catalog::tilde;
use crate::git::account;

impl McpPage {
    pub(super) fn render_install_sheet(
        &self,
        sheet: &InstallSheet,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let s = sheet.server;
        let server = &SERVERS[s];
        let spec = self.spec(s);
        let selected = &sheet.selected;
        // What Install and Remove act on.
        let to_install = self.selected_for(false).len();
        let to_remove = self.selected_for(true).len();
        let to_sign_in: Vec<Pair> = self
            .pending_sign_ins(Some(s))
            .into_iter()
            .filter(|(_, t)| selected.contains(t))
            .collect();
        let busy = (0..self.targets.len()).any(|t| self.busy.contains(&(s, t)));
        let blocked = self.install_blocked(s, cx);
        let github_gate = self.github_gate(s, cx);
        let pm_menu = self
            .uses_runner(s)
            .then(|| {
                crate::package_managers::menu(
                    "mcp-pm",
                    &self.pms,
                    Use::Run,
                    self.pm,
                    cx.listener(|this, pm: &PackageManager, _, cx| {
                        this.pm = Some(*pm);
                        crate::package_managers::remember(*pm);
                        this.recompute();
                        cx.notify();
                    }),
                )
            })
            .flatten();

        // Targets grouped under their client: agents first, then editors.
        let mut groups: Vec<(Client, Vec<usize>)> = Vec::new();
        for (t, target) in self.targets.iter().enumerate() {
            match groups.iter_mut().find(|(c, _)| *c == target.client) {
                Some((_, ts)) => ts.push(t),
                None => groups.push((target.client, vec![t])),
            }
        }
        groups.sort_by_key(|(c, _)| {
            (
                c.category() == Category::Editor,
                Client::ALL.iter().position(|x| x == c),
            )
        });

        let select = |label: &'static str, id: &'static str, pick: fn(&Status) -> bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .label(label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let n = this.targets.len();
                    let chosen: HashSet<usize> = (0..n)
                        .filter(|t| this.status.get(&(s, *t)).is_some_and(pick))
                        .collect();
                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                        sheet.selected = chosen;
                    }
                    cx.notify();
                }))
        };

        let select_buttons = [
            select("All", "mcp-sel-all", |_| true),
            select("Installed", "mcp-sel-installed", |st| {
                matches!(st, Status::Installed | Status::Outdated(_))
            }),
            select("None", "mcp-sel-none", |_| false),
        ];
        let list = v_flex().children(groups.into_iter().map(|(client, ts)| {
            v_flex()
                .child(
                    h_flex()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .bg(theme.secondary)
                        .border_b_1()
                        .border_color(theme.border)
                        .child(client_logo(client, 18., cx))
                        .child(div().text_xs().font_semibold().child(client.name())),
                )
                .children(
                    ts.into_iter()
                        .map(|t| self.render_target_row(s, t, &spec, selected.contains(&t), cx)),
                )
        }));

        let title = format!("Install {}", server.name);
        v_flex()
            .id("mcp-sheet")
            .role(Role::Dialog)
            .aria_label(title.clone())
            .w(px(720.))
            .max_w_full()
            .max_h_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .shadow_lg()
            .child(
                v_flex()
                    .p_4()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(server_logo(server, cx))
                            .child(div().text_base().font_semibold().flex_1().child(title))
                            .child(
                                Button::new("mcp-sheet-close")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::X)
                                    .accessibility_label("Close")
                                    .tooltip("Close")
                                    .on_click(cx.listener(|this, _, _, cx| this.close_sheet(cx))),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(theme.mono_font_family.clone())
                            .text_color(muted)
                            .whitespace_normal()
                            .child(server.address_with(&self.region, self.run_pm())),
                    )
                    .when_some(sheet.region.as_ref(), |el, region| {
                        el.child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .child(div().text_sm().child("Region"))
                                .child(div().w(px(180.)).child(Input::new(region).small()))
                                .child(if sheet.region_invalid {
                                    div()
                                        .text_xs()
                                        .text_color(theme.danger)
                                        .child("Not an AWS Region (like eu-west-1).")
                                } else {
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child("The AWS Region the server works in.")
                                }),
                        )
                    })
                    .when_some(pm_menu, |el, menu| {
                        el.child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .child(div().text_sm().child("Run with"))
                                .child(menu)
                                .child(div().text_xs().text_color(muted).child(
                                    if server.package().is_some() {
                                        "The package manager written into each client's config to start it."
                                    } else if server.auth == Auth::GitHub {
                                        "Runs the mcp-remote bridge for the clients that get Sawit's sign-in through it."
                                    } else {
                                        "Runs the mcp-remote bridge for clients that only start programs."
                                    },
                                )),
                        )
                    }),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().text_xs().text_color(muted).pr_1().child("Select"))
                    .children(select_buttons)
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{} selected", selected.len())),
                    ),
            )
            .child(
                div()
                    .id("mcp-sheet-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .when(!to_sign_in.is_empty(), |el| {
                        el.child(
                            Button::new("mcp-sheet-sign-in")
                                .outline()
                                .small()
                                .icon(Lucide::KeyRound)
                                .label(format!("Sign In ({})", to_sign_in.len()))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.start_sign_in(to_sign_in.clone(), window, cx)
                                })),
                        )
                    })
                    .when(github_gate.is_some(), |el| {
                        el.child(account::sign_in_link("mcp-sheet-github-sign-in"))
                    })
                    .child(div().flex_1())
                    .child(
                        Button::new("mcp-sheet-remove")
                            .debug_selector(|| "mcp-sheet-remove".into())
                            .danger()
                            .outline()
                            .small()
                            .label(format!("Remove from {to_remove}"))
                            .disabled(to_remove == 0 || busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_remove_selected(window, cx)
                            })),
                    )
                    .child({
                        let install = Button::new("mcp-sheet-install")
                            .debug_selector(|| "mcp-sheet-install".into())
                            .primary()
                            .small()
                            .label(format!("Install to {to_install}"))
                            .loading(busy)
                            .disabled(to_install == 0 || busy || blocked)
                            .on_click(cx.listener(|this, _, _, cx| this.install_selected(cx)));
                        // Off with why while signed out, as every GitHub control is.
                        if github_gate.is_some() {
                            account::gate(install, self.github.read(cx), "install", cx)
                        } else {
                            install
                        }
                    }),
            )
            .into_any_element()
    }

    fn render_target_row(
        &self,
        s: usize,
        t: usize,
        spec: &Spec,
        checked: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let target = &self.targets[t];
        let pair = (s, t);
        let state = self
            .status
            .get(&pair)
            .cloned()
            .unwrap_or(Status::NotInstalled);
        let signed = self
            .sign_ins
            .get(&t)
            .and_then(|m| m.get(SERVERS[s].id))
            .copied();
        let elsewhere = self.other_region(pair);
        let (badge, color): (String, Hsla) = match &state {
            Status::Installed if self.needs_sign_in(pair) => {
                ("Needs sign-in".into(), theme.warning)
            }
            Status::Installed
                if signed == Some(SignIn::SignedIn) && SERVERS[s].auth == Auth::Browser =>
            {
                ("Signed in".into(), theme.success)
            }
            Status::Installed => ("Installed".into(), theme.success),
            Status::Outdated(_) if let Some(region) = &elsewhere => {
                (format!("In {region}"), theme.info)
            }
            Status::Outdated(_) => ("Other version".into(), theme.warning),
            Status::NotInstalled if self.bridge_blocked(pair) => {
                ("Needs Node.js".into(), theme.warning)
            }
            Status::NotInstalled => ("Not installed".into(), muted),
            Status::Unreadable(_) => ("Edit manually".into(), theme.danger),
            Status::Manual => ("Paste by hand".into(), muted),
        };
        let detail = match &state {
            Status::Outdated(other) => Some(format!("Points to {other}")),
            Status::Unreadable(why) => Some(why.clone()),
            _ if SERVERS[s].auth == Auth::GitHub => Some(handover_note(target.client).into()),
            _ => None,
        };
        let via = match self.cli_for(target.client) {
            Some(_) => format!("via {}", target.client.cli().unwrap_or_default()),
            None if !target.client.editable() => "by hand".into(),
            None => "edits file".into(),
        };
        let manual = matches!(state, Status::Manual | Status::Unreadable(_))
            || (!target.client.editable() && state == Status::Installed);
        let busy = self.busy.contains(&pair);
        let text = snippet(target, spec);
        let profile_label = if target.profile == "default" && target.client.profile_var().is_none()
        {
            "This Mac".to_string()
        } else {
            target.profile.clone()
        };
        v_flex()
            .px_3()
            .py_1p5()
            .gap_0p5()
            .border_b_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().w(px(150.)).flex_none().child(
                            Checkbox::new(("mcp-target", t))
                                .label(profile_label)
                                .checked(checked)
                                .disabled(manual)
                                .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                                        if *checked {
                                            sheet.selected.insert(t);
                                        } else {
                                            sheet.selected.remove(&t);
                                        }
                                    }
                                    cx.notify();
                                })),
                        ),
                    )
                    .child(
                        div()
                            .id(("mcp-config", t))
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .font_family(theme.mono_font_family.clone())
                            .tooltip({
                                let full = target.config.display().to_string();
                                move |window, cx| Tooltip::new(full.clone()).build(window, cx)
                            })
                            .child(tilde(&target.config)),
                    )
                    .child(div().text_xs().text_color(muted).child(via))
                    .child(
                        h_flex()
                            .gap_1()
                            .w(px(104.))
                            .flex_none()
                            .text_xs()
                            .text_color(color)
                            .when(busy, |el| el.child(Spinner::new().xsmall().color(muted)))
                            .when(!busy, |el| {
                                el.child(div().size(px(6.)).rounded_full().bg(color))
                            })
                            .child(badge),
                    )
                    .child(
                        div()
                            .w(px(92.))
                            .flex_none()
                            .flex()
                            .justify_end()
                            .when(manual, |el| {
                                el.child(
                                    Button::new(("mcp-copy", t))
                                        .ghost()
                                        .xsmall()
                                        .icon(Lucide::Copy)
                                        .label("Snippet")
                                        .tooltip("Copy the entry to paste into the config")
                                        .on_click(move |_, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                text.clone(),
                                            ))
                                        }),
                                )
                            })
                            .when(!manual && self.needs_sign_in(pair), |el| {
                                el.child(
                                    Button::new(("mcp-row-sign-in", t))
                                        .ghost()
                                        .xsmall()
                                        .label("Sign In")
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.start_sign_in(vec![pair], window, cx)
                                        })),
                                )
                            }),
                    ),
            )
            .when_some(detail, |el, d| {
                el.child(
                    div()
                        .pl(px(158.))
                        .text_xs()
                        .text_color(muted)
                        .whitespace_normal()
                        .child(d),
                )
            })
            .when_some(self.errors.get(&pair).cloned(), |el, e| {
                el.child(
                    div()
                        .pl(px(158.))
                        .text_xs()
                        .text_color(theme.danger)
                        .whitespace_normal()
                        .child(e),
                )
            })
    }
}
