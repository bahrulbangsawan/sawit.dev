//! What the tabs build their rows from: a colour's swatch, its hex with the alpha apart, and
//! rows whose value copies on click.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::site_design::color::Rgba;

use super::{DesignPanel, colors};
use crate::browser::panel;

/// A colour's swatch: the colour over the panel, framed so a colour close to the panel's
/// still shows its edge.
pub(super) fn swatch(c: &Rgba, size: Pixels, cx: &App) -> Div {
    div()
        .size(size)
        .flex_none()
        .rounded(px(4.))
        .border_1()
        .border_color(cx.theme().border)
        .bg(gpui_kit::rgba(colors::rgba_u32(c)))
}

/// `#132643`, and its opacity apart (`60%`) when it has one: a hex with alpha is hard to read.
pub(super) fn hex_and_alpha(c: &Rgba) -> (String, Option<String>) {
    let hex = format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b);
    let alpha = (c.a < 0.995).then(|| format!("{}%", (f64::from(c.a) * 100.).round()));
    (hex, alpha)
}

/// A row whose value copies on click: `label`, then `value`, with a copy mark that shows on
/// hover. `copy` is what goes to the clipboard.
pub(super) fn copy_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    value: impl IntoElement,
    copy: String,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    copy_row_at(id, label, panel::LABEL_WIDTH, value, copy, cx)
}

/// [`copy_row`] with a label column `label_width` wide, for a section whose names run long
/// (`primary-foreground`), so its values still start on one edge.
pub(super) fn copy_row_at(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    label_width: f32,
    value: impl IntoElement,
    copy: String,
    cx: &mut Context<DesignPanel>,
) -> AnyElement {
    let id = id.into();
    let group: SharedString = format!("copy-{id:?}").into();
    let theme = cx.theme();
    h_flex()
        .id(id)
        .group(group.clone())
        .min_h(px(24.))
        .px_1()
        .mx(px(-4.))
        .gap_2()
        .items_center()
        .rounded(px(4.))
        .text_xs()
        .cursor_pointer()
        .hover(|el| el.bg(theme.secondary))
        // The label column, wider for a long name (`primary-foreground`) than for the rest.
        .child(
            div()
                .min_w(px(label_width - 4.))
                .max_w(px(180.))
                .flex_none()
                .truncate()
                .text_color(theme.muted_foreground)
                .child(label.into()),
        )
        .child(div().flex_1().min_w_0().child(value))
        .child(
            div()
                .flex_none()
                .invisible()
                .group_hover(group, |s| s.visible())
                .child(panel::glyph(Lucide::Copy, cx)),
        )
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.copy(copy.clone(), cx);
        }))
        .into_any_element()
}
