//! The Accounts page drawn: the header with the page's summary, the tab list grouped by what
//! the tabs are about (icons only in a narrow window), the notices, then the tab on show.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{AccountsPage, GROUPS, NARROW, Tab, summary};
use crate::states::Banner;

impl AccountsPage {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let m = self.model.read(cx);
        let summary = summary(m, sawit_core::github::now() as f64);
        h_flex()
            .flex_none()
            .h(px(52.))
            .px_4()
            .gap_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Icon::new(Lucide::Users)
                    .small()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(div().text_base().font_semibold().child("Accounts"))
            .child(
                div()
                    .id("accounts-summary")
                    .role(Role::Label)
                    .aria_label(summary.clone())
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(summary),
            )
    }

    /// The tab list: grouped, with icons; icons only when `narrow`. ↑ and ↓ move between tabs
    /// while it has the keyboard, and its selected tab shows a ring then.
    fn render_nav(
        &self,
        narrow: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let focused = self.nav_focus.is_focused(window);
        let current = self.tab;
        let muted = cx.theme().muted_foreground;
        let mut items: Vec<AnyElement> = Vec::new();
        for (gi, (group, tabs)) in GROUPS.iter().enumerate() {
            if narrow {
                if gi > 0 {
                    items.push(
                        div()
                            .w(px(24.))
                            .h(px(1.))
                            .my_1p5()
                            .bg(cx.theme().border)
                            .into_any_element(),
                    );
                }
            } else {
                items.push(
                    div()
                        .px_2()
                        .pt(px(if gi == 0 { 0. } else { 12. }))
                        .pb_1()
                        .text_xs()
                        .font_semibold()
                        .text_color(muted)
                        .child(*group)
                        .into_any_element(),
                );
            }
            for &t in tabs.iter() {
                let selected = t == current;
                let label = t.label();
                items.push(
                    h_flex()
                        .id(SharedString::from(format!("accounts-tab-{label}")))
                        .debug_selector(move || format!("accounts-tab-{label}"))
                        .role(Role::Tab)
                        .aria_label(label)
                        .aria_selected(selected)
                        .flex_none()
                        .h(px(28.))
                        .gap_2()
                        .rounded(cx.theme().radius)
                        .cursor_pointer()
                        .text_sm()
                        .border_2()
                        .border_color(transparent_black())
                        .map(|el| {
                            if narrow {
                                el.w(px(32.)).justify_center()
                            } else {
                                el.w_full().px_1p5()
                            }
                        })
                        .when(selected, |el| el.bg(cx.theme().list_active).font_medium())
                        .when(selected && focused, |el| el.border_color(cx.theme().ring))
                        .when(!selected, |el| el.hover(|el| el.bg(cx.theme().secondary)))
                        .child(Icon::new(t.icon()).small().text_color(if selected {
                            cx.theme().primary
                        } else {
                            muted
                        }))
                        .when(!narrow, |el| el.child(label))
                        .when(narrow, |el| {
                            el.tooltip(move |window, cx| Tooltip::new(label).build(window, cx))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.nav_focus.focus(window, cx);
                            this.show_tab(t, cx);
                        }))
                        .into_any_element(),
                );
            }
        }
        v_flex()
            .id("accounts-nav")
            .role(Role::TabList)
            .aria_label("Account Sections")
            .track_focus(&self.nav_focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if e.keystroke.modifiers.modified() {
                    return;
                }
                let delta = match e.keystroke.key.as_str() {
                    "up" => -1,
                    "down" => 1,
                    _ => return,
                };
                let next = this.tab.step(delta);
                this.show_tab(next, cx);
                cx.stop_propagation();
            }))
            .flex_none()
            .h_full()
            .gap_0p5()
            .py_3()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .map(|el| {
                if narrow {
                    el.w(px(48.)).items_center()
                } else {
                    el.w(px(188.)).px_2()
                }
            })
            .children(items)
    }
}

impl Render for AccountsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let narrow = window.viewport_size().width < NARROW;
        let (error, notice) = {
            let m = self.model.read(cx);
            (m.error.clone(), m.notice.clone())
        };
        let body = match self.tab {
            Tab::Accounts => {
                let offer = self.render_import_offer(cx);
                let list = self.render_list(narrow, window, cx);
                let detail = self.render_detail(narrow, window, cx);
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .children(offer)
                    .child(h_flex().flex_1().min_h_0().child(list).child(detail))
                    .into_any_element()
            }
            Tab::Usage => self.render_usage(cx),
            Tab::StatusLine => self.render_statusline(cx),
            Tab::Diagnostics => self.render_diagnostics(cx),
            Tab::Settings => self.render_settings(cx),
            t => self.render_pool(t, narrow, window, cx),
        };
        let nav = self.render_nav(narrow, window, cx);
        v_flex()
            .id("accounts-page")
            .track_focus(&self.focus)
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_header(cx))
            .child(
                h_flex().flex_1().min_h_0().child(nav).child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .when_some(error, |el, e| {
                            el.child(Banner::error("accounts-error", e).on_dismiss(cx.listener(
                                |this, _, _, cx| {
                                    this.model.update(cx, |m, cx| {
                                        m.error = None;
                                        cx.notify()
                                    })
                                },
                            )))
                        })
                        .when_some(notice, |el, n| {
                            el.child(Banner::info("accounts-notice", n).on_dismiss(cx.listener(
                                |this, _, _, cx| {
                                    this.model.update(cx, |m, cx| {
                                        m.notice = None;
                                        cx.notify()
                                    })
                                },
                            )))
                        })
                        .child(div().flex_1().min_h_0().flex().child(body)),
                ),
            )
    }
}
