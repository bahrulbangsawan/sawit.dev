//! Copy and paste between canvases, other apps and the text being edited.

use super::*;

/// The text on the system clipboard.
fn system_text(cx: &TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// Puts `text` on the system clipboard, as copying it in another app does.
fn copy_in_another_app(cx: &TestAppContext, text: &str) {
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.into()));
}

fn no_toast(view: &Entity<BoardView>, cx: &TestAppContext) {
    let toast = view.read_with(cx, |view, _| view.toast.clone());
    assert_eq!(toast, None);
}

#[gpui_kit::test]
fn copy_and_paste_act_on_the_text_being_edited(cx: &mut TestAppContext) {
    let tmp = Scratch::new("text-clipboard");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    let hello = json!({
        "op": "command", "command": "board.sticky",
        "params": {"x": 0, "y": 0, "text": "hello"},
    });
    let made = run_op(&view, hello, &mut cx);
    // Typing starts with all of "hello" selected.
    let note = type_in(&view, &made, &mut cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(system_text(&cx).as_deref(), Some("hello"));
    copy_in_another_app(&cx, "x");
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(reads(&view, note, &cx), "x");
    // ⌘X takes the selected text out, onto the system clipboard.
    cx.simulate_keystrokes("cmd-a cmd-x");
    assert_eq!(system_text(&cx).as_deref(), Some("x"));
    assert_eq!(reads(&view, note, &cx), "");
    // Only the text changed: no layer was copied or pasted, and the typing goes on.
    assert_eq!(layers(&view, &cx), [note]);
    assert!(view.read_with(&cx, |view, _| view.editing_text()));
    no_toast(&view, &cx);
}

#[gpui_kit::test]
fn layers_copied_or_cut_in_one_canvas_paste_in_another(cx: &mut TestAppContext) {
    let _turn = clipboard_turn();
    let tmp = Scratch::new("copy-across");
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let mut canvas = |name: &str| {
        let (window, view) = open_in_window(cx, &tmp.0.join(name));
        next_frame(cx, &view, None);
        (view, VisualTestContext::from_window(window.into(), cx))
    };
    let (one, mut one_cx) = canvas("one.sawit");
    let (other, mut other_cx) = canvas("other.sawit");
    let sticky = |text: &str| {
        json!({
            "op": "command", "command": "board.sticky",
            "params": {"x": 0, "y": 0, "text": text},
        })
    };
    run_op(&one, sticky("copied"), &mut one_cx);
    focus_canvas(&one, &mut one_cx);
    one_cx.simulate_keystrokes("cmd-c");
    // Other apps see the copy's marker, which says the copy is the canvas's newest.
    let marker = system_text(&one_cx);
    assert!(
        marker
            .as_deref()
            .is_some_and(|m| m.starts_with("sawit-canvas:")),
        "{marker:?}"
    );
    assert_eq!(marker, sawit_board::clipboard::marker());

    focus_canvas(&other, &mut other_cx);
    other_cx.simulate_keystrokes("cmd-v");
    let [pasted] = layers(&other, &other_cx)[..] else {
        panic!("layers {:?}", layers(&other, &other_cx))
    };
    assert_eq!(reads(&other, pasted, &other_cx), "copied");
    // ⇧⌘V pastes it again, over the selection.
    other_cx.simulate_keystrokes("shift-cmd-v");
    assert_eq!(layers(&other, &other_cx).len(), 2);

    // A cut leaves its canvas, and pastes in the other one.
    run_op(&one, sticky("cut"), &mut one_cx);
    one_cx.simulate_keystrokes("cmd-x");
    assert_eq!(layers(&one, &one_cx).len(), 1);
    assert_ne!(system_text(&one_cx), marker);
    other_cx.simulate_keystrokes("cmd-v");
    let added = layers(&other, &other_cx);
    assert_eq!(added.len(), 3);
    assert_eq!(reads(&other, added[2], &other_cx), "cut");
    no_toast(&one, &one_cx);
    no_toast(&other, &other_cx);
}

#[gpui_kit::test]
fn paste_takes_what_another_app_copied_since(cx: &mut TestAppContext) {
    let _turn = clipboard_turn();
    let tmp = Scratch::new("paste-newer");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    run_op(&view, sticky_op(), &mut cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-c");
    // Text copied in another app since: a text layer, not the sticky again.
    copy_in_another_app(&cx, "hello");
    cx.simulate_keystrokes("cmd-v");
    let [_, text] = layers(&view, &cx)[..] else {
        panic!("layers {:?}", layers(&view, &cx))
    };
    assert_eq!(reads(&view, text, &cx), "hello");
    // An image copied in another app: an image layer.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let png = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, png);
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_image(&png));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(layers(&view, &cx).len(), 3);
    let images = view.read_with(&cx, |view, _| {
        view.board().unwrap().doc().unwrap().images.len()
    });
    assert_eq!(images, 1);
    no_toast(&view, &cx);
}

#[gpui_kit::test]
fn a_paste_with_nothing_to_paste_says_nothing(cx: &mut TestAppContext) {
    let _turn = clipboard_turn();
    // The app's copy holds no layers, as before anything is copied, and neither does the
    // system clipboard.
    sawit_board::clipboard::after_copy(&BoardDoc::new_board("Canvas").unwrap());
    let tmp = Scratch::new("paste-nothing");
    let (view, mut cx) = painted_canvas(cx, &tmp);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("cmd-v shift-cmd-v");
    assert!(layers(&view, &cx).is_empty());
    // A copy's marker that Sawit left on the system clipboard before a restart is no text.
    copy_in_another_app(&cx, "sawit-canvas:999999");
    cx.simulate_keystrokes("cmd-v");
    assert!(layers(&view, &cx).is_empty());
    // Paste to replace, with a layer selected to replace.
    run_op(&view, sticky_op(), &mut cx);
    cx.simulate_keystrokes("shift-cmd-r");
    assert_eq!(layers(&view, &cx).len(), 1);
    no_toast(&view, &cx);
}
