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
use super::ui;

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
        // The preview, drawn by the collector's own code.
        let lines = sl::segments(&layout, &values, Some(100));
        let preview = v_flex()
            .id("sl-preview")
            .role(Role::Image)
            .aria_label("Status line preview")
            .px_3()
            .py_2()
            .rounded(cx.theme().radius)
            .bg(gpui_kit::rgb(0x1e1e1e))
            .font_family(cx.theme().mono_font_family.clone())
            .text_sm()
            .overflow_x_hidden()
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
        let page = cx.entity();
        let editor = v_flex()
            .gap_2()
            .children(layout.lines.iter().enumerate().map(|(li, line)| {
                let len = line.len();
                h_flex()
                    .gap_1()
                    .flex_wrap()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .w(px(48.))
                            .child(format!("Line {}", li + 1)),
                    )
                    // Each chip's menu moves it left or right, or removes it: the keyboard path.
                    .children(line.iter().enumerate().map(|(wi, w)| {
                        let page = page.clone();
                        Button::new(SharedString::from(format!("sl-w-{li}-{wi}")))
                            .xsmall()
                            .outline()
                            .label(w.kind.title())
                            .dropdown_caret(true)
                            .accessibility_label(format!(
                                "{}, line {}, place {}",
                                w.kind.title(),
                                li + 1,
                                wi + 1
                            ))
                            .dropdown_menu(move |menu, _, _| {
                                let edit = |f: fn(&mut Vec<Widget>, usize)| {
                                    let page = page.clone();
                                    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                                        page.update(cx, |this, cx| {
                                            if let Some(l) = this.layout.as_mut()
                                                && let Some(line) = l.lines.get_mut(li)
                                                && wi < line.len()
                                            {
                                                f(line, wi);
                                            }
                                            cx.notify();
                                        })
                                    }
                                };
                                menu.item(
                                    PopupMenuItem::new("Move Left")
                                        .icon(Lucide::ArrowLeft)
                                        .disabled(wi == 0)
                                        .on_click(edit(|line, wi| line.swap(wi, wi - 1))),
                                )
                                .item(
                                    PopupMenuItem::new("Move Right")
                                        .icon(Lucide::ArrowRight)
                                        .disabled(wi + 1 >= len)
                                        .on_click(edit(|line, wi| line.swap(wi, wi + 1))),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Remove")
                                        .icon(crate::menu_icons::DELETE)
                                        .danger(true)
                                        .on_click(edit(|line, wi| {
                                            line.remove(wi);
                                        })),
                                )
                            })
                    }))
                    .child(
                        Button::new(SharedString::from(format!("sl-add-{li}")))
                            .xsmall()
                            .ghost()
                            .icon(Lucide::Plus)
                            .accessibility_label(format!("Add a Component to Line {}", li + 1))
                            .tooltip("Add a component")
                            .dropdown_menu({
                                let page = cx.entity();
                                move |menu, _, _| {
                                    Kind::ALL.iter().fold(menu, |menu, &k| {
                                        let page = page.clone();
                                        menu.item(
                                            PopupMenuItem::new(k.title())
                                                .icon(crate::menu_icons::NEW)
                                                .on_click(move |_, _, cx| {
                                                    page.update(cx, |this, cx| {
                                                        if let Some(l) = this.layout.as_mut()
                                                            && let Some(line) = l.lines.get_mut(li)
                                                        {
                                                            line.push(Widget::new(k));
                                                        }
                                                        cx.notify();
                                                    })
                                                }),
                                        )
                                    })
                                }
                            }),
                    )
            }));
        let tools = h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                Button::new("sl-add-line")
                    .xsmall()
                    .outline()
                    .icon(Lucide::Plus)
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
                    .icon(Lucide::Palette)
                    .label(format!("Theme: {theme_name}"))
                    .dropdown_menu({
                        let page = cx.entity();
                        move |menu, _, _| {
                            sl::THEMES.iter().fold(menu, |menu, t| {
                                let (page, id) = (page.clone(), t.id.to_string());
                                menu.item(
                                    PopupMenuItem::new(t.name).icon(Lucide::Palette).on_click(
                                        move |_, _, cx| {
                                            page.update(cx, |this, cx| {
                                                if let Some(l) = this.layout.as_mut() {
                                                    l.theme = id.clone();
                                                }
                                                cx.notify();
                                            })
                                        },
                                    ),
                                )
                            })
                        }
                    }),
            )
            .child(
                Button::new("sl-preset")
                    .xsmall()
                    .outline()
                    .icon(Lucide::LayoutTemplate)
                    .label("Start From…")
                    .dropdown_menu({
                        let page = cx.entity();
                        move |menu, _, _| {
                            sl::presets()
                                .into_iter()
                                .fold(menu, |menu, (name, config)| {
                                    let page = page.clone();
                                    menu.item(
                                        PopupMenuItem::new(name)
                                            .icon(crate::menu_icons::WORK)
                                            .on_click(move |_, _, cx| {
                                                page.update(cx, |this, cx| {
                                                    this.layout = Some(config.clone());
                                                    cx.notify();
                                                })
                                            }),
                                    )
                                })
                        }
                    }),
            )
            .child(
                Switch::new("sl-powerline")
                    .checked(layout.powerline.enabled)
                    .label("Powerline")
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        if let Some(l) = this.layout.as_mut() {
                            l.powerline.enabled = *on;
                        }
                        cx.notify();
                    })),
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
        let account_rows: Vec<AnyElement> = accounts
            .into_iter()
            .map(|(id, name, managed, tracked)| {
                let control = Switch::new(SharedString::from(format!("sl-use-{id}")))
                    .checked(managed)
                    .disabled(!tracked)
                    .accessibility_label(format!("{name}: Use Sawit's Status Line"))
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, on: &bool, _, cx| {
                            this.model.update(cx, |m, cx| m.set_managed(&id, *on, cx))
                        }
                    }));
                ui::setting_row(
                    Lucide::SquareTerminal,
                    name,
                    if tracked {
                        "Use Sawit's status line in this account's Claude Code"
                    } else {
                        "Turn on usage tracking on the Accounts tab first: it runs the status line"
                    },
                    control,
                    !tracked,
                    cx,
                )
                .into_any_element()
            })
            .collect();
        let apply = h_flex()
            .gap_2()
            .child(
                Button::new("sl-apply")
                    .small()
                    .primary()
                    .icon(Lucide::Check)
                    .label("Apply")
                    .disabled(!problems.is_empty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        let Some(l) = this.layout.clone() else { return };
                        this.model.update(cx, |m, cx| {
                            match sl::apply(&m.root, &l, sawit_core::github::now()) {
                                Ok(()) => m.notice = Some(
                                    "Applied. Claude Code shows it at the next status line update."
                                        .into(),
                                ),
                                Err(e) => m.error = Some(e.to_string()),
                            }
                            cx.notify();
                        });
                    })),
            )
            .child(
                Button::new("sl-undo")
                    .small()
                    .ghost()
                    .icon(Lucide::Undo2)
                    .label("Undo Last Apply")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.model.update(cx, |m, cx| {
                            match sl::undo(&m.root) {
                                Ok(true) => {
                                    m.notice =
                                        Some("The layout before the last Apply is back.".into())
                                }
                                Ok(false) => m.notice = Some("There is no earlier layout.".into()),
                                Err(e) => m.error = Some(e.to_string()),
                            }
                            cx.notify();
                        });
                        this.layout = None;
                        cx.notify();
                    })),
            );
        // The preview stays in view at the top while the layout below scrolls.
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(
                v_flex()
                    .flex_none()
                    .w_full()
                    .px_6()
                    .pt_6()
                    .pb_4()
                    .child(ui::section_header(Lucide::Eye, "Preview", cx).child(
                        div().text_xs().text_color(muted).child(
                            "The first account's name, plan and usage, with a made-up session",
                        ),
                    ))
                    .child(preview),
            )
            .child(
                div()
                    .id("accounts-statusline")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .w_full()
                            .px_6()
                            .pb_6()
                            .gap_6()
                            .child(
                                v_flex()
                                    .child(ui::section_header(Lucide::LayoutTemplate, "Layout", cx))
                                    .child(
                                        ui::group_box(cx)
                                            .p_4()
                                            .gap_3()
                                            .child(editor)
                                            .child(tools)
                                            .when(!problems.is_empty(), |el| {
                                                el.child(
                                                    h_flex()
                                                        .items_start()
                                                        .gap_1()
                                                        .text_xs()
                                                        .text_color(cx.theme().danger)
                                                        .child(div().pt(px(1.)).flex_none().child(
                                                            Icon::new(Lucide::CircleX).xsmall(),
                                                        ))
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .whitespace_normal()
                                                                .child(problems.join(" ")),
                                                        ),
                                                )
                                            }),
                                    )
                                    .child(div().pt_3().child(apply)),
                            )
                            .child(
                                v_flex()
                                    .child(ui::section_header(Lucide::Users, "Accounts", cx))
                                    .child(ui::group_box(cx).children(ui::rows(account_rows, cx))),
                            ),
                    ),
            )
            .into_any_element()
    }
}
