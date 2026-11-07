//! The Accounts tab's list (design: `docs/design/accounts.sawit`): Add Account at the top, then
//! the accounts grouped by agent under its logo. Each row has the avatar, the name with its
//! `cpN` badge, who it is, the 5-hour and 7-day meters with "used" in their names, and a status
//! pill (Active, At limit, Signed out). The list takes the keyboard: ↑ and ↓ select, Enter
//! opens the selected account's terminal; rows drag to reorder, with Move Up and Move Down in
//! each row's menu as the keyboard path.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::{self as aa, Account, AgentKind};

use super::page::AccountsPage;
use super::rows;
use super::ui::{self, Tone};
use super::{AccountsModel, SignIn, global};

/// The list's width, and in a narrow window.
const WIDTH: f32 = 300.;
const NARROW_WIDTH: f32 = 248.;

/// A row being dragged to a new place.
#[derive(Clone)]
struct DraggedAccount {
    id: String,
    name: SharedString,
}

/// What follows the pointer while a row is dragged.
struct DragLabel(SharedString);

impl Render for DragLabel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded(cx.theme().radius)
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .text_xs()
            .child(self.0.clone())
    }
}

/// An account's status as its row's pill shows it: words and an icon, never a dot's colour.
pub fn status_of(
    m: &AccountsModel,
    a: &Account,
    active: bool,
    now: f64,
) -> Option<(Tone, Lucide, &'static str)> {
    let signed_out = matches!(m.sign_in.get(&a.id), Some(SignIn::Known(s)) if !s.signed_in);
    let at_limit = m.usage.get(&a.id).is_some_and(|s| {
        [s.five_hour, s.seven_day]
            .into_iter()
            .filter_map(|w| rows::current(w, now))
            .any(|w| w.used_percentage >= 100.0)
    });
    if signed_out {
        Some((Tone::Neutral, Lucide::LogIn, "Signed out"))
    } else if at_limit {
        Some((Tone::Bad, Lucide::CircleX, "At limit"))
    } else if active {
        Some((Tone::Good, Lucide::Check, "Active"))
    } else {
        None
    }
}

/// A compact meter for the list: "5h 23% used" over a thin bar, or why there is none.
fn mini_meter(label: &str, w: Option<aa::usage::Window>, now: f64, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let (text, fill, color) = match rows::current(w, now) {
        Some(w) => (
            format!("{label} {} used", rows::pct(&w)),
            (w.used_percentage / 100.0) as f32,
            ui::meter_color(w.used_percentage, cx),
        ),
        None if w.is_some() => (format!("{label} · reset"), 0.0, muted),
        None => (format!("{label} · no data"), 0.0, muted),
    };
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().text_xs().text_color(muted).truncate().child(text))
        .child(ui::track(fill, color, 4., cx))
}

