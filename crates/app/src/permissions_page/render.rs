//! The page around its sections: the header, the last change's message, and the sheet that
//! shows a change's diff before it's saved.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Pending, PermissionsPage};
use crate::catalog::tilde;

impl PermissionsPage {
    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        h_flex()
            .gap_4()
            .items_start()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(div().text_xl().font_semibold().child("Permissions"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .whitespace_normal()
                            .child(
                                "What agents may do on this Mac. Claude Code applies Deny, then Ask, then Allow. \
                                 Each change shows its diff first; Save keeps a backup beside the file.",
                            ),
                    ),
            )
            .child(
                Button::new("perms-reload")
                    .ghost()
                    .small()
                    .icon(Lucide::RefreshCw)
                    .tooltip("Read the Files Again")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.reload();
                        cx.notify();
                    })),
            )
    }

    fn render_message(&self, cx: &App) -> Option<AnyElement> {
        let (ok, text) = self.message.as_ref()?;
        let color = if *ok {
            cx.theme().success
        } else {
            cx.theme().danger
        };
        Some(
            h_flex()
                .id("perms-message")
                .gap_1p5()
                .items_start()
                .text_sm()
                .text_color(color)
                .child(
                    div().pt(px(2.)).child(
                        Icon::new(if *ok {
                            Lucide::CircleCheck
                        } else {
                            Lucide::CircleAlert
                        })
                        .small(),
                    ),
                )
                .child(div().min_w_0().whitespace_normal().child(text.clone()))
                .into_any_element(),
        )
    }

    fn render_sheet(&self, pending: &Pending, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let mono = theme.mono_font_family.clone();
        let lines = pending.plan.diff.lines().skip(2).map(|line| {
            let color = match line.chars().next() {
                Some('+') => theme.success,
                Some('-') => theme.danger,
                Some('@') => theme.muted_foreground,
                _ => theme.foreground,
            };
            div()
                .whitespace_nowrap()
                .text_color(color)
                .child(line.to_string())
        });
        v_flex()
            .id("perms-sheet")
            .role(Role::Dialog)
            .aria_label("Save Change")
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
                        div()
                            .text_base()
                            .font_semibold()
                            .whitespace_normal()
                            .child(pending.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .font_family(mono.clone())
                            .truncate()
                            .child(tilde(&pending.plan.path)),
                    ),
            )
            .child(
                div()
                    .id("perms-diff")
                    .p_4()
                    .flex_1()
                    .min_h_0()
                    .overflow_scroll()
                    .text_xs()
                    .font_family(mono)
                    .bg(theme.secondary)
                    .children(lines),
            )
            .child(
                h_flex()
                    .p_3()
                    .gap_2()
                    .justify_end()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .whitespace_normal()
                            .child(if pending.plan.before.is_empty() {
                                "The file is new: Save creates it."
                            } else {
                                "Only these lines change. The file as it is now is kept beside it (.sawit-backup)."
                            }),
                    )
                    .child(
                        Button::new("perms-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, window, cx| this.cancel(window, cx))),
                    )
                    .child(
                        Button::new("perms-save")
                            .primary()
                            .small()
                            // Says what it does; Return doesn't press it (only Escape, which
                            // cancels, is a key here), so Cancel stays the safe default.
                            .label("Save Change")
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
            .into_any_element()
    }
}

impl Render for PermissionsPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let files = self.render_files(window, cx);
        let overlay = self.pending.as_ref().map(|p| self.render_sheet(p, cx));
        if overlay.is_some() && !self.sheet_focus.contains_focused(window, cx) {
            self.sheet_focus.focus(window, cx);
        }
        div()
            .relative()
            .size_full()
            .key_context("PermissionsPage")
            .track_focus(&self.focus)
            .bg(cx.theme().background)
            .child(
                div()
                    .id("permissions-page")
                    .size_full()
                    .overflow_y_scroll()
                    .child(
                        v_flex()
                            .p_6()
                            .gap_6()
                            .child(self.render_header(cx))
                            .children(self.render_message(cx))
                            .child(files)
                            .child(self.render_tools(cx))
                            .child(self.render_macos(cx))
                            .children(self.render_others(cx)),
                    ),
            )
            .when_some(overlay, |el, sheet| {
                el.child(
                    div()
                        .id("perms-sheet-backdrop")
                        .track_focus(&self.sheet_focus)
                        .absolute()
                        .inset_0()
                        .p_6()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(gpui_kit::black().opacity(0.45))
                        .occlude()
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                            if e.keystroke.key == "escape" {
                                this.cancel(window, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(sheet),
                )
            })
    }
}
