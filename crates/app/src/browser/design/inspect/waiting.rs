//! The tab with nothing inspected: how to start, the keys, and the element pinned last.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::inspect::StyleInfo;

use super::super::{DesignEvent, DesignPanel, Segment};
use crate::browser::panel;

/// The tab with nothing inspected: how to start, the keys, and the element pinned last.
pub(super) fn waiting(
    last: Option<&StyleInfo>,
    inspecting: bool,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let line = |keys: Vec<Div>, what: &'static str| {
        h_flex()
            .gap_1()
            .items_center()
            .text_xs()
            .children(keys)
            .child(div().pl_1().text_color(muted).child(what))
    };
    let action = (!inspecting).then(|| {
        Button::new("design-inspect-again")
            .outline()
            .xsmall()
            .icon(Lucide::MousePointer2)
            .label("Inspect the page")
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.set_segment(Segment::Inspect, cx);
            }))
            .into_any_element()
    });
    v_flex()
        .gap_4()
        .child(panel::empty(
            if inspecting {
                "Point at an element"
            } else {
                "Inspecting is off"
            },
            if inspecting {
                "Hover the page to read an element's box, type, colors and layout here. \
                 Click it to keep it while you look."
            } else {
                "Esc stopped it. Inspect again to read elements as you hover them."
            },
            action,
            cx,
        ))
        .children(last.map(|info| {
            let selector = info.selector.clone();
            let again = info.selector.clone();
            panel::section(
                "Pinned last",
                None,
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .child(info.label.clone()),
                    )
                    .child(
                        Button::new("design-last-show")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Eye)
                            .label("Show")
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(DesignEvent::Flash(selector.clone()));
                            })),
                    )
                    .child(
                        Button::new("design-last-pin")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Pin)
                            .label("Pin")
                            .disabled(!inspecting)
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(DesignEvent::Pin(again.clone()));
                            })),
                    ),
                cx,
            )
        }))
        .child(panel::section(
            "Keys",
            None,
            v_flex()
                .gap_1p5()
                .child(line(vec![panel::key("Click", cx)], "Pin the element"))
                .child(line(
                    vec![panel::key("⌥", cx)],
                    "Hold over another to measure the gap",
                ))
                .child(line(
                    vec![panel::key("↑", cx), panel::key("↓", cx)],
                    "Walk to the parent or back to the child",
                ))
                .child(line(vec![panel::key("Esc", cx)], "Stop inspecting")),
            cx,
        ))
        .into_any_element()
}
