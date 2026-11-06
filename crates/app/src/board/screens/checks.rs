//! The design standard's machine checks on the canvas: after an agent writes a screen, or says a
//! work is done, `sawit-mcp` sends what failed (the `checks` op, [`sawit_board::checks`]), and the
//! frame shows a badge over its top-right corner, "3 design issues", which the user opens for the
//! list: each failure's rule, where it is, and the fix. A check that passes takes the badge away.
//! The badge is the user's to open: an agent's check never takes the keyboard or opens it.

use figcraft_doc::NodeId;
use gpui_kit::component::button::*;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_board::checks::Finding;
use serde_json::Value;

use crate::board::BoardView;

/// The badge's height, how far in from the frame's corner it sits, and the list's width.
const HEIGHT: f32 = 22.0;
const INSET: f32 = 8.0;
const LIST_WIDTH: f32 = 340.0;
/// The narrowest frame on screen that shows a badge.
const NARROWEST: f64 = 120.0;

/// What the badge says for `n` failures.
pub(in crate::board) fn badge_text(n: usize) -> String {
    match n {
        1 => "1 design issue".to_string(),
        n => format!("{n} design issues"),
    }
}

impl BoardView {
    /// The `checks` op: the document's answer (the findings given, or the frame's layers'
    /// checked), kept for the frame's badge.
    pub(in crate::board) fn design_checks_op(
        &mut self,
        op: &Value,
        cx: &mut Context<Self>,
    ) -> Value {
        let reply = self.run_op(op, cx);
        let frame = reply["frame"]
            .as_str()
            .and_then(|f| f.parse::<NodeId>().ok());
        let findings: Option<Vec<Finding>> = serde_json::from_value(reply["findings"].clone()).ok();
        if let (Some(frame), Some(findings)) = (frame, findings) {
            if findings.is_empty() {
                self.screen_ui.checks.remove(&frame);
                if self.screen_ui.checks_open == Some(frame) {
                    self.screen_ui.checks_open = None;
                }
            } else {
                self.screen_ui.checks.insert(frame, findings);
            }
            cx.notify();
        }
        reply
    }

    /// The badges of the frames on the page on show that have failures, and the open list.
    pub(in crate::board) fn render_check_badges(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(ds) = self.board().and_then(|b| b.session.ds().ok()) else {
            return Vec::new();
        };
        let view = &ds.view;
        let theme = cx.theme().clone();
        let mut out = Vec::new();
        for (frame, findings) in &self.screen_ui.checks {
            if ds.doc.page_of(*frame) != Some(ds.page) {
                continue;
            }
            let Some(b) = ds.doc.world_bounds(*frame) else {
                continue;
            };
            let (x0, y0) = view.page_to_screen(b.x0, b.y0);
            let (x1, y1) = view.page_to_screen(b.x1, b.y1);
            let shown = x1 > 0.0 && y1 > 0.0 && x0 < view.vw && y0 < view.vh;
            if !shown || x1 - x0 < NARROWEST || y1 - y0 < f64::from(HEIGHT) {
                continue;
            }
            let frame = *frame;
            let open = self.screen_ui.checks_open == Some(frame);
            let right = x1 as f32 - INSET;
            let label = badge_text(findings.len());
            let badge = h_flex()
                .id(("design-checks", frame.0))
                .debug_selector(move || format!("design-checks-{}", frame.0))
                .role(Role::Button)
                .aria_label(SharedString::from(label.clone()))
                .absolute()
                .right(px(view.vw as f32 - right))
                .top(px(y0 as f32 + INSET))
                .h(px(HEIGHT))
                .px_2()
                .gap_1p5()
                .items_center()
                .rounded_full()
                .border_1()
                .border_color(theme.border)
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .shadow_sm()
                .text_xs()
                .cursor_pointer()
                .child(
                    div()
                        .flex_none()
                        .size(px(7.))
                        .rounded_full()
                        .bg(theme.warning),
                )
                .child(div().whitespace_nowrap().child(label))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    let ui = &mut this.screen_ui;
                    ui.checks_open = (ui.checks_open != Some(frame)).then_some(frame);
                    cx.notify();
                }));
            out.push(badge.into_any_element());
            if open {
                let top = y0 as f32 + INSET + HEIGHT + 4.0;
                out.push(list(
                    frame,
                    findings,
                    right,
                    top,
                    view.vw as f32,
                    &theme,
                    cx,
                ));
            }
        }
        out
    }
}

/// The open list of `findings`, its right edge at `right` and its top at `top`.
fn list(
    frame: NodeId,
    findings: &[Finding],
    right: f32,
    top: f32,
    vw: f32,
    theme: &Theme,
    cx: &mut Context<BoardView>,
) -> AnyElement {
    let rows = findings.iter().enumerate().map(|(i, f)| {
        let place = f.place().to_string();
        v_flex()
            .id(("design-check", i))
            .gap_0p5()
            .py_1p5()
            .when(i > 0, |row| row.border_t_1().border_color(theme.border))
            .child(
                h_flex()
                    .id(("design-check-place", i))
                    .gap_1p5()
                    .min_w_0()
                    .child(div().flex_none().font_semibold().child(f.rule.clone()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(place.clone()),
                    )
                    .tooltip(move |window, cx| Tooltip::new(place.clone()).build(window, cx)),
            )
            .child(div().child(f.fix.clone()))
    });
    v_flex()
        .id(("design-checks-list", frame.0))
        .debug_selector(move || format!("design-checks-list-{}", frame.0))
        .role(Role::Group)
        .aria_label("Design standard: what fails")
        .occlude()
        .absolute()
        .right(px((vw - right).max(INSET)))
        .top(px(top))
        .w(px(LIST_WIDTH))
        .max_h(px(280.))
        .overflow_y_scroll()
        .px_3()
        .py_1()
        .rounded(px(10.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .text_color(theme.popover_foreground)
        .shadow_md()
        .text_xs()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_scroll(|_, _, cx| cx.stop_propagation())
        .child(
            h_flex()
                .justify_between()
                .py_1()
                .child(div().font_semibold().child("Design standard: what fails"))
                .child(
                    Button::new("design-checks-close")
                        .ghost()
                        .xsmall()
                        .label("Close")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.screen_ui.checks_open = None;
                            cx.notify();
                        })),
                ),
        )
        .children(rows)
        .into_any_element()
}
