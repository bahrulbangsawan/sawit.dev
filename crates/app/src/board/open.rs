//! Opening a canvas: its file, or an Excalidraw file imported into the `.sawit` file beside it,
//! with what opening it left out; the system's fonts, read once the first canvas opens; and the
//! file followed when it is renamed or moved.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::StreamExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::store::{self, Backup, Stamp, Writer};
use sawit_board::{BoardDoc, BoardError, DocKind};

use super::toast::hide_toast_later;
use super::worker::Worker;
use super::{BoardView, design_panel, layers_panel, menus, presence, screens, web};

/// The system's fonts are being read, or were ([`read_system_fonts`]).
pub(super) struct SystemFonts;
impl Global for SystemFonts {}

/// Reads the system's fonts into the canvas's font database on a background thread, once: when
/// the first canvas opens, since only the canvas uses them. Reading them at startup left AppKit's
/// 160 MB symbol catalog resident for the first half minute of every launch, with or without a
/// canvas. Then every window repaints: text laid out meanwhile in the default face is laid out
/// again in its own ([`figcraft_text::layout_generation`] has changed).
fn read_system_fonts(cx: &mut App) {
    if cx.has_global::<SystemFonts>() {
        return;
    }
    cx.set_global(SystemFonts);
    let scan = cx.background_spawn(async { figcraft_text::scan_system_fonts() });
    cx.spawn(async move |cx| {
        scan.await;
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

impl BoardView {
    /// Opens the canvas file at `path` ([`store::open_with_issues`]).
    ///
    /// - A missing file is a new, empty board. Nothing is written until there is an edit.
    /// - A missing project canvas file with the old canvas beside it opens as the old canvas,
    ///   imported. That isn't on disk yet, so the first autosave, or a flush, writes it, edited or
    ///   not: the old canvas is imported once.
    /// - A file that can't be read is shown as an error, and stays as it is: this view never saves.
    /// - What loading repaired or left out, or saving would drop, shows as a toast. The original
    ///   was kept as `<path>.lossy.bak` before the file opened.
    /// - The file's version history and branches are saved with it. A file that holds images the
    ///   loader left out (past what it reads from one file) is never saved over, which would delete
    ///   them: an edit says so instead, in FigCraft's words.
    /// - A file that another app or window wrote to since the canvas opened or saved it is never
    ///   saved over, which would replace what was written: an edit says so instead, in FigCraft's
    ///   words ([`Writer::guard`]). A file that wasn't there yet is written by the first save, and
    ///   is guarded from then on, unless a file was made there meanwhile (another window's canvas
    ///   of it, a `git pull`), which isn't saved over either (CANVAS-CORE-5).
    pub fn open(path: PathBuf, cx: &mut Context<Self>) -> Self {
        // Stamped before it is read, as FigCraft stamps a file it opens: a change made while it is
        // read makes the next save refuse rather than replace it.
        let read = Stamp::of(&path);
        let opened = store::open_with_issues(&path)
            .map(|(board, issues)| (board, issues_line(&path, &issues)));
        Self::with_board(path, opened, read, cx)
    }

    /// Opens the Excalidraw file at `path` imported ([`store::import_file`]), as a canvas bound to
    /// the `<stem>.sawit` file beside it ([`store::bound_file`]), which it saves to (spec D14).
    /// The `.excalidraw` file is never written.
    ///
    /// - When that `.sawit` file is there, the scene was imported before, and the file is what
    ///   opens ([`BoardView::open`]).
    /// - Else the scene is drawn on a new board, and what the import left out shows as a toast
    ///   that names the `.excalidraw` file. The board isn't on disk yet, so its first autosave, or
    ///   a flush, writes the `.sawit` file, edited or not. A scene with nothing in it is a blank
    ///   board, written once there is an edit, as a new canvas is.
    /// - A file that can't be imported is shown as an error that names it, as a canvas file that
    ///   can't be read is, and nothing is written.
    pub fn import(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let file = store::bound_file(&path);
        if file.exists() {
            return Self::open(file, cx);
        }
        let imported =
            store::import_file(&path).map(|(board, issues)| (board, issues_line(&path, &issues)));
        Self::with_board(file, imported, None, cx)
    }

    /// The canvas of the file at `path`, as it was opened: its board and the toast that says what
    /// opening it left out, or why it couldn't be opened. `read` is the file's stamp from before
    /// it was read, `None` when there was no file.
    fn with_board(
        path: PathBuf,
        opened: Result<(BoardDoc, Option<String>), BoardError>,
        read: Option<Stamp>,
        cx: &mut Context<Self>,
    ) -> Self {
        read_system_fonts(cx);
        let (worker, mut frames) = Worker::spawn();
        let frames = cx.spawn(async move |this, cx| {
            while let Some(frame) = frames.next().await {
                if this
                    .update(cx, |this, cx| this.show_frame(frame, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let (board, toast) = match opened {
            Ok((mut board, toast)) => {
                // The theme, as each frame tells it (`set_ui_dark`), told without the first time's
                // recolouring: FigCraft gives the dark page to a blank new file it makes, which it
                // tells by its having no path, and a canvas keeps none, so a blank file another
                // app made took the dark page, and the first edit saved that into it
                // (CANVAS-CORE-11). Sawit makes no new design file, and a board's page isn't one
                // that is recoloured.
                board.session.ui_dark = Some(cx.theme().mode.is_dark());
                board.session.apply_view_prefs();
                (Ok(board), toast)
            }
            Err(err) => (Err(err.to_string().into()), None),
        };
        // The document the file holds. A board with layers whose file isn't there yet (the old
        // canvas migrated, an `.excalidraw` file imported) is in no file, so the first autosave or
        // flush writes it; a blank one waits for an edit.
        let on_disk = read.is_some();
        let saved = (board.as_ref().ok())
            .and_then(|board| board.document_itself().ok())
            .filter(|doc| on_disk || store::is_empty(doc));
        // Its saves never replace what another app or window wrote to the file since.
        let writer = Writer::spawn();
        writer.guard(path.clone(), read);
        // A design file opens with both panels, a board with neither (spec D11).
        let design_file = board
            .as_ref()
            .is_ok_and(|board| board.kind() == DocKind::Design);
        let web = web::WebFrames::new(cx, &path);
        Self {
            path,
            board,
            focus: cx.focus_handle(),
            worker,
            requested: None,
            composition: None,
            ime_owner: None,
            frame: None,
            stale: Vec::new(),
            fitted: false,
            bounds: Bounds::default(),
            dragging: false,
            pointer: (Point::default(), Modifiers::default()),
            space_held: false,
            pan: None,
            keyboard_watch: None,
            window: None,
            zooming_until: None,
            zoom_settle: None,
            writer: Arc::new(writer),
            saved,
            pending: None,
            next_backup: Backup::None,
            kept_unsaved: None,
            save_later: None,
            toast_timer: toast.is_some().then(|| hide_toast_later(cx)),
            toast: toast.map(SharedString::from),
            open_flyout: None,
            last_tools: Default::default(),
            picker: None,
            shown_kind: None,
            show_layers: design_file,
            expanded: HashSet::new(),
            layer_filter: String::new(),
            renaming: None,
            drag: None,
            layer_anchor: None,
            layers: layers_panel::LayersPanel::default(),
            show_design: design_file,
            design: design_panel::DesignPanel::default(),
            scrub: None,
            menus: menus::Menus::default(),
            shown: true,
            wants_keyboard: false,
            gave_keyboard: false,
            agents: presence::Presence::default(),
            web: Some(web),
            screen_ui: screens::ScreenUi::default(),
            _frames: frames,
            // ⌘Q quits without dropping the views: what isn't on disk yet is written first.
            _quit: cx.on_app_quit(|this, _| {
                this.flush_on_close();
                async {}
            }),
            _release: cx.on_release(|this, cx| this.release_images(cx)),
        }
    }

    /// The canvas's file was renamed or moved to `path`, its folder with it perhaps, as the Files
    /// panel renames and moves files: the canvas follows the file, as a file editor's tab does,
    /// and saves there from now on (CANVAS-UI-3). Before, it went on saving at the old path, which
    /// made the file again there: the board forked into two files, and the renamed one lacked
    /// every edit made since.
    ///
    /// A save handed to the writer before is written first, where it was going. When that was
    /// after the move, it made the old file again, with the newest document, and the file at
    /// `path` is an older save: the canvas then saves the document at `path` too, guarded from the
    /// file there now. Otherwise the guard against another app's changes moves with the file
    /// ([`Writer::follow`]).
    pub fn rebind(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path == self.path {
            return;
        }
        self.writer.flush();
        if Stamp::of(&self.path).is_some() {
            self.writer.guard(path.clone(), Stamp::of(&path));
            self.saved = None;
            self.pending = None;
        } else {
            self.writer.follow(&self.path, path.clone());
        }
        self.path = path;
        self.autosave(cx);
        cx.notify();
    }
}

/// The notes from opening `path` as one line for the toast, worded as FigCraft's own `file.open`
/// words them: "<file>: <note>", or how many there are and the first.
pub(super) fn issues_line(path: &Path, issues: &[String]) -> Option<String> {
    let file = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    match issues {
        [] => None,
        [one] => Some(format!("{file}: {one}")),
        more => Some(format!(
            "{file}: {} problems were repaired or left out while opening it, e.g. {}",
            more.len(),
            more[0]
        )),
    }
}
