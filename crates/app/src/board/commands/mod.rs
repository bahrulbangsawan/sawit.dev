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
use sawit_core::files::CANVAS_EXTENSION;
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
                    failed = Some(BoardError::Engine(format!("{name}: {}", error.for_user())));
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
    // A copy is a `.sawit` file.
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
/// replaces (the dialog asked about the name without it). Any other extension is refused.
fn canvas_path(mut path: PathBuf) -> Result<PathBuf, String> {
    let is = |ext: &OsStr, wanted: &str| ext.eq_ignore_ascii_case(wanted);
    match path.extension() {
        Some(ext) if is(ext, CANVAS_EXTENSION) => Ok(path),
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
mod tests;
