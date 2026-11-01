//! The commands the canvas carries out itself instead of the engine (spec, "Menu policy": the
//! UI-handled ids, `sawit_board::policy::UI_COMMANDS`), from the menus, the Actions palette and
//! the keys alike ([`run_ui`]):
//!
//! - **Import from file…** and **Place image…** ask for files, and bring each one into the middle
//!   of the view (`sawit_board::ops::import_file`).
//! - **Save** writes the canvas's file now, rather than once the canvas has been still for a
//!   moment. **Save as…** asks where, and writes a copy of the canvas there as a `.sawit` file;
//!   the canvas stays bound to its own file.
//! - **Export**, **Export as SVG** and **Export frames to PDF** write the selection, or the page,
//!   into the canvas's exports folder, and show the first file in the Finder
//!   ([`BoardView::export_selection`]).
//! - **Copy as** text, CSS, SVG and PNG put the selection on the system clipboard, or for SVG and
//!   PNG the page when nothing is selected (`sawit_board::ops::copy_as`).
//!
//! A dialog that is cancelled changes nothing and says nothing. Whatever fails says why in a
//! toast. Save, Save as and Copy as say so when they go through; an export shows its first file
//! instead, and an import selects what it brought in.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use gpui_kit::*;
use sawit_board::ops::{self, Copied};
use sawit_board::store::{self, Writer};
use sawit_board::{BoardDoc, BoardError};
use sawit_core::files::{CANVAS_EXTENSION, LEGACY_CANVAS_EXTENSION};
use sawit_core::instances::AppPaths;
use sawit_core::nyawit;

use super::BoardView;

/// Carries out command `id`, one of `sawit_board::policy::UI_COMMANDS`, as a click on its menu
/// item or its shortcut does ([`BoardView::run_menu`], [`BoardView::on_key`]). A canvas whose file
/// couldn't be opened has nothing to carry it out on, and says why already.
pub(super) fn run_ui(
    view: &mut BoardView,
    id: &'static str,
    window: &mut Window,
    cx: &mut Context<BoardView>,
) {
    if view.board().is_none() {
        return;
    }
    match id {
        "file.import" => import(false, window, cx),
        "file.placeImage" => import(true, window, cx),
        "file.save" => {
            let (Ok(said) | Err(said)) = save(view);
            view.show_toast(said, cx);
        }
        "file.saveAs" => save_as(view, window, cx),
        "file.export" => {
            let written = view.export_selection(cx);
            view.reveal_exports(written, cx);
        }
        "file.exportSvg" => {
            let written = view.export_selection_as_svg(cx);
            view.reveal_exports(written, cx);
        }
        "file.exportFramesPdf" => {
            let written = view.export_frames_pdf(cx);
            view.reveal_exports(written, cx);
        }
        "edit.copyAsText" | "edit.copyAsCss" | "edit.copyAsSvg" | "edit.copyAsPng" => {
            copy_as(view, id, cx);
        }
        // An id the policy might add before the canvas carries it out.
        _ => {
            log::debug!("the canvas doesn't carry out {id} yet");
            view.show_toast("Coming soon", cx);
        }
    }
}

/// Save: writes the canvas's own file now, as closing it does ([`BoardView::save_now`]). What to say:
/// that it is saved, or that it was written again where it had gone, or why it isn't.
fn save(view: &mut BoardView) -> Result<String, String> {
    view.save_now()
        .map(|notice| notice.unwrap_or_else(|| "Saved".to_string()))
}

/// Import from file… (`place` false) and Place image… (`place` true): asks for files, then brings
/// each one in ([`import_files`]). A dialog that is cancelled, or that couldn't open, does nothing.
fn import(place: bool, window: &mut Window, cx: &mut Context<BoardView>) {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: true,
        prompt: Some(if place { "Place" } else { "Import" }.into()),
    });
    cx.spawn_in(window, async move |this, cx| {
        let Ok(Ok(Some(paths))) = picked.await else {
            return;
        };
        let _ = this.update(cx, |view, cx| import_files(view, &paths, place, cx));
    })
    .detach();
}

