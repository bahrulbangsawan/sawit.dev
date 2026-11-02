//! Where the canvas file is, and how it opens: as itself, as the old canvas it takes over from, or
//! as an `.excalidraw` file imported.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};

use sawit_core::{files, nyawit};

use super::MAX_FILE;
use super::backup::{keep_copy, lossy_backup};
use crate::doc::{BoardDoc, BoardError};
use crate::excalidraw;

/// What a new canvas is called.
const NAME: &str = "Canvas";

/// The file the project canvas was in before the native canvas
/// (`<project>/.nyawit/canvas.excalidraw`, [`nyawit::legacy_canvas_file`]), which a canvas file
/// that isn't there yet is imported from. It is found beside the canvas file by its name, because
/// the path of the canvas file is all the store is given.
const OLD_CANVAS: &str = "canvas.excalidraw";

/// The canvas file: `<project>/.nyawit/canvas.sawit`, else `canvas.sawit` in `app_root`
/// (Sawit's own folder).
pub fn board_file(project: Option<&Path>, app_root: &Path) -> PathBuf {
    match project {
        Some(project) => nyawit::board_file(project),
        None => app_root.join(nyawit::BOARD_FILE),
    }
}

/// Opens the canvas at `path`, as [`open_with_issues`] does (so it imports the old canvas too), and
/// only logs its notes.
pub fn open(path: &Path) -> Result<BoardDoc, BoardError> {
    let (board, issues) = open_with_issues(path)?;
    for issue in issues {
        log::warn!(
            "canvas file {} loaded with an issue: {issue}",
            path.display()
        );
    }
    Ok(board)
}

/// Opens the canvas at `path`, with the notes on what saving it back would lose
/// ([`BoardDoc::from_bytes_with_issues`]).
///
/// - A missing file is a new, empty board. Nothing is written, not even the folder: the first
///   save does that.
/// - A missing canvas file (`canvas.sawit`, [`nyawit::BOARD_FILE`]) with a `canvas.excalidraw`
///   beside it is that old canvas, imported ([`excalidraw::import`]). It is the first open only:
///   once the canvas has been saved the file is there, and is what opens. Neither file is
///   written, and no `.bak` is made: the old canvas stays as it is, and the notes are what the
///   import left out. An old canvas with nothing in it (empty, or only white space) is a blank
///   board, as it was on the old canvas, and has nothing to say. Any other old canvas that can't be
///   imported is an error that names it, and then nothing is made either; no new board takes its
///   place, because the first save would hide it.
/// - A corrupt or truncated file is `Err(BoardError::Format)`, and stays as it is.
/// - Only a regular file is read, of at most [`MAX_FILE`] bytes ([`files::read_regular`]): a
///   named pipe, or a link a cloned repository holds to a device such as `/dev/zero`, is refused
///   at once, where its read blocked the UI thread for ever, or until the Mac ran out of memory
///   (CANVAS-CORE-6). The error, `Err(BoardError::Io)`, says so.
/// - A file opens with its version history and branches, which a save writes back
///   ([`BoardDoc::to_bytes`]). Images past the loader's limits stay in the file, unread, so a save
///   over it is refused ([`BoardDoc::refuse_dropping_images`]); one elsewhere isn't.
/// - With notes, the original is first copied to `<path>.lossy.bak`, so the first save can't
///   silently drop what the loader left out. When that copy can't be made the file is not opened
///   either: it stays as it is rather than get saved over unprotected. The copy has a name of its
///   own: the backups that an emptying save keeps ([`Writer::save_with`]) move along, and go.
pub fn open_with_issues(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let bytes = match files::read_regular(path, MAX_FILE) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return open_missing(path),
        Err(e) => {
            return Err(BoardError::Io(format!(
                "couldn't read {}: {e}",
                path.display()
            )));
        }
    };
    let (board, issues) = BoardDoc::from_file_bytes(&bytes, path)?;
    if !issues.is_empty() {
        let bak = lossy_backup(path);
        keep_copy(&bak, &bytes, path).map_err(|e| {
            BoardError::Io(format!(
                "couldn't keep a copy of {} at {} before opening it: {e}",
                path.display(),
                bak.display()
            ))
        })?;
    }
    Ok((board, issues))
}

/// What the canvas file `path`, which isn't there, opens as: the old canvas beside it imported,
/// else a new board.
fn open_missing(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    match old_canvas(path) {
        Some(old) => import_old_canvas(&old),
        None => Ok((BoardDoc::new_board(NAME)?, Vec::new())),
    }
}

/// The old canvas that the canvas file `path` replaces: `canvas.excalidraw` in the same folder.
/// Only the canvas file takes over from it, not any other `.sawit` file that is missing.
fn old_canvas(path: &Path) -> Option<PathBuf> {
    if path.file_name() != Some(OsStr::new(nyawit::BOARD_FILE)) {
        return None;
    }
    let old = path.with_file_name(OLD_CANVAS);
    old.is_file().then_some(old)
}

/// A board that holds the old canvas `old`, and the notes on what the import left out, as
/// [`import_file`] imports a file.
fn import_old_canvas(old: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    import_as(old, NAME)
}

/// The canvas file that a canvas opened on the file at `path` is bound to, and saves to (spec
/// D14): for an `.excalidraw` file the `<stem>.sawit` beside it (so the old project canvas is
/// bound to the canvas file), for any other file the file itself.
pub fn bound_file(path: &Path) -> PathBuf {
    if is_excalidraw(path) {
        path.with_extension(files::CANVAS_EXTENSION)
    } else {
        path.to_path_buf()
    }
}

/// Whether `path` names an `.excalidraw` file, in any case.
fn is_excalidraw(path: &Path) -> bool {
    (path.extension()).is_some_and(|ext| ext.eq_ignore_ascii_case("excalidraw"))
}

/// The `.excalidraw` file at `path` as a new board named after it, and the notes on what the
/// import left out ([`excalidraw::to_board`]). The board opens as a file does, with no undo step.
/// Nothing is written, and the file stays as it is.
///
/// A file with nothing in it (empty, or only white space) is a blank board with nothing to say,
/// as it was on the old canvas ([`excalidraw::read_file_or_blank`]). Any other file that can't be
/// imported is an error that names it.
pub fn import_file(path: &Path) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let name = path
        .file_stem()
        .map_or_else(|| NAME.into(), |stem| stem.to_string_lossy());
    import_as(path, &name)
}

/// The `.excalidraw` file at `path` imported as a new board called `name`, and the notes on what
/// the import left out ([`import_file`]).
fn import_as(path: &Path, name: &str) -> Result<(BoardDoc, Vec<String>), BoardError> {
    let scene = excalidraw::read_file_or_blank(path)?;
    excalidraw::to_board(name, &scene)
        .map_err(|e| BoardError::Format(format!("{} can't be imported: {e}", path.display())))
}

/// Opens the canvas a canvas tab shows for the file at `path` (spec D14), as [`open`] opens one:
/// an `.excalidraw` file's scene, imported ([`import_file`]), while the `.sawit` file it is
/// bound to ([`bound_file`]) isn't there, and that file once it is; any other file itself. What
/// the import left out is only logged. Nothing is written, the `.excalidraw` file least of all:
/// the first save of the canvas makes the `.sawit` file, so the scene is imported once.
pub fn open_bound(path: &Path) -> Result<BoardDoc, BoardError> {
    let file = bound_file(path);
    if !is_excalidraw(path) || file.exists() {
        return open(&file);
    }
    let (board, issues) = import_file(path)?;
    for issue in issues {
        log::warn!("{} imported with an issue: {issue}", path.display());
    }
    Ok(board)
}
