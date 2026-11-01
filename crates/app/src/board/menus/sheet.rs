//! The keyboard-shortcuts sheet (⌃⇧?, Help ▸ Keyboard shortcuts): a column of keys for each
//! group of commands.

use gpui_kit::component::*;
use gpui_kit::*;
use sawit_board::shortcuts::{self, ShortcutGroup};

use super::{SHEET_COLUMN, Sheet, guarded, keycap, live_sheet};
use crate::board::BoardView;

impl BoardView {
    /// The sheet, in the middle of the tab over a backdrop that takes the presses around it: a
    /// column for each group of keys, scrolling when they don't fit. It has the keyboard.
    pub(super) fn render_sheet(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.menus.sheet_open {
            return None;
        }
        if self.menus.sheet.is_none() {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            let groups = live_sheet(guarded("ui.shortcuts", shortcuts::sheet), cx);
            self.menus.sheet = Some(Sheet { focus, groups });
        }
        let sheet = self.menus.sheet.as_ref()?;
        let theme = cx.theme();
        let columns: Vec<AnyElement> = (sheet.groups.iter())
            .map(|group| sheet_column(group, cx))
            .collect();
        Some(
            div()
                .id("board-sheet-backdrop")
                // The presses around the sheet are its own: one closes it, and none reaches what
                // is under it.
                .occlude()
                .absolute()
                .inset_0()
                .p_6()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui_kit::black().opacity(0.45))
                .child(
                    v_flex()
                        .id("board-sheet")
                        .debug_selector(|| "board-sheet".into())
                        .track_focus(&sheet.focus)
                        .w_full()
                        .max_w(px(1040.))
                        .max_h_full()
                        .overflow_hidden()
                        .rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .shadow_lg()
                        .on_key_down(cx.listener(|view, event: &KeyDownEvent, _, cx| {
                            let keystroke = &event.keystroke;
                            if keystroke.key == "escape" && !keystroke.modifiers.modified() {
                                view.close_menus(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_mouse_down_out(cx.listener(|view, _: &MouseDownEvent, _, cx| {
                            view.close_menus(cx);
                        }))
                        .child(
                            div()
                                .flex_shrink_0()
                                .px_4()
                                .py_3()
                                .border_b_1()
                                .border_color(theme.border)
                                .text_sm()
                                .font_medium()
                                .child("Keyboard shortcuts"),
                        )
                        .child(
                            div()
                                .id("board-sheet-groups")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p_4()
                                .child(
                                    div()
                                        .flex()
                                        .flex_wrap()
                                        .items_start()
                                        .gap_x_8()
                                        .gap_y_6()
                                        .children(columns),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// The sheet's column of `group`: its name, then a row for each command, with its label and its
/// keys, each alternative a key of its own.
fn sheet_column(group: &ShortcutGroup, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let name = group.name.clone();
    let rows = group.rows.iter().map(|row| {
        let id = row.id;
        let keys = (row.keys.split(" or ")).map(|keys| keycap(keys.to_owned(), cx));
        h_flex()
            .debug_selector(move || format!("board-sheet-{id}"))
            .w_full()
            .gap_2()
            .justify_between()
            .text_sm()
            .child(div().min_w_0().truncate().child(row.label.clone()))
            .child(h_flex().flex_shrink_0().gap_1().children(keys))
    });
    v_flex()
        .debug_selector(move || format!("board-sheet-{name}"))
        .w(px(SHEET_COLUMN))
        .gap_1()
        .child(
            div()
                .pb_1()
                .text_xs()
                .font_medium()
                .text_color(theme.muted_foreground)
                .child(group.name.clone()),
        )
        .children(rows)
        .into_any_element()
}
