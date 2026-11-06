//! The Accounts page's Status Line tab: the line Claude Code shows under the prompt, designed
//! here. A preset or the layout in use to start from, a theme, components added and removed per
//! line, a live preview with an account's real name, plan and usage, then Apply (the layout before
//! is kept for Undo Last Apply) and, per account, Use Sawit's Status Line (it needs usage
//! tracking, which runs the collector that draws it).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::statusline::{self as sl, Kind, Values, Widget};
use sawit_core::agent_accounts::tracking;

use super::AccountsModel;
use super::page::AccountsPage;

impl AccountsModel {
    /// Uses (or stops using) Sawit's layout for `id`, with the name, email and plan it shows.
    pub fn set_managed(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let Some(a) = self.accounts.get(id).cloned() else {
            return;
        };
        let line = sl::AccountLine {
            managed: on,
            profile_name: Some(a.name.clone()),
            account: self.status(id).and_then(|s| s.email.clone()),
            plan: self.plan.get(id).cloned(),
        };
        if let Err(e) = sl::save_account(&self.root, id, &line) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}

/// The values the preview draws with: a made-up session in this folder, and `id`'s account.
fn preview_values(m: &AccountsModel, id: Option<&str>) -> Values {
    let now = sawit_core::github::now() as f64;
    let usage = id.and_then(|id| m.usage.get(id));
    Values {
        model: Some("Opus".into()),
        cwd: Some("~/projects/sawit".into()),
        git_branch: Some("main".into()),
        git_status: Some(sl::GitStatus {
            staged: 1,
            modified: 2,
            ..Default::default()
        }),
        context_percent: Some(23.0),
        context_tokens: Some(45_200),
        context_size: Some(200_000),
        last_input: Some(1_200),
        last_output: Some(830),
        cache_read: Some(41_000),
        cache_write: Some(2_100),
        cost: Some(1.42),
        duration_ms: Some(2_460_000.0),
        lines_added: Some(120),
        lines_removed: Some(18),
        effort: Some("high".into()),
        version: Some("2.1.300".into()),
        original: Some("your own status line".into()),
        five_hour: usage.and_then(|u| u.five_hour),
        seven_day: usage.and_then(|u| u.seven_day),
        account: id.and_then(|id| m.status(id)).and_then(|s| s.email.clone()),
        plan: id.and_then(|id| m.plan.get(id).cloned()),
        profile: id.and_then(|id| m.accounts.get(id)).map(|a| a.name.clone()),
        now,
        ..Default::default()
    }
}

impl AccountsPage {
    pub(super) fn render_statusline(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let root = m.root.clone();
        let layout = self
            .layout
            .get_or_insert_with(|| sl::load_config(&root).unwrap_or_default())
            .clone();
        let first = m.list().first().map(|a| a.id.clone());
        let values = preview_values(m, first.as_deref());
        let muted = cx.theme().muted_foreground;
        let card = |title: &'static str| {
            v_flex()
                .gap_2()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().font_semibold().child(title))
        };
        // The preview, drawn by the collector's own code.
        let lines = sl::segments(&layout, &values, Some(100));
        let preview = v_flex()
            .p_2()
            .rounded(cx.theme().radius)
            .bg(gpui_kit::rgb(0x1e1e1e))
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .children(lines.into_iter().map(|line| {
                h_flex().children(line.into_iter().map(|s| {
                    let mut el = div().child(s.text.replace(' ', "\u{a0}"));
                    if let Some((r, g, b)) = s.fg.as_deref().and_then(sl::rgb) {
                        el = el.text_color(gpui_kit::rgb(
                            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
                        ));
                    }
                    if let Some((r, g, b)) = s.bg.as_deref().and_then(sl::rgb) {
                        el = el.bg(gpui_kit::rgb(
                            (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
                        ));
                    }
                    if s.bold {
                        el = el.font_weight(FontWeight::BOLD);
                    }
                    el
                }))
            }));
        let problems = layout.problems();
        let theme_name = sl::theme(&layout.theme)
            .map(|t| t.name)
            .unwrap_or("Classic");
        let editor = v_flex()
            .gap_2()
            .children(layout.lines.iter().enumerate().map(|(li, line)| {
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .w(px(44.))
                            .child(format!("Line {}", li + 1)),
                    )
                    .children(line.iter().enumerate().map(|(wi, w)| {
                        Button::new(SharedString::from(format!("sl-w-{li}-{wi}")))
                            .xsmall()
                            .outline()
                            .label(w.kind.title())
                            .icon(Lucide::X)
                            .tooltip("Remove")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(l) = this.layout.as_mut()
                                    && let Some(line) = l.lines.get_mut(li)
                                {
                                    line.remove(wi);
                                }
                                cx.notify();
                            }))
                    }))
                    .child(
                        Button::new(SharedString::from(format!("sl-add-{li}")))
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Plus)
                            .tooltip("Add a Component")
                            .dropdown_menu({
                                let page = cx.entity();
                                move |menu, _, _| {
                                    Kind::ALL.iter().fold(menu, |menu, &k| {
                                        let page = page.clone();
                                        menu.item(PopupMenuItem::new(k.title()).on_click(
                                            move |_, _, cx| {
                                                page.update(cx, |this, cx| {
                                                    if let Some(l) = this.layout.as_mut()
                                                        && let Some(line) = l.lines.get_mut(li)
                                                    {
                                                        line.push(Widget::new(k));
                                                    }
                                                    cx.notify();
                                                })
                                            },
                                        ))
                                    })
                                }
                            }),
                    )
            }))
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("sl-add-line")
                            .xsmall()
                            .ghost()
                            .label("Add Line")
                            .disabled(layout.lines.len() >= sl::MAX_LINES)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(l) = this.layout.as_mut() {
                                    l.lines.push(Vec::new());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("sl-theme")
                            .xsmall()
                            .outline()
                            .label(format!("Theme: {theme_name}"))
                            .dropdown_menu({
                                let page = cx.entity();
                                move |menu, _, _| {
                                    sl::THEMES.iter().fold(menu, |menu, t| {
                                        let (page, id) = (page.clone(), t.id.to_string());
                                        menu.item(PopupMenuItem::new(t.name).on_click(
                                            move |_, _, cx| {
                                                page.update(cx, |this, cx| {
                                                    if let Some(l) = this.layout.as_mut() {
                                                        l.theme = id.clone();
                                                    }
                                                    cx.notify();
                                                })
                                            },
                                        ))
                                    })
                                }
                            }),
                    )
                    .child(
                        Button::new("sl-preset")
                            .xsmall()
                            .outline()
                            .label("Start From…")
                            .dropdown_menu({
                                let page = cx.entity();
                                move |menu, _, _| {
                                    sl::presets()
                                        .into_iter()
                                        .fold(menu, |menu, (name, config)| {
                                            let page = page.clone();
                                            menu.item(PopupMenuItem::new(name).on_click(
                                                move |_, _, cx| {
                                                    page.update(cx, |this, cx| {
                                                        this.layout = Some(config.clone());
                                                        cx.notify();
                                                    })
                                                },
                                            ))
                                        })
                                }
                            }),
                    )
                    .child(
                        Button::new("sl-powerline")
                            .xsmall()
                            .ghost()
                            .label(if layout.powerline.enabled {
                                "Powerline: On"
                            } else {
                                "Powerline: Off"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(l) = this.layout.as_mut() {
                                    l.powerline.enabled = !l.powerline.enabled;
                                }
                                cx.notify();
                            })),
                    ),
            );
        let accounts: Vec<(String, String, bool, bool)> = m
            .list()
            .iter()
            .map(|a| {
                let managed = sl::load_account(&root, &a.id).is_some_and(|l| l.managed);
                (
                    a.id.clone(),
                    a.name.clone(),
                    managed,
                    tracking::state(a) == tracking::Tracking::On,
                )
            })
            .collect();
        v_flex()
            .id("accounts-statusline")
            .flex_1()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(card("Preview").child(preview).child(div().text_xs().text_color(muted).child("With the first account's name, plan and usage; the session's values are made up.")))
            .child(card("Layout").child(editor).when(!problems.is_empty(), |el| el.child(div().text_xs().text_color(cx.theme().danger).child(problems.join(" ")))))
            .child(
                h_flex()
                    .gap_1()
                    .child(Button::new("sl-apply").small().primary().label("Apply").disabled(!problems.is_empty()).on_click(cx.listener(|this, _, _, cx| {
                        let Some(l) = this.layout.clone() else { return };
                        this.model.update(cx, |m, cx| {
                            match sl::apply(&m.root, &l, sawit_core::github::now()) {
                                Ok(()) => m.notice = Some("Applied. Claude Code shows it at the next status line update.".into()),
                                Err(e) => m.error = Some(e.to_string()),
                            }
                            cx.notify();
                        });
                    })))
                    .child(Button::new("sl-undo").small().ghost().label("Undo Last Apply").on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            match sl::undo(&m.root) {
                                Ok(true) => m.notice = Some("The layout before the last Apply is back.".into()),
                                Ok(false) => m.notice = Some("There is no earlier layout.".into()),
                                Err(e) => m.error = Some(e.to_string()),
                            }
                            cx.notify();
                        });
                        this.layout = None;
                        cx.notify();
                    }))),
            )
            .child(card("Accounts").children(accounts.into_iter().map(|(id, name, managed, tracked)| {
                Switch::new(SharedString::from(format!("sl-use-{id}")))
                    .checked(managed)
                    .disabled(!tracked)
                    .label(if tracked { format!("{name}: Use Sawit's Status Line") } else { format!("{name}: turn on usage tracking first") })
                    .on_click(cx.listener(move |this, on: &bool, _, cx| this.model.update(cx, |m, cx| m.set_managed(&id, *on, cx))))
            })))
            .into_any_element()
    }
}
