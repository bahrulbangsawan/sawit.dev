//! The mouse on a terminal: selecting and copying, reports to programs that take the mouse,
//! scrolling, right-clicks and dropped files.

use std::path::PathBuf;

use gpui_kit::*;
use sawit_core::terminal::{
    MouseAction, MouseButton as TermButton, SelectKind, drop_bytes, mouse_report,
};

use super::{TerminalEvent, TerminalView};

/// Where a right-click goes.
pub enum RightClick {
    /// Opens the pane menu.
    Menu,
    /// Goes to the program as a mouse report.
    Program,
    /// Goes nowhere: the pane sends right-clicks on, but its program does
    /// not take the mouse.
    Nothing,
}

/// Where a right-click goes. Shift always opens the menu; otherwise it goes
/// to the program when the pane is set to send right-clicks on.
pub fn right_click_target(
    send_right_clicks: bool,
    shift: bool,
    program_has_mouse: bool,
) -> RightClick {
    if shift || !send_right_clicks {
        RightClick::Menu
    } else if program_has_mouse {
        RightClick::Program
    } else {
        RightClick::Nothing
    }
}

impl TerminalView {
    /// Grid cell under `position`, clamped to the screen, and whether the
    /// mouse is over its right half.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, bool)> {
        let (origin, cell) = self.layout.get()?;
        let size = self.session.size();
        let x = ((position.x - origin.x) / cell.width).max(0.);
        let row = ((position.y - origin.y) / cell.height).floor().max(0.) as usize;
        let col = x.floor() as usize;
        let past_end = col >= size.cols as usize;
        Some((
            col.min(size.cols as usize - 1),
            row.min(size.rows as usize - 1),
            past_end || x.fract() >= 0.5,
        ))
    }

    pub(super) fn start_selection(&mut self, position: Point<Pixels>, clicks: usize) {
        if let Some((col, row, right)) = self.cell_at(position) {
            self.session
                .select_start(col, row, right, SelectKind::from_clicks(clicks));
            self.selecting = true;
        }
    }

    pub(super) fn extend_selection(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        if self.selecting
            && let Some((col, row, right)) = self.cell_at(position)
        {
            self.session.select_extend(col, row, right);
            cx.notify();
        }
    }

    /// Ends a drag: whatever got selected goes to the clipboard.
    pub(super) fn finish_selection(&mut self, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.selecting) {
            return;
        }
        if let Some(text) = self.session.selection_text() {
            self.copy(text, cx);
        }
        cx.notify();
    }

    /// Types dropped files (from Finder or the file tree) as quoted paths.
    pub(super) fn drop_paths(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if paths.is_empty() || self.exited {
            return;
        }
        self.touched = true;
        self.write(drop_bytes(paths, self.modes()));
        self.focus(window, cx);
        cx.emit(TerminalEvent::Focused);
        cx.notify();
    }

    pub(super) fn send_mouse(
        &self,
        action: MouseAction,
        position: Point<Pixels>,
        shift: bool,
    ) -> bool {
        let Some((col, row, _)) = self.cell_at(position) else {
            return false;
        };
        match mouse_report(action, col, row, shift, self.modes()) {
            Some(bytes) => {
                self.session.write(bytes);
                true
            }
            None => false,
        }
    }

    pub(super) fn on_scroll(
        &mut self,
        event: &ScrollEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, cell)) = self.layout.get() else {
            return;
        };
        let pixels = f32::from(event.delta.pixel_delta(cell.height).y);
        let lines = self.scroll.add(pixels, f32::from(cell.height));
        if lines == 0 {
            return;
        }
        let modes = self.modes();
        let action = if lines > 0 {
            MouseAction::WheelUp
        } else {
            MouseAction::WheelDown
        };
        if modes.mouse_report || modes.mouse_drag || modes.mouse_motion {
            for _ in 0..lines.unsigned_abs().min(10) {
                self.send_mouse(action, event.position, false);
            }
        } else if modes.alt_screen && modes.alternate_scroll {
            let arrow: &[u8] = match (lines > 0, modes.app_cursor) {
                (true, true) => b"\x1bOA",
                (true, false) => b"\x1b[A",
                (false, true) => b"\x1bOB",
                (false, false) => b"\x1b[B",
            };
            self.session
                .write(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            self.session.scroll(lines);
        }
        cx.notify();
    }
}

/// Files dragged in from another app (a screenshot from Finder, for Claude Code) come with no
/// mouse move: while another app drags, macOS sends the window only the drag's own events, and
/// GPUI turns those into mouse moves and a mouse up without counting them as the mouse being
/// used. After typing it still takes the keyboard to be in use, so nothing reads as hovered
/// and a drop let go of straight away types nothing (one held still over the terminal for a
/// moment got through). A mouse move where the drag is, sent before the drop comes, tells GPUI
/// the pointer is in use again.
pub(super) fn pointer_moved_for_drop(position: Point<Pixels>, window: &mut Window, cx: &mut App) {
    if !window.last_input_was_keyboard() {
        return;
    }
    window.defer(cx, move |window, cx| {
        // Every pane on screen sees the drag: the first one's move is enough.
        if window.last_input_was_keyboard() {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: Some(MouseButton::Left),
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
        }
    });
}

pub(super) fn to_term_button(button: MouseButton) -> Option<TermButton> {
    match button {
        MouseButton::Left => Some(TermButton::Left),
        MouseButton::Middle => Some(TermButton::Middle),
        MouseButton::Right => Some(TermButton::Right),
        _ => None,
    }
}