/// Brings each of the files at `paths` into the middle of the view, an undo step each
/// (`sawit_board::ops::import_file`), and selects all that they made. A file that can't come in
/// says why in a toast, by its name, and the others still come in.
fn import_files(view: &mut BoardView, paths: &[PathBuf], place: bool, cx: &mut Context<BoardView>) {
    let import = |board: &mut BoardDoc| {
        let mut made = Vec::new();
        let mut failed = None;
        for path in paths {
            match ops::import_file(board, path, place) {
                Ok(layers) => made.extend(layers),
                Err(error) if failed.is_none() => {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    failed = Some(BoardError::Engine(format!("{name}: {error}")));
                }
                Err(_) => {}
            }
        }
        if made.len() > 1 {
            board.session.set_selection(made);
        }
        failed.map_or(Ok(()), Err)
    };
    view.run(import, cx);
}

/// Save as…: asks where, beginning in [`save_as_folder`] with the name of the canvas's file, then
/// writes the canvas there ([`save_copy`]). A dialog that is cancelled, or that couldn't open,
/// does nothing.
fn save_as(view: &BoardView, window: &mut Window, cx: &mut Context<BoardView>) {
    let canvas = view.path();
    let folder = save_as_folder(canvas, || AppPaths::default_macos().root, home);
    // A copy is a `.sawit` file, a canvas opened from a `.figcraft` file's too.
    let name = Path::new(canvas.file_name().unwrap_or_default()).with_extension(CANVAS_EXTENSION);
    let name = name.to_string_lossy();
    let picked = cx.prompt_for_new_path(&folder, Some(name.as_ref()));
    cx.spawn_in(window, async move |this, cx| {
        let Ok(Ok(Some(path))) = picked.await else {
            return;
        };
        let _ = this.update(cx, |view, cx| save_copy(view, path, cx));
    })
    .detach();
}

/// The folder Save as… begins in for the canvas file at `canvas`: the project's for a project's
/// canvas (in its `.nyawit` folder), the home folder for the canvas in Sawit's own folder
/// (`app_root`), else the canvas file's own folder.
fn save_as_folder(
    canvas: &Path,
    app_root: impl FnOnce() -> PathBuf,
    home: impl FnOnce() -> PathBuf,
) -> PathBuf {
    let Some(folder) = canvas.parent() else {
        return home();
    };
    if folder.file_name() == Some(OsStr::new(nyawit::DIR))
        && let Some(project) = folder.parent()
    {
        return project.to_path_buf();
    }
    if folder == app_root() {
        home()
    } else {
        folder.to_path_buf()
    }
}

/// The user's home folder.
fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

/// Writes the canvas as a `.sawit` file at `path`, chosen in Save as…'s dialog
/// ([`canvas_path`]), and says so in a toast, or why it couldn't. The canvas's own file is saved
/// as Save saves it; any other gets a copy, which the canvas doesn't follow: it goes on saving to
/// its own file.
fn save_copy(view: &mut BoardView, path: PathBuf, cx: &mut Context<BoardView>) {
    let saved = canvas_path(path).and_then(|path| {
        if same_file(&path, view.path()) {
            return save(view);
        }
        write_copy(view, &path)?;
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        Ok(format!("Saved a copy as {name}"))
    });
    let (Ok(said) | Err(said)) = saved;
    view.show_toast(said, cx);
}

