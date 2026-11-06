//! The Accounts page's list: every Claude Code account (colour, number and name, who it is,
//! 5h/7d), then Add Account and Adopt Existing.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::{self as aa, AgentKind};

use super::global;
use super::page::AccountsPage;
use super::rows::{self, swatch};

impl AccountsPage {
    pub(super) fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let now = sawit_core::github::now() as f64;
        let active = m.active().map(|a| a.id.clone());
        let sel = self.selected(m).map(|a| a.id.clone());
        let muted = cx.theme().muted_foreground;
        let rows: Vec<AnyElement> = m
            .list()
            .into_iter()
            .enumerate()
            .map(|(ix, a)| {
                let id = a.id.clone();
                let who = m
                    .status(&a.id)
                    .and_then(|s| s.email.clone())
                    .unwrap_or_else(|| a.dir.display().to_string());
                h_flex()
                    .id(SharedString::from(format!("account-row-{}", a.id)))
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(cx.theme().radius)
                    .when(sel.as_ref() == Some(&a.id), |el| el.bg(cx.theme().accent))
                    .hover(|el| el.bg(cx.theme().secondary))
                    .child(swatch(&a.color, 10.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(
                                        div().text_sm().font_semibold().truncate().child(format!(
                                            "{} {}",
                                            ix + 1,
                                            a.name
                                        )),
                                    )
                                    .when(active.as_ref() == Some(&a.id), |el| {
                                        el.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().success)
                                                .child("Active"),
                                        )
                                    }),
                            )
                            .child(div().text_xs().text_color(muted).truncate().child(who))
                            .when_some(rows::short(m.usage.get(&a.id), now), |el, u| {
                                el.child(div().text_xs().text_color(muted).child(u))
                            }),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id.clone());
                        this.renaming = false;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let registered: Vec<PathBuf> = m.list().iter().map(|a| a.dir.clone()).collect();
        let adoptable: Vec<PathBuf> = aa::discover(AgentKind::ClaudeCode, &m.home)
            .into_iter()
            .filter(|d| !registered.contains(d))
            .collect();
        v_flex()
            .w(px(280.))
            .flex_none()
            .h_full()
            .p_2()
            .gap_1()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .px_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child("CLAUDE CODE"),
            )
            .child(
                v_flex()
                    .id("accounts-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(rows),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().flex_1().child(Input::new(&self.new_name).small()))
                    .child(
                        Button::new("account-add")
                            .small()
                            .icon(Lucide::Plus)
                            .tooltip("Add Account and Sign It In")
                            .on_click(cx.listener(|this, _, window, cx| this.add(window, cx))),
                    ),
            )
            .child(
                Button::new("account-adopt")
                    .ghost()
                    .small()
                    .w_full()
                    .icon(Lucide::FolderInput)
                    .label("Adopt Existing…")
                    .disabled(adoptable.is_empty())
                    .tooltip(if adoptable.is_empty() {
                        "No Other ~/.claude-* Folders"
                    } else {
                        "Add a ~/.claude-* Folder You Already Have"
                    })
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
                            menu.item(PopupMenuItem::new(dir.display().to_string()).on_click(
                                move |_, _, cx| {
                                    global(cx).update(cx, |m, cx| {
                                        m.add(&name, Some(d.clone()), cx);
                                    });
                                },
                            ))
                        })
                    }),
            )
    }
}
