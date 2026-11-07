//! What the tab menus share (the file tab menu, the terminal's tab and pane menus): an item with
//! its keys on the right, read from the keymap, the way the file tab menu always drew them.

use gpui_kit::component::h_flex;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::*;

/// An item labelled `label` with `keys` (as a Mac menu writes them, `⌘K W`) on the right in
/// `muted`; no keys, nothing there. A long label is cut short with an ellipsis.
pub fn keyed(label: impl Into<SharedString>, keys: SharedString, muted: Hsla) -> PopupMenuItem {
    let label = label.into();
    // no icon: the caller gives the item its icon (`.icon(…)`).
    PopupMenuItem::element(move |_, _| {
        h_flex()
            .w_full()
            .gap_6()
            .justify_between()
            .child(
                div()
                    .min_w_0()
                    .max_w(px(LABEL_MAX))
                    .overflow_hidden()
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(label.clone()),
            )
            .child(div().flex_none().text_color(muted).child(keys.clone()))
    })
}

/// The widest a menu item's label gets before it is cut short (a long project name).
pub const LABEL_MAX: f32 = 260.;
