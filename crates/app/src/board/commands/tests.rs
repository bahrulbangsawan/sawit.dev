//! Tests of the commands the canvas carries out itself: their dialogs, files, clipboard and toasts.

use std::path::{Path, PathBuf};

use figcraft_doc::NodeId;
use gpui_kit::{ClipboardEntry, Entity, ImageFormat, TestAppContext, VisualTestContext};
use serde_json::json;

// Not `super::*`: gpui-kit's `test` attribute would come with it, in place of the standard one.
use super::super::BoardView;
use super::super::tests::{
    Scratch, chrome_canvas, chrome_canvas_at, layers, names, run_op, saved_layers, sticky_op,
};

/// Runs command `id` as a menu or its shortcut hands it over.
fn run_ui(view: &Entity<BoardView>, id: &'static str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| view.update(cx, |view, cx| super::run_ui(view, id, window, cx)));
}

fn toast(view: &Entity<BoardView>, cx: &TestAppContext) -> Option<String> {
    view.read_with(cx, |view, _| view.toast.as_ref().map(ToString::to_string))
}

fn selection(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<NodeId> {
    view.read_with(cx, |view, _| view.board().unwrap().session.selection())
}

/// Runs engine command `id` with `params` as an agent does, and returns the layer it made.
fn make(
    view: &Entity<BoardView>,
    id: &str,
    params: serde_json::Value,
    cx: &mut TestAppContext,
) -> NodeId {
    let op = json!({"op": "command", "command": id, "params": params});
    let made = run_op(view, op, cx);
    figcraft_engine::params::id_of(&made["result"]["id"]).unwrap()
}

/// The text on the system clipboard.
fn clipboard_text(cx: &TestAppContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

/// Runs Save as…, answers its dialog with `answer`, and returns the toast after.
fn save_as(
    view: &Entity<BoardView>,
    answer: PathBuf,
    cx: &mut VisualTestContext,
) -> Option<String> {
    run_ui(view, "file.saveAs", cx);
    cx.simulate_new_path_selection(|_| Some(answer));
    cx.run_until_parked();
    toast(view, cx)
}

/// A PNG file of a `width` × `height` red picture.
fn red_png(width: u32, height: u32) -> Vec<u8> {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(width, height, image::Rgba([255, 0, 0, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    png
}

/// One red rectangle, 100 × 80, as an SVG file holds it.
const RECT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80"><rect width="100" height="80" fill="#f00"/></svg>"##;

/// Review Focus 2 and ruling P6-C12: a menu command the engine refuses says why, in a toast,
/// in plain words: the user's screenshot showed the engine's words for agents, "command
/// `object.frameSelection` is not available right now: nothing is selected (pass `ids`)".
#[gpui_kit::test]
fn disabled_command_toasts_reason(cx: &mut TestAppContext) {
    let tmp = Scratch::new("disabled-command");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    for (id, said) in [
        ("edit.pasteProperties", "No copied properties"),
        ("object.frameSelection", "Nothing is selected"),
        ("component.goToMain", "Select an instance"),
    ] {
        cx.update(|window, cx| view.update(cx, |view, cx| view.run_menu(id, window, cx)));
        assert_eq!(toast(&view, &cx).as_deref(), Some(said), "{id}");
    }
}

/// Review Focus 3: Export frames to PDF on a page without frames says why there is no PDF.
#[gpui_kit::test]
fn frames_pdf_without_frames_explains(cx: &mut TestAppContext) {
    let tmp = Scratch::new("frames-pdf");
    // A project's canvas, so that an export that went through would stay in the test's own
    // folder rather than go into Sawit's.
    let project = tmp.0.join("proj");
    let (view, mut cx) = chrome_canvas_at(cx, &sawit_core::nyawit::board_file(&project));
    run_ui(&view, "file.exportFramesPdf", &mut cx);
    let toast = toast(&view, &cx).unwrap_or_default();
    assert!(toast.contains("no visible frames"), "{toast}");
    assert!(!sawit_core::nyawit::exports_dir(&project).exists());
}

/// Review Focus 4: a Save as or an Import whose dialog is cancelled changes nothing, and says
/// nothing.
#[gpui_kit::test]
fn cancelled_dialog_is_a_no_op(cx: &mut TestAppContext) {
    let tmp = Scratch::new("cancelled-dialog");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    run_op(&view, sticky_op(), &mut cx);
    view.update(&mut cx, |view, _| view.flush());
    let canvas = tmp.0.join("canvas.sawit");
    let (files, saved) = (names(&tmp.0), std::fs::read(&canvas).unwrap());
    run_ui(&view, "file.saveAs", &mut cx);
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert_eq!(names(&tmp.0), files);
    assert_eq!(std::fs::read(&canvas).unwrap(), saved);
    assert_eq!(toast(&view, &cx), None);
    let on_page = layers(&view, &cx);
    for id in ["file.import", "file.placeImage"] {
        run_ui(&view, id, &mut cx);
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        assert_eq!(layers(&view, &cx), on_page, "{id}");
        assert_eq!(toast(&view, &cx), None, "{id}");
    }
}

#[gpui_kit::test]
fn copy_as_css_writes_clipboard(cx: &mut TestAppContext) {
    let tmp = Scratch::new("copy-as-css");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let shape = json!({"shape": "rectangle", "x": 0, "y": 0, "width": 120, "height": 80});
    let shape = make(&view, "board.shape", shape, &mut cx);
    let select = json!({"op": "command", "command": "select.set", "params": {"ids": [shape]}});
    run_op(&view, select, &mut cx);
    run_ui(&view, "edit.copyAsCss", &mut cx);
    let css = clipboard_text(&cx).unwrap_or_default();
    assert!(css.contains("width"), "{css}");
    assert_eq!(toast(&view, &cx).as_deref(), Some("Copied as CSS"));
}

/// Copy as text puts the text on the system clipboard, and Copy as PNG an image. With nothing
/// to copy, the clipboard keeps what it held, and the toast says why.
#[gpui_kit::test]
fn copy_as_puts_text_or_a_png_on_the_clipboard(cx: &mut TestAppContext) {
    let tmp = Scratch::new("copy-as");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let note = json!({"x": 0, "y": 0, "text": "Ship it"});
    make(&view, "board.sticky", note, &mut cx);
    run_ui(&view, "edit.copyAsText", &mut cx);
    assert_eq!(clipboard_text(&cx).as_deref(), Some("Ship it"));
    assert_eq!(toast(&view, &cx).as_deref(), Some("Copied as text"));
    run_ui(&view, "edit.copyAsPng", &mut cx);
    let png = |cx: &VisualTestContext| {
        let item = cx.read_from_clipboard()?;
        item.entries().iter().find_map(|entry| match entry {
            ClipboardEntry::Image(image) if matches!(image.format, ImageFormat::Png) => {
                Some(image.bytes.clone())
            }
            _ => None,
        })
    };
    let copied = png(&cx).expect("a PNG on the clipboard");
    assert!(copied.starts_with(b"\x89PNG"), "{:02x?}", &copied[..8]);
    assert_eq!(toast(&view, &cx).as_deref(), Some("Copied as PNG"));
    let none = json!({"op": "command", "command": "select.none"});
    run_op(&view, none, &mut cx);
    run_ui(&view, "edit.copyAsText", &mut cx);
    assert_eq!(toast(&view, &cx).as_deref(), Some("Nothing is selected"));
    assert_eq!(png(&cx), Some(copied));
}

/// Save as… writes a copy where its dialog says, in the canvas file's folder to begin with,
/// and the canvas goes on saving to its own file.
#[gpui_kit::test]
fn save_as_writes_a_copy_and_the_canvas_keeps_its_file(cx: &mut TestAppContext) {
    let tmp = Scratch::new("save-as");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    run_op(&view, sticky_op(), &mut cx);
    let copy = tmp.0.join("copy.sawit");
    run_ui(&view, "file.saveAs", &mut cx);
    let folder = tmp.0.clone();
    let answer = copy.clone();
    cx.simulate_new_path_selection(move |shown| {
        assert_eq!(shown, folder.as_path());
        Some(answer)
    });
    cx.run_until_parked();
    assert_eq!(saved_layers(&copy), 1);
    assert_eq!(
        toast(&view, &cx).as_deref(),
        Some("Saved a copy as copy.sawit")
    );
    let canvas = tmp.0.join("canvas.sawit");
    assert_eq!(
        view.read_with(&cx, |view, _| view.path().to_path_buf()),
        canvas
    );
    run_op(&view, sticky_op(), &mut cx);
    view.update(&mut cx, |view, _| view.flush());
    assert_eq!(saved_layers(&canvas), 2);
    assert_eq!(saved_layers(&copy), 1);
}

/// Save as… onto the canvas's own file saves it, as Save does. A name without an extension
/// is given `.sawit`, and another extension is refused, the canvas engine's own included.
#[gpui_kit::test]
fn save_as_writes_only_canvas_files(cx: &mut TestAppContext) {
    let tmp = Scratch::new("save-as-names");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    run_op(&view, sticky_op(), &mut cx);
    let canvas = tmp.0.join("canvas.sawit");
    assert_eq!(
        save_as(&view, canvas.clone(), &mut cx).as_deref(),
        Some("Saved")
    );
    assert_eq!(saved_layers(&canvas), 1);
    let bare = tmp.0.join("copy");
    assert_eq!(
        save_as(&view, bare.clone(), &mut cx).as_deref(),
        Some("Saved a copy as copy.sawit")
    );
    assert_eq!(saved_layers(&tmp.0.join("copy.sawit")), 1);
    assert!(!bare.exists());
    // That name again: the file is there now, and only its full name replaces it.
    let refused = save_as(&view, bare, &mut cx).unwrap_or_default();
    assert!(refused.contains("already"), "{refused}");
    // Regression: the engine's own extension is refused like any other, not written.
    for name in ["copy.png", "copy.figcraft"] {
        let other = tmp.0.join(name);
        let refused = save_as(&view, other.clone(), &mut cx).unwrap_or_default();
        assert!(refused.contains(".sawit"), "{name}: {refused}");
        assert!(!other.exists(), "{name}");
    }
}

/// Every command the policy hands to the canvas has its own arm in `run_ui`: on an empty
/// canvas, each asks for a file, saves, or says why there is nothing to export or copy, and
/// none says it is coming.
#[gpui_kit::test]
fn every_ui_command_is_carried_out(cx: &mut TestAppContext) {
    let tmp = Scratch::new("every-ui-command");
    // A project's canvas: an export would stay in the test's own folder.
    let project = tmp.0.join("proj");
    let (view, mut cx) = chrome_canvas_at(cx, &sawit_core::nyawit::board_file(&project));
    for &id in sawit_board::policy::UI_COMMANDS {
        view.update(&mut cx, |view, _| view.toast = None);
        run_ui(&view, id, &mut cx);
        assert_ne!(toast(&view, &cx).as_deref(), Some("Coming soon"), "{id}");
    }
    // Import, Place image and Save as asked for their files.
    for _ in 0..2 {
        cx.simulate_path_prompt_response(|_| None);
    }
    cx.simulate_new_path_selection(|_| None);
    cx.run_until_parked();
    assert!(!sawit_core::nyawit::exports_dir(&project).exists());
}

#[test]
fn one_file_under_two_names_is_the_same_file() {
    let tmp = Scratch::new("same-file");
    let (file, link, other) = (
        tmp.0.join("canvas.sawit"),
        tmp.0.join("link.sawit"),
        tmp.0.join("other.sawit"),
    );
    std::fs::write(&file, "a").unwrap();
    std::fs::write(&other, "a").unwrap();
    std::os::unix::fs::symlink(&file, &link).unwrap();
    assert!(super::same_file(&file, &file));
    assert!(super::same_file(&link, &file));
    assert!(!super::same_file(&other, &file));
    // A file that isn't there yet is only itself.
    let new = tmp.0.join("new.sawit");
    assert!(super::same_file(&new, &new));
    assert!(!super::same_file(&new, &file));
}

#[test]
fn save_as_begins_in_the_project_or_the_canvas_files_folder() {
    let (app, home) = (|| PathBuf::from("/app"), || PathBuf::from("/home/me"));
    for (canvas, folder) in [
        ("/proj/.nyawit/canvas.sawit", "/proj"),
        // The canvas without a project is in Sawit's own folder, which is no place for a copy.
        ("/app/canvas.sawit", "/home/me"),
        ("/proj/designs/card.sawit", "/proj/designs"),
    ] {
        let begins = super::save_as_folder(Path::new(canvas), app, home);
        assert_eq!(begins, Path::new(folder), "{canvas}");
    }
}

/// Import from file… brings in each file chosen and selects what they made; Place image…
/// takes images only, and a file it can't bring in says why by its name while the others
/// still come in.
#[gpui_kit::test]
fn import_brings_in_each_file_chosen(cx: &mut TestAppContext) {
    let tmp = Scratch::new("import");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    let (png, svg, notes) = (
        tmp.0.join("red.png"),
        tmp.0.join("logo.svg"),
        tmp.0.join("notes.txt"),
    );
    std::fs::write(&png, red_png(40, 20)).unwrap();
    std::fs::write(&svg, RECT_SVG).unwrap();
    std::fs::write(&notes, "hello").unwrap();
    run_ui(&view, "file.import", &mut cx);
    let chosen = vec![png.clone(), svg];
    cx.simulate_path_prompt_response(move |options| {
        assert!(options.files && options.multiple && !options.directories);
        Some(chosen)
    });
    cx.run_until_parked();
    let made = layers(&view, &cx);
    assert_eq!(made.len(), 2);
    assert_eq!(selection(&view, &cx), made);
    assert_eq!(toast(&view, &cx), None);
    run_ui(&view, "file.placeImage", &mut cx);
    cx.simulate_path_prompt_response(move |_| Some(vec![notes, png]));
    cx.run_until_parked();
    assert_eq!(layers(&view, &cx).len(), 3);
    let toast = toast(&view, &cx).unwrap_or_default();
    assert!(toast.starts_with("notes.txt: "), "{toast}");
}