impl AccountsPage {
    pub(super) fn render_list(
        &self,
        narrow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let now = sawit_core::github::now() as f64;
        let active = m.active().map(|a| a.id.clone());
        let sel = self.selected(m).map(|a| a.id.clone());
        let focused = self.list_focus.is_focused(window);
        let muted = cx.theme().muted_foreground;
        let list: Vec<Account> = m.list().into_iter().cloned().collect();
        let count = list.len();
        let home = m.home.to_string_lossy().to_string();
        let registered: Vec<PathBuf> = list.iter().map(|a| a.dir.clone()).collect();
        let adoptable: Vec<PathBuf> = aa::discover(AgentKind::ClaudeCode, &m.home)
            .into_iter()
            .filter(|d| !registered.contains(d))
            .collect();
        let rows: Vec<AnyElement> = list
            .iter()
            .enumerate()
            .map(|(ix, a)| {
                let row = RowFacts {
                    ix,
                    count,
                    selected: sel.as_ref() == Some(&a.id),
                    active: active.as_ref() == Some(&a.id),
                    focused,
                    narrow,
                    now,
                };
                self.render_row(a, row, &home, cx)
            })
            .collect();
        let top = if self.adding {
            self.render_add_panel(adoptable, cx).into_any_element()
        } else {
            Button::new("account-add")
                .small()
                .outline()
                .w_full()
                .icon(Lucide::Plus)
                .label("Add Account")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.adding = true;
                    this.new_name.read(cx).focus_handle(cx).focus(window, cx);
                    cx.notify();
                }))
                .into_any_element()
        };
        v_flex()
            .w(px(if narrow { NARROW_WIDTH } else { WIDTH }))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(div().flex_none().p_3().pb_2().child(top))
            .child(
                h_flex()
                    .flex_none()
                    .px(px(14.))
                    .pt_2()
                    .pb_1()
                    .gap_1p5()
                    .child(crate::terminal::agents::logo(
                        sawit_core::terminal::AgentKind::ClaudeCode,
                        px(14.),
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(AgentKind::ClaudeCode.label()),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_xs()
                            .text_color(muted)
                            .child(count.to_string()),
                    ),
            )
            .child(
                v_flex()
                    .id("accounts-list")
                    .role(Role::List)
                    .aria_label("Claude Code Accounts")
                    .track_focus(&self.list_focus)
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                        if e.keystroke.modifiers.modified() {
                            return;
                        }
                        match e.keystroke.key.as_str() {
                            "up" => this.select_step(-1, cx),
                            "down" => this.select_step(1, cx),
                            "enter" => {
                                let a = this.selected(this.model.read(cx)).cloned();
                                if let Some(a) = a {
                                    this.open_terminal(&a, cx);
                                }
                            }
                            _ => return,
                        }
                        cx.stop_propagation();
                    }))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .pb_3()
                    .gap_0p5()
                    .children(rows),
            )
    }

    fn render_row(
        &self,
        a: &Account,
        row: RowFacts,
        home: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let m = self.model.read(cx);
        let id = a.id.clone();
        let who = m
            .status(&a.id)
            .and_then(|s| s.email.clone())
            .unwrap_or_else(|| a.dir.display().to_string().replacen(home, "~", 1));
        let status = status_of(m, a, row.active, row.now);
        let usage = m.usage.get(&a.id).cloned();
        let cp = format!("cp{}", row.ix + 1);
        let spoken = match status {
            Some((_, _, s)) => format!("{}, {cp}, {s}", a.name),
            None => format!("{}, {cp}", a.name),
        };
        let page = cx.entity();
        let menu_account = a.clone();
        let tip = who.clone();
        let RowFacts {
            ix,
            count,
            selected,
            active,
            focused,
            narrow,
            now,
        } = row;
        h_flex()
            .id(SharedString::from(format!("account-row-{}", a.id)))
            .debug_selector({
                let id = a.id.clone();
                move || format!("account-row-{id}")
            })
            .role(Role::ListItem)
            .aria_label(spoken)
            .aria_selected(selected)
            .items_start()
            .gap(px(10.))
            .px(px(10.))
            .py_2()
            .rounded(cx.theme().radius)
            .border_2()
            .border_color(transparent_black())
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(selected && focused, |el| el.border_color(cx.theme().ring))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().secondary)))
            .child(ui::avatar(&a.name, &a.color, 28., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .min_h(px(20.))
                            .child(
                                div()
                                    .text_sm()
                                    .font_semibold()
                                    .min_w_0()
                                    .truncate()
                                    .child(a.name.clone()),
                            )
                            .child(ui::badge(cp.clone(), cx))
                            .child(div().flex_1())
                            .when_some(status, |el, (tone, icon, label)| {
                                el.child(ui::pill(
                                    SharedString::from(format!("account-status-{}", a.id)),
                                    tone,
                                    Some(icon),
                                    label,
                                    cx,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("account-who-{}", a.id)))
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(who)
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx)),
                    )
                    .when(!narrow, |el| {
                        el.child(
                            h_flex()
                                .mt_1p5()
                                .gap_3()
                                .child(mini_meter(
                                    "5h",
                                    usage.as_ref().and_then(|u| u.five_hour),
                                    now,
                                    cx,
                                ))
                                .child(mini_meter(
                                    "7d",
                                    usage.as_ref().and_then(|u| u.seven_day),
                                    now,
                                    cx,
                                )),
                        )
                    }),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.list_focus.focus(window, cx);
                this.selected = Some(id.clone());
                this.renaming = false;
                cx.notify();
            }))
            .on_drag(
                DraggedAccount {
                    id: a.id.clone(),
                    name: a.name.clone().into(),
                },
                |d: &DraggedAccount, _, _, cx| cx.new(|_| DragLabel(d.name.clone())),
            )
            .drag_over::<DraggedAccount>(|el, _, _, cx| el.bg(cx.theme().accent))
            .on_drop(cx.listener(move |this, d: &DraggedAccount, _, cx| {
                this.model
                    .update(cx, |m, cx| m.move_account_to(&d.id, ix, cx));
            }))
            .context_menu(move |menu, _, _| {
                account_menu(menu, &page, &menu_account, ix, count, active)
            })
            .into_any_element()
    }

    /// The Add Account panel: a labelled name field, Add and Sign In, Cancel, and the folders
    /// that can be adopted instead (with the reason when there are none).
    fn render_add_panel(
        &self,
        adoptable: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let none = adoptable.is_empty();
        v_flex()
            .id("account-add-panel")
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(div().text_xs().font_medium().child("Account name"))
            .child(
                Input::new(&self.new_name)
                    .small()
                    .aria_label("Account Name"),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("account-add-confirm")
                            .small()
                            .primary()
                            .icon(Lucide::LogIn)
                            .label("Add and Sign In")
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    )
                    .child(
                        Button::new("account-add-cancel")
                            .small()
                            .ghost()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adding = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().pt_1().text_xs().text_color(muted).child(if none {
                "No other ~/.claude-* folders to adopt."
            } else {
                "Or adopt a Claude Code folder you already have:"
            }))
            .when(!none, |el| el.child(adopt_button(adoptable)))
    }
}

