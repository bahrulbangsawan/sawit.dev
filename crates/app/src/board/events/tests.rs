//! Tests of the pointer's translation for FigCraft: its cursors and its modifier keys.

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
