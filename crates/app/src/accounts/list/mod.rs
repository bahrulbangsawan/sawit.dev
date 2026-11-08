//! The Accounts tab's list (design: `docs/design/accounts.sawit`): Add Account at the top, then
//! the accounts grouped by agent under its logo. Each row has the avatar, the name with its
//! `cpN` badge, who it is, the 5-hour and 7-day meters with "used" in their names, and a status
//! pill (Active, At limit, Signed out). The list takes the keyboard: ↑ and ↓ select, Enter
//! opens the selected account's terminal; rows drag to reorder, with Move Up and Move Down in
//! each row's menu as the keyboard path.

mod row;

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
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

/// A compact meter for the list: "5h 23% used" over a thin bar, or why there is none. Its words
/// may be cut in a narrow list: hovered, the whole reading shows in a tooltip.
fn mini_meter(
    id: String,
    label: &str,
    w: Option<aa::usage::Window>,
    now: f64,
    cx: &App,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let (text, fill, color, said) = match rows::current(w, now) {
        Some(w) => (
            format!("{label} {} used", rows::pct(&w)),
            (w.used_percentage / 100.0) as f32,
            ui::meter_color(w.used_percentage, cx),
            format!(
                "{label} · {} used · {}",
                rows::pct(&w),
                rows::resets_in(&w, now)
            ),
        ),
        None if w.is_some() => {
            let t = format!("{label} · reset");
            (t.clone(), 0.0, muted, format!("{t} since its last reading"))
        }
        None => {
            let t = format!("{label} · no data");
            (t, 0.0, muted, format!("{label} · not reported yet"))
        }
    };
    let meter = v_flex()
        .flex_1()
        .min_w_0()
        .gap_1()
        .child(div().text_xs().text_color(muted).truncate().child(text))
        .child(ui::track(fill, color, 4., cx));
    ui::chart_tip(meter, id, said, cx)
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
        // Add Account is a compact button on the list's header row; its panel opens above.
        let top = self
            .adding
            .then(|| self.render_add_panel(adoptable, cx).into_any_element());
        let add = Button::new("account-add")
            .xsmall()
            .outline()
            .icon(Lucide::Plus)
            .label("Add")
            .accessibility_label("Add Account")
            .tooltip("Add a Claude Code Account")
            .disabled(self.adding)
            .on_click(cx.listener(|this, _, window, cx| {
                this.adding = true;
                this.new_name.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }));
        v_flex()
            .w(px(if narrow { NARROW_WIDTH } else { WIDTH }))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .children(top.map(|top| div().flex_none().p_3().pb_2().child(top)))
            .child(
                h_flex()
                    .flex_none()
                    .pl(px(14.))
                    .pr(px(10.))
                    .pt_3()
                    .pb_1p5()
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
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .child(div().ml_auto().child(add)),
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
            // Cancel, then the action, on the right; on a narrow list they wrap rather than cut.
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("account-add-cancel")
                            .small()
                            .ghost()
                            .icon(Lucide::X)
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adding = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("account-add-confirm")
                            .small()
                            .primary()
                            .icon(Lucide::LogIn)
                            .label("Add and Sign In")
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .child(div().pt_1().text_xs().text_color(muted).child(if none {
                "No other ~/.claude-* folders to adopt."
            } else {
                "Or adopt a Claude Code folder you already have:"
            }))
            .when(!none, |el| {
                el.child(h_flex().justify_end().child(adopt_button(adoptable)))
            })
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
