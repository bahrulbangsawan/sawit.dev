//! The box model's diagram: margin, border and padding around the content's size, each layer
//! with its four sides.

use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::inspect::StyleInfo;

use super::num;

/// The box model's layers, tinted as browsers' inspectors tint them, so the diagram reads
/// at a glance next to the page's own overlay.
const MARGIN: u32 = 0xf6b26b40;
const BORDER: u32 = 0xffe59950;
const PADDING: u32 = 0x93c47d40;
const CONTENT: u32 = 0x6fa8dc40;

/// A nested box of the diagram: its name in the corner, its four sides' numbers around `inner`.
fn layer(title: &'static str, v: [f64; 4], tint: u32, inner: impl IntoElement, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let n = move |x: f64| {
        div()
            .text_size(px(11.))
            .line_height(px(14.))
            .text_color(if x == 0. {
                muted
            } else {
                cx.theme().foreground
            })
            .child(if x == 0. { "–".to_string() } else { num(x) })
    };
    v_flex()
        .w_full()
        .px_1p5()
        .pb_1()
        .gap_0p5()
        .items_center()
        .rounded(px(4.))
        .bg(gpui_kit::rgba(tint))
        .child(
            h_flex()
                .w_full()
                .h(px(14.))
                .child(
                    div()
                        .flex_1()
                        .text_size(px(11.))
                        .line_height(px(14.))
                        .text_color(muted)
                        .child(title),
                )
                .child(n(v[0]))
                .child(div().flex_1()),
        )
        .child(
            h_flex()
                .w_full()
                .gap_1p5()
                .items_center()
                .child(div().w(px(22.)).flex().justify_center().child(n(v[3])))
                .child(div().flex_1().min_w_0().child(inner))
                .child(div().w(px(22.)).flex().justify_center().child(n(v[1]))),
        )
        .child(n(v[2]))
}

pub(super) fn box_model(info: &StyleInfo, cx: &App) -> impl IntoElement {
    let content = div()
        .w_full()
        .h(px(26.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(3.))
        .bg(gpui_kit::rgba(CONTENT))
        .text_xs()
        .child(format!(
            "{} × {}",
            num(info.rect.width),
            num(info.rect.height)
        ));
    let b = &info.box_model;
    layer(
        "margin",
        b.margin,
        MARGIN,
        layer(
            "border",
            b.border,
            BORDER,
            layer("padding", b.padding, PADDING, content, cx),
            cx,
        ),
        cx,
    )
}
