//! Tab groups, one per open project, shared by the center tabs and the
//! terminal dock: each group has its project's colour and a header that
//! folds the group's tabs away.

use std::path::Path;

use gpui_kit::assets::IconName as Lucide;
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

/// The label in front of a group's tabs; folded, it shows how many it hides.
pub fn group_header(
    id: impl Into<ElementId>,
    name: String,
    color: Hsla,
    collapsed: bool,
    count: usize,
    cx: &App,
) -> Stateful<Div> {
    h_flex()
        .id(id)
        .flex_none()
        .h_full()
        .px_1p5()
        .gap_1()
        .cursor_pointer()
        .border_r_1()
        .border_color(cx.theme().border)
        .child(
            h_flex()
                .px_1p5()
                .gap_1()
                .rounded(px(4.))
                .text_xs()
                .font_semibold()
                .bg(color.opacity(0.18))
                .text_color(color)
                .child(
                    Icon::new(if collapsed {
                        Lucide::ChevronRight
                    } else {
                        Lucide::ChevronDown
                    })
                    .xsmall(),
                )
                .child(name)
                .when(collapsed, |el| el.child(format!("{count}"))),
        )
}

/// The thin coloured line across the top of a grouped tab.
pub fn group_stripe(color: Hsla) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(px(2.))
        .bg(color)
}
