//! The pointer on the canvas: GPUI's mouse events as FigCraft's pointer events
//! (`sawit_board::input`), and the engine's cursor as GPUI's.
//!
//! A press of the left button on the canvas starts a gesture with the tools, and a second quick
//! press is a double-click. Until the button comes up, its moves go to the tools wherever the
//! pointer is, off the canvas or out of the window too, and its release ends the gesture there:
//! listeners on the whole window, registered with each paint ([`track_drags`]), see those events
//! where the canvas can't. Without a button the pointer hovers, and leaving the canvas tells the
//! engine that nothing there is hovered any more. The right button is left for the context menu.

use figcraft_engine::PointerKind;
use figcraft_engine::tools::Cursor;
use gpui_kit::{
    Context, CursorStyle, DispatchPhase, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, WeakEntity, Window,
};
use sawit_board::input::{self, Mods, ResizeDir};

use super::BoardView;

impl BoardView {
    /// The left button went down on the canvas: the canvas takes the keyboard, and the tools get a
    /// press there, or a double-click for the second press of one. A drag whose release never
    /// reached the canvas (the tab was hidden meanwhile) ends first, where this press is.
    pub(super) fn on_left_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.end_drag(event.position, event.modifiers, cx);
        let kind = if event.click_count == 2 {
            PointerKind::DoubleClick
        } else {
            PointerKind::Down
        };
        self.dragging = true;
        self.send_pointer(kind, event.position, event.modifiers, cx);
    }

    /// The pointer moved over the canvas. With no drag on it hovers; a drag's moves go to the tools
    /// from the window's listener ([`track_drags`]), on the canvas or off it.
    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dragging {
            self.send_pointer(PointerKind::Hover, event.position, event.modifiers, cx);
        }
    }

    /// The pointer went off the canvas (`hovered` is false): onto something drawn over it,
    /// elsewhere in the window, or out of the window.
    pub(super) fn on_hover(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            self.pointer_left(cx);
        }
    }

    /// A move with the left button down, anywhere: the drag's move, if one is on.
    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.dragging {
            self.send_pointer(PointerKind::Move, event.position, event.modifiers, cx);
        }
    }

    /// The left button came up at `position` in the window, wherever that is: the drag on, if
    /// any, ends there, once. Released off the canvas, it leaves nothing hovered on it either.
    fn end_drag(&mut self, position: Point<Pixels>, modifiers: Modifiers, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.dragging) {
            return;
        }
        self.send_pointer(PointerKind::Up, position, modifiers, cx);
        if !self.bounds.contains(&position) {
            self.pointer_left(cx);
        }
    }

    /// Sends the tools a pointer event of `kind` at `position` in the window, with `modifiers`
    /// held, then carries out what they asked of the view, and redraws. The engine's error shows
    /// as a toast.
    fn send_pointer(
        &mut self,
        kind: PointerKind,
        position: Point<Pixels>,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let local = position - self.bounds.origin;
        let local = (f64::from(local.x), f64::from(local.y));
        let Some(board) = self.board_mut() else {
            return;
        };
        let Ok(ds) = board.session.ds() else {
            return;
        };
        let event = input::pointer_event(kind, local, &ds.view, mods(modifiers));
        let requests = match input::pointer(board, &event) {
            Ok(requests) => requests,
            Err(error) => {
                self.show_toast(error.to_string(), cx);
                Vec::new()
            }
        };
        self.handle_ui_requests(requests, cx);
        cx.notify();
    }

    /// The pointer left the canvas: what hovering there showed goes, unless a drag on holds it
    /// until its release.
    fn pointer_left(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            return;
        }
        let Some(board) = self.board_mut() else {
            return;
        };
        if let Err(error) = input::pointer_left(board) {
            self.show_toast(error.to_string(), cx);
        }
        cx.notify();
    }
}

/// Registers, for the frame being painted, the listeners on the whole window that give a drag
/// begun on the canvas of `view` its moves and its release wherever they are: over the canvas,
/// elsewhere in the window, or out of it. They listen in the capture phase, before anything drawn
/// over the canvas can stop the events, which belong to the drag.
pub(super) fn track_drags(view: &WeakEntity<BoardView>, window: &mut Window) {
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.pressed_button == Some(MouseButton::Left) {
                let _ = view.update(cx, |view, cx| view.drag_to(event, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = view.update(cx, |view, cx| {
                    view.end_drag(event.position, event.modifiers, cx)
                });
            }
        }
    });
}