/// What a row needs to know of its place and the page.
#[derive(Clone, Copy)]
struct RowFacts {
    ix: usize,
    count: usize,
    selected: bool,
    active: bool,
    focused: bool,
    narrow: bool,
    now: f64,
}

/// Adopt Existing…: a menu of the `~/.claude-*` folders not yet in the list.
pub fn adopt_button(adoptable: Vec<PathBuf>) -> impl IntoElement {
    Button::new("account-adopt")
        .small()
        .outline()
        .icon(Lucide::FolderInput)
        .label("Adopt Existing…")
        .dropdown_menu(move |menu, _, _| {
            adoptable.iter().fold(menu, |menu, dir| {
                let d = dir.clone();
                let name = dir
                    .file_name()
                    .map(|f| {
                        f.to_string_lossy()
                            .trim_start_matches(".claude-")
                            .to_string()
                    })
                    .unwrap_or_default();
                menu.item(
                    PopupMenuItem::new(dir.display().to_string())
                        .icon(crate::menu_icons::IMPORT)
                        .on_click(move |_, _, cx| {
                            global(cx).update(cx, |m, cx| {
                                m.add(&name, Some(d.clone()), cx);
                            });
                        }),
                )
            })
        })
}

type PageAction = fn(&mut AccountsPage, &Account, &mut Window, &mut Context<AccountsPage>);

/// The menu of an account (its row's context menu and the detail's ⋯): Set Active, Open in
/// Terminal, Rename, Check Sign-In Again, Move Up and Move Down, and Remove, apart and asked
/// first.
pub fn account_menu(
    menu: gpui_kit::component::menu::PopupMenu,
    page: &Entity<AccountsPage>,
    a: &Account,
    ix: usize,
    count: usize,
    is_active: bool,
) -> gpui_kit::component::menu::PopupMenu {
    let on = |f: PageAction| {
        let (page, a) = (page.clone(), a.clone());
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            page.update(cx, |this, cx| f(this, &a, window, cx))
        }
    };
    menu.item(
        PopupMenuItem::new("Set Active")
            .icon(crate::menu_icons::SWITCH)
            .disabled(is_active)
            .on_click(on(|this, a, _, cx| {
                let id = a.id.clone();
                this.model.update(cx, |m, cx| m.set_active(&id, cx))
            })),
    )
    .item(
        PopupMenuItem::new("Open in Terminal")
            .icon(crate::menu_icons::OPEN_TERMINAL)
            .on_click(on(|this, a, _, cx| this.open_terminal(a, cx))),
    )
    .separator()
    .item(
        PopupMenuItem::new("Rename…")
            .icon(crate::menu_icons::RENAME)
            .on_click(on(|this, a, window, cx| this.start_rename(a, window, cx))),
    )
    .item(
        PopupMenuItem::new("Check Sign-In Again")
            .icon(crate::menu_icons::REFRESH)
            .on_click(on(|this, a, _, cx| {
                let id = a.id.clone();
                this.model.update(cx, |m, cx| m.check(&id, cx))
            })),
    )
    .item(
        PopupMenuItem::new("Move Up")
            .icon(Lucide::ArrowUp)
            .disabled(ix == 0)
            .on_click(on(|this, a, _, cx| {
                let id = a.id.clone();
                this.model.update(cx, |m, cx| m.move_account(&id, -1, cx))
            })),
    )
    .item(
        PopupMenuItem::new("Move Down")
            .icon(Lucide::ArrowDown)
            .disabled(ix + 1 >= count)
            .on_click(on(|this, a, _, cx| {
                let id = a.id.clone();
                this.model.update(cx, |m, cx| m.move_account(&id, 1, cx))
            })),
    )
    .when(a.tracking, |menu| {
        menu.item(
            PopupMenuItem::new("Turn Off Usage Tracking")
                .icon(crate::menu_icons::HIDE)
                .on_click(on(|this, a, _, cx| {
                    let id = a.id.clone();
                    this.model
                        .update(cx, |m, cx| m.set_tracking(&id, false, cx))
                })),
        )
    })
    .separator()
    .item(
        PopupMenuItem::new("Remove from Sawit…")
            .icon(crate::menu_icons::DELETE)
            .danger(true)
            .on_click(on(|this, a, window, cx| {
                this.confirm_remove(a.clone(), window, cx)
            })),
    )
}
