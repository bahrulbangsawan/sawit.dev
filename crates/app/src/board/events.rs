//! The pointer on the canvas: GPUI's mouse events as FigCraft's pointer events
//! (`sawit_board::input`), and the engine's cursor as GPUI's.
//!
//! A press of the left button on the canvas starts a gesture with the tools, and a second quick
//! press is a double-click. Until the button comes up, its moves go to the tools wherever the
//! pointer is, off the canvas or out of the window too, and its release ends the gesture there:
//! listeners on the whole window, registered with each paint ([`track_drags`]), see those events
//! where the canvas can't. Without a button the pointer hovers, and leaving the canvas tells the
//! engine that nothing there is hovered any more. The right button is left for the context menu.
//!
//! The canvas pans by hand as FigCraft's own canvas does, outside the engine: a drag of the middle
//! button, whatever the tool, or of the left button while Space is held down (not while text is
//! edited, where Space types). The content follows the pointer until the button comes up, on the
//! canvas or off it, and the tools see none of it: no gesture, no undo step.

use figcraft_engine::PointerKind;
use figcraft_engine::tools::Cursor;
use gpui_kit::{
    Context, CursorStyle, DispatchPhase, KeyDownEvent, KeyUpEvent, Modifiers, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, WeakEntity, Window,
};
use sawit_board::input::{self, Mods, ResizeDir};
use sawit_board::view;

use super::BoardView;

/// A pan of the canvas by hand: the button that holds it, and where the pointer was last, in the
/// window.
#[derive(Clone, Copy, Debug)]
pub(super) struct Pan {
    button: MouseButton,
    last: Point<Pixels>,
}

impl BoardView {
    /// The left button went down on the canvas: the canvas takes the keyboard, and the tools get a
    /// press there, or a double-click for the second press of one. With Space held down it starts
    /// a pan instead. A drag or a pan whose release never reached the canvas (the tab was hidden
    /// meanwhile) ends first, where this press is.
    pub(super) fn on_left_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.end_drag(event.position, event.modifiers, cx);
        self.pan = None;
        if self.space_held {
            self.start_pan(MouseButton::Left, event.position, cx);
            return;
        }
        let kind = if event.click_count == 2 {
            PointerKind::DoubleClick
        } else {
            PointerKind::Down
        };
        self.dragging = true;
        self.send_pointer(kind, event.position, event.modifiers, cx);
    }

    /// The middle button went down on the canvas: the canvas takes the keyboard, and a pan starts
    /// there with any tool, unless the tools have a drag on.
    pub(super) fn on_middle_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if !self.dragging {
            self.start_pan(MouseButton::Middle, event.position, cx);
        }
    }

    /// The pointer moved over the canvas. With no drag or pan on it hovers; their moves come from
    /// the window's listener ([`track_drags`]), on the canvas or off it.
    pub(super) fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dragging && self.pan.is_none() {
            self.send_pointer(PointerKind::Hover, event.position, event.modifiers, cx);
        }
    }

    /// A key went down on the canvas: Space, without modifiers and with no text edited, is held
    /// for a pan, and taken, its repeats too. Whether it was.
    pub(super) fn hold_space(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        if keystroke.key != "space"
            || keystroke.modifiers.modified()
            || self.board().is_none()
            || self.editing_text()
        {
            return false;
        }
        if !std::mem::replace(&mut self.space_held, true) {
            cx.notify();
        }
        true
    }

    /// A key came up on the canvas: Space's no longer arms a pan. One under way goes on until its
    /// button comes up.
    pub(super) fn on_key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.let_go_of_space(cx);
        }
    }

    /// Space no longer arms a pan: it came up, or its release goes elsewhere, now that the canvas
    /// or its window has lost the keyboard.
    fn let_go_of_space(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.space_held) {
            cx.notify();
        }
    }

    /// Watches, from the first render on, for the canvas losing the keyboard in `window`, which
    /// lets go of Space. GPUI blurs the canvas when the focus moves elsewhere in the window, and
    /// when the window itself loses the keyboard to another.
    pub(super) fn watch_keyboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.keyboard_watch.is_none() {
            let watch = cx.on_blur(&self.focus, window, |this, _, cx| this.let_go_of_space(cx));
            self.keyboard_watch = Some(watch);
        }
    }

    /// A pan held by `button` starts at `position` in the window.
    fn start_pan(&mut self, button: MouseButton, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.pan = Some(Pan {
            button,
            last: position,
        });
        cx.notify();
    }

    /// The cursor over the canvas: a hand while it pans by hand, closed while a pan is on, else
    /// the engine's.
    pub(super) fn cursor(&self) -> CursorStyle {
        if self.pan.is_some() {
            CursorStyle::ClosedHand
        } else if self.space_held {
            CursorStyle::OpenHand
        } else {
            self.board().map_or(CursorStyle::Arrow, |board| {
                cursor_style(&board.session.tool.overlay.cursor)
            })
        }
    }

    /// The pointer went off the canvas (`hovered` is false): onto something drawn over it,
    /// elsewhere in the window, or out of the window.
    pub(super) fn on_hover(&mut self, hovered: &bool, _: &mut Window, cx: &mut Context<Self>) {
        if !hovered {
            self.pointer_left(cx);
        }
    }

    /// A move with a button down, anywhere: the pan's, which the content follows, or the move of
    /// the left button's drag, if one is on.
    fn drag_to(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if let Some(pan) = &mut self.pan {
            let travel = event.position - std::mem::replace(&mut pan.last, event.position);
            if let Some(view) = self.view_mut() {
                view::pan(view, f64::from(travel.x), f64::from(travel.y));
            }
            cx.notify();
        } else if self.dragging && event.pressed_button == Some(MouseButton::Left) {
            self.send_pointer(PointerKind::Move, event.position, event.modifiers, cx);
        }
    }

    /// A button came up, wherever that is: the pan it held ends, or the left button's drag does.
    fn release(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        if self.pan.is_some_and(|pan| pan.button == event.button) {
            self.pan = None;
            cx.notify();
        } else if event.button == MouseButton::Left {
            self.end_drag(event.position, event.modifiers, cx);
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

/// Registers, for the frame being painted, the listeners on the whole window that give a drag or a
/// pan begun on the canvas of `view` its moves and its release wherever they are: over the canvas,
/// elsewhere in the window, or out of it. They listen in the capture phase, before anything drawn
/// over the canvas can stop the events, which belong to the drag.
pub(super) fn track_drags(view: &WeakEntity<BoardView>, window: &mut Window) {
    let held = |button: Option<MouseButton>| {
        matches!(button, Some(MouseButton::Left | MouseButton::Middle))
    };
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && held(event.pressed_button) {
                let _ = view.update(cx, |view, cx| view.drag_to(event, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && held(Some(event.button)) {
                let _ = view.update(cx, |view, cx| view.release(event, cx));
            }
        }
    });
}

/// The system cursor showing FigCraft's `cursor`: the nearest one GPUI has. A resize's angle is in
/// degrees (ruling P2-C3); one that is no number shows the arrow.
fn cursor_style(cursor: &Cursor) -> CursorStyle {
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
pub(super) fn mods(modifiers: Modifiers) -> Mods {
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