/// The system cursor showing FigCraft's `cursor`: the nearest one GPUI has. A resize's angle is in
/// degrees (ruling P2-C3); one that is no number shows the arrow.
pub(super) fn cursor_style(cursor: &Cursor) -> CursorStyle {
    match *cursor {
        Cursor::Default | Cursor::Move | Cursor::Comment => CursorStyle::Arrow,
        Cursor::Crosshair | Cursor::Pen(_) | Cursor::Eyedropper | Cursor::Rotate(_) => {
            CursorStyle::Crosshair
        }
        Cursor::Resize(degrees) if degrees.is_finite() => {
            match input::resize_dir(degrees.to_radians()) {
                ResizeDir::Horizontal => CursorStyle::ResizeLeftRight,
                ResizeDir::Vertical => CursorStyle::ResizeUpDown,
                ResizeDir::DiagDown => CursorStyle::ResizeUpLeftDownRight,
                ResizeDir::DiagUp => CursorStyle::ResizeUpRightDownLeft,
            }
        }
        Cursor::Resize(_) => CursorStyle::Arrow,
        Cursor::Grab => CursorStyle::OpenHand,
        Cursor::Grabbing => CursorStyle::ClosedHand,
        Cursor::Text => CursorStyle::IBeam,
        Cursor::Copy => CursorStyle::DragCopy,
        Cursor::Pointer => CursorStyle::PointingHand,
        Cursor::NotAllowed => CursorStyle::OperationNotAllowed,
    }
}

/// GPUI's modifier keys as FigCraft names them: ⌘ is `cmd`.
fn mods(modifiers: Modifiers) -> Mods {
    Mods {
        shift: modifiers.shift,
        alt: modifiers.alt,
        cmd: modifiers.platform,
        ctrl: modifiers.control,
    }
}

#[cfg(test)]
mod tests {
    use figcraft_engine::tools::PenCursor;

    use super::*;

    #[test]
    fn resize_cursors_take_degrees() {
        for (degrees, style) in [
            (0.0, CursorStyle::ResizeLeftRight),
            (90.0, CursorStyle::ResizeUpDown),
            (45.0, CursorStyle::ResizeUpLeftDownRight),
            (135.0, CursorStyle::ResizeUpRightDownLeft),
            // A handle on the other side pulls along the same line.
            (180.0, CursorStyle::ResizeLeftRight),
            (-45.0, CursorStyle::ResizeUpRightDownLeft),
        ] {
            assert_eq!(cursor_style(&Cursor::Resize(degrees)), style, "{degrees}");
        }
        for degrees in [f64::NAN, f64::INFINITY] {
            assert_eq!(cursor_style(&Cursor::Resize(degrees)), CursorStyle::Arrow);
        }
    }

    #[test]
    fn every_engine_cursor_has_a_system_cursor() {
        for (cursor, style) in [
            (Cursor::Default, CursorStyle::Arrow),
            (Cursor::Move, CursorStyle::Arrow),
            (Cursor::Comment, CursorStyle::Arrow),
            (Cursor::Crosshair, CursorStyle::Crosshair),
            (Cursor::Pen(PenCursor::New), CursorStyle::Crosshair),
            (Cursor::Pen(PenCursor::Bend), CursorStyle::Crosshair),
            (Cursor::Eyedropper, CursorStyle::Crosshair),
            (Cursor::Rotate(45.0), CursorStyle::Crosshair),
            (Cursor::Grab, CursorStyle::OpenHand),
            (Cursor::Grabbing, CursorStyle::ClosedHand),
            (Cursor::Text, CursorStyle::IBeam),
            (Cursor::Copy, CursorStyle::DragCopy),
            (Cursor::Pointer, CursorStyle::PointingHand),
            (Cursor::NotAllowed, CursorStyle::OperationNotAllowed),
        ] {
            assert_eq!(cursor_style(&cursor), style, "{cursor:?}");
        }
    }

    #[test]
    fn command_is_cmd_and_control_is_ctrl() {
        let shift_cmd = mods(Modifiers {
            platform: true,
            shift: true,
            ..Modifiers::default()
        });
        assert!(shift_cmd.cmd && shift_cmd.shift && !shift_cmd.alt && !shift_cmd.ctrl);
        let control = mods(Modifiers {
            control: true,
            ..Modifiers::default()
        });
        assert!(control.ctrl && !control.cmd);
    }
}
