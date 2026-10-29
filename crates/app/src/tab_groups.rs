//! Tab groups, one per open project, shared by the center tabs and the
//! terminal dock: each group has its project's colour and a header that
//! folds the group's tabs away.

use std::path::Path;

use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The colour of the project at `ix` in the window's project list.
pub fn group_color(ix: usize, cx: &App) -> Hsla {
    let t = cx.theme();
    [t.blue, t.green, t.yellow, t.magenta, t.cyan, t.red][ix % 6]
}

/// A project's name: its folder's.
pub fn project_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

/// The label in front of a group's tabs: the project's colour as a dot and
/// its name, quiet so the tabs stay the focus. Folded, it shows how many
/// tabs it hides.
pub fn group_header(
    id: impl Into<ElementId>,
    name: String,
    color: Hsla,
    collapsed: bool,
    count: usize,
    cx: &App,
) -> Stateful<Div> {
    let t = cx.theme();
    h_flex()
        .id(id)
        .flex_none()
        .h_full()
        .px_2p5()
        .gap_1p5()
        .text_xs()
        .font_medium()
        .cursor_pointer()
        .border_r_1()
        .border_color(t.border)
        .text_color(t.muted_foreground)
        .hover(|s| s.text_color(t.foreground))
        .child(div().flex_none().size(px(6.)).rounded_full().bg(color))
        .child(name)
        .when(collapsed, |el| {
            el.child(
                div()
                    .px_1()
                    .rounded(px(3.))
                    .bg(t.muted)
                    .text_color(t.muted_foreground)
                    .child(format!("{count}")),
            )
        })
}

/// The line across the top of a grouped tab: full colour on the active tab,
/// faint on the rest so the group still reads as one run.
pub fn group_stripe(color: Hsla, active: bool) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(if active { 2. } else { 1. }))
        .bg(if active { color } else { color.opacity(0.45) })
}