/// `path` as the file Save as… writes, as FigCraft's own Save as takes it: one without an
/// extension gets `.sawit`, unless a file of that name is there already, which only its full name
/// replaces (the dialog asked about the name without it). A `.figcraft` name, the extension canvas
/// files had before (the name of a file opened from before, say), is written as it is named. Any
/// other extension is refused.
fn canvas_path(mut path: PathBuf) -> Result<PathBuf, String> {
    let is = |ext: &OsStr, wanted: &str| ext.eq_ignore_ascii_case(wanted);
    match path.extension() {
        Some(ext) if is(ext, CANVAS_EXTENSION) || is(ext, LEGACY_CANVAS_EXTENSION) => Ok(path),
        Some(ext) if !ext.is_empty() => Err(format!("Use the .{CANVAS_EXTENSION} extension.")),
        _ => {
            path.set_extension(CANVAS_EXTENSION);
            if path.exists() {
                return Err(format!(
                    "A .{CANVAS_EXTENSION} file with this name already exists. Choose its full \
                     name to confirm replacement."
                ));
            }
            Ok(path)
        }
    }
}

/// Whether `a` and `b` name one file: the same path, or one file under two names (a link, or its
/// name in other capitals on a disk that ignores them).
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let id = |path: &Path| (std::fs::metadata(path).ok()).map(|meta| (meta.dev(), meta.ino()));
    a == b || id(a).is_some_and(|first| id(b) == Some(first))
}

/// Writes the canvas's document to `path`, as its own file is written
/// (`sawit_board::store::Writer`): whole or not at all, and after a `.bak` of a file there that has
/// layers when the canvas has none. The writer is the copy's own, so its error is this write's,
/// and the canvas's own saves go on as they were.
fn write_copy(view: &BoardView, path: &Path) -> Result<(), String> {
    let board = view.board().ok_or("the canvas file couldn't be opened")?;
    let bytes = board.to_bytes().map_err(|error| error.to_string())?;
    let doc = board.document_itself().map_err(|error| error.to_string())?;
    let empty = store::is_empty(&doc);
    let writer = Writer::spawn();
    writer.save(path.to_path_buf(), bytes, empty);
    writer.flush();
    writer.last_error().map_or(Ok(()), Err)
}

/// Copy as text, CSS, SVG or PNG (`id`): puts what `sawit_board::ops::copy_as` copies on the
/// system clipboard, and says so, as FigCraft's own app does. With nothing to copy, the clipboard
/// keeps what it holds, and the toast says why.
fn copy_as(view: &mut BoardView, id: &str, cx: &mut Context<BoardView>) {
    let Some(copied) = view.run(|board| ops::copy_as(board, id), cx) else {
        return;
    };
    let (item, what) = match copied {
        Copied::Text(text) => {
            let what = match id {
                "edit.copyAsCss" => "CSS",
                "edit.copyAsSvg" => "SVG",
                _ => "text",
            };
            (ClipboardItem::new_string(text), what)
        }
        Copied::Png(png) => {
            let png = Image::from_bytes(ImageFormat::Png, png);
            (ClipboardItem::new_image(&png), "PNG")
        }
    };
    cx.write_to_clipboard(item);
    view.show_toast(format!("Copied as {what}"), cx);
}

#[cfg(test)]
mod tests {
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

    /// Review Focus 2 and ruling P6-C12: a menu command the engine refuses says why, in a toast.
    #[gpui_kit::test]
    fn disabled_command_toasts_reason(cx: &mut TestAppContext) {
        let tmp = Scratch::new("disabled-command");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.run_menu("edit.pasteProperties", window, cx)
            })
        });
        let toast = toast(&view, &cx).unwrap_or_default();
        assert!(toast.contains("no copied properties"), "{toast}");
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
        assert_eq!(toast(&view, &cx).as_deref(), Some("nothing is selected"));
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
    /// is given `.sawit`, one with `.figcraft` (canvas files' extension before) is written as it
    /// is named, and another extension is refused.
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
        let before = tmp.0.join("copy.figcraft");
        assert_eq!(
            save_as(&view, before.clone(), &mut cx).as_deref(),
            Some("Saved a copy as copy.figcraft")
        );
        assert_eq!(saved_layers(&before), 1);
        let png = tmp.0.join("copy.png");
        let refused = save_as(&view, png.clone(), &mut cx).unwrap_or_default();
        assert!(refused.contains(".sawit"), "{refused}");
        assert!(!png.exists());
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
}
