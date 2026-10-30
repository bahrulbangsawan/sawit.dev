//! The native canvas tab: a FigCraft document, rendered off the UI thread and painted by GPUI.
//! Only the GPUI side lives here; the document logic is `sawit_board` (`crates/board`).
//!
//! [`BoardView`] shows one canvas file. Each frame it asks the render thread ([`worker`]) for the
//! view when that changed, and paints the newest frame it has, moved and scaled to the view, so
//! pans and zooms show at once and the render catches up ([`paint`]). Over it go the selection,
//! handles, names and previews `sawit_board::overlay` lists, as the view is now
//! ([`overlay_paint`]). Edits are saved once the canvas has been still for a moment, and when the
//! tab closes or the app quits.
//!
//! The pointer drives FigCraft's tools ([`events`]). Keys go where FigCraft's keymap sends them,
//! and the ones Sawit uses too are shared by the spec's conflict policy ([`bindings`],
//! [`BoardView::on_key`]). Copy and paste reach the other canvas tabs and the system clipboard
//! ([`BoardView::run_clipboard`]). While a text is edited, what the keys type and what an input
//! method composes go into it ([`text_input`]).
//!
//! Around the canvas goes its chrome (spec D11): the top bar over it ([`top_bar`]), the toolbar
//! floating at its bottom centre ([`toolbar`]), the context bar floating over a board's
//! selection ([`context_bar`]) and the Design panel on its right ([`design_panel`]). Their buttons
//! run their commands through [`BoardView::choose`]. The menus (spec D8: the top bar's main menu,
//! the context menu, the Actions palette and the shortcuts sheet) run theirs through
//! [`BoardView::run_menu`] ([`menus`]). While the UI is hidden (`⌘\`, `view.toggleUi`) only the
//! canvas shows, and the context menu, the palette and the sheet when they are open.

mod context_bar;
mod design_panel;
mod events;
mod menus;
mod overlay_paint;
mod paint;
mod text_input;
mod toolbar;
mod top_bar;
mod worker;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::Document;
use figcraft_doc::color::Color;
use figcraft_engine::textedit::ImeEditOwner;
use figcraft_engine::{UiRequest, ViewState};
use figcraft_render::{Options, View};
use futures::StreamExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::clipboard::{self, PasteSource};
use sawit_board::input::KeyOutcome;
use sawit_board::overlay::{self, Composition};
use sawit_board::store::{self, Stamp, Writer};
use sawit_board::toolbar::Flyout;
use sawit_board::view::{self, PREVIEW_QUALITY, Viewport};
use sawit_board::{BoardDoc, BoardError, DocKind, input, ops, policy};
use serde_json::{Value, json};

use crate::canvas::ToggleCanvasMaximize;
use crate::states::Placeholder;
use paint::Painting;
use toolbar::Picker;
use worker::{Frame, Job, Worker};

/// How long the canvas must be still after an edit before the edit is saved.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// How long after the last step of a zoom gesture frames are rendered at full quality again.
const ZOOM_SETTLE: Duration = Duration::from_millis(150);
/// How long a toast stays up.
const TOAST_TIME: Duration = Duration::from_secs(3);
/// How far the toast stays off the canvas's bottom edge.
const TOAST_BOTTOM: f32 = 16.;
/// How far the toast stays off the toolbar while that shows (ruling P3-C10).
const TOAST_OVER_TOOLBAR: f32 = 8.;
/// How far a mouse wheel's scroll of one line pans the canvas.
const SCROLL_LINE: Pixels = px(20.);

/// Starts what every canvas shares, once at startup: its key bindings ([`bindings`]), bound after
/// the workspace's, and the system fonts, read on a background thread. Then every window
/// repaints: text laid out with FigCraft's bundled fonts meanwhile is laid out again
/// ([`figcraft_text::layout_generation`] has changed).
pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
    let scan = cx.background_spawn(async { figcraft_text::scan_system_fonts() });
    cx.spawn(async move |cx| {
        scan.await;
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
}

/// The keys the canvas takes from the app while it has the keyboard (spec, "Keyboard conflict
/// policy"). Key bindings run before a view sees the key, so each is bound to `NoAction` in the
/// canvas's key context, and goes on to [`BoardView::on_key`]:
///
/// - `Board`, the canvas: ⌘0 and ⌘K (FigCraft's zoom to 100% and Actions), and the keys of
///   gpui-kit's root that FigCraft uses.
/// - `BoardText`, while the canvas edits text: ⌘B, ⌥⌘T and ⇧⌘↩ (bold, centre and the next
///   sticky), which are Sawit's otherwise.
///
/// Bound after the workspace's keys, or its ⌘K chords would still keep ⌘K waiting.
pub fn bindings() -> Vec<KeyBinding> {
    let board = Some("Board");
    let text = Some("BoardText");
    vec![
        KeyBinding::new("cmd-0", NoAction, board),
        KeyBinding::new("cmd-k", NoAction, board),
        // The workspace's ⌘K chords, which would hold ⌘K until their second key or a timeout.
        KeyBinding::new("cmd-k e", NoAction, board),
        KeyBinding::new("cmd-k t", NoAction, board),
        KeyBinding::new("cmd-k u", NoAction, board),
        KeyBinding::new("cmd-k w", NoAction, board),
        KeyBinding::new("cmd-k shift-enter", NoAction, board),
        // Root's Tab (root::Tab, the next focus): FigCraft's select.next.
        KeyBinding::new("tab", NoAction, board),
        // Root's ⇧Tab (root::TabPrev, the previous focus): FigCraft's select.previous.
        KeyBinding::new("shift-tab", NoAction, board),
        // Root's ⌘C (input::Copy): FigCraft's edit.copy.
        KeyBinding::new("cmd-c", NoAction, board),
        KeyBinding::new("cmd-b", NoAction, text),
        KeyBinding::new("alt-cmd-t", NoAction, text),
        KeyBinding::new("shift-cmd-enter", NoAction, text),
    ]
}

/// A canvas tab: the board in one `.figcraft` file, drawn as its view and its document change,
/// and saved as it is edited. An `.excalidraw` file opens imported, in the `.figcraft` file beside
/// it ([`BoardView::import`]).
pub struct BoardView {
    /// The `.figcraft` file the canvas is bound to: the one it opened, or the one an imported
    /// `.excalidraw` file is saved to.
    path: PathBuf,
    /// The document, or why the file couldn't be opened: the tab then says so, and never saves.
    board: Result<BoardDoc, SharedString>,
    focus: FocusHandle,
    worker: Worker,
    /// The newest render request, and its document. The document is kept so that no other one
    /// can take its address, which the request's key holds.
    requested: Option<(u64, Arc<Document>)>,
    /// What an input method composes in the text being edited, kept while it lasts: the canvas
    /// renders its document in place of the document, and lays out the caret in it
    /// ([`Composition::refresh`]). Kept, its render is asked for once.
    composition: Option<Composition>,
    /// The text edit the input method composed in when it last marked text: its unmark commits
    /// the composition only while that edit still has it (ruling P2-C9, [`text_input`]).
    ime_owner: Option<ImeEditOwner>,
    /// The newest frame, painted until a newer one arrives.
    frame: Option<Frame>,
    /// The images of the frames replaced since the last paint, which takes them out of the
    /// window's sprite atlas.
    stale: Vec<Arc<RenderImage>>,
    /// The view has been fitted to the content: on the first frame with a size.
    fitted: bool,
    /// Where the canvas was in the window when it was last painted.
    bounds: Bounds<Pixels>,
    /// The left button went down on the canvas and hasn't come up: its moves and its release go
    /// to the tools wherever the pointer is ([`events`]).
    dragging: bool,
    /// Space is held down on the canvas, and no text is edited: a press of the left button pans
    /// instead of reaching the tools ([`events`]).
    space_held: bool,
    /// The canvas is being panned by hand, with Space and the left button or with the middle
    /// button ([`events`]).
    pan: Option<events::Pan>,
    /// Watches for the canvas losing the keyboard, whereupon Space's release goes elsewhere. Set
    /// up with the first render, which has the window.
    keyboard_watch: Option<Subscription>,
    /// The window it was last painted in, whose sprite atlas holds its images.
    window: Option<AnyWindowHandle>,
    /// A zoom gesture is on until then: frames render at [`PREVIEW_QUALITY`] to keep up.
    zooming_until: Option<Instant>,
    /// Ends the zoom gesture [`ZOOM_SETTLE`] after its last step, for a full-quality render.
    zoom_settle: Option<Task<()>>,
    /// This canvas's own writer: the error it keeps is of its newest save, so it's read right
    /// after each save it waits for.
    writer: Arc<Writer>,
    /// The document the file holds: as loaded, or as last saved. `None` when the file holds
    /// something else (the last save failed or was refused), or nothing yet of a board with layers
    /// (the old canvas migrated, an `.excalidraw` file imported).
    saved: Option<Arc<Document>>,
    /// The document the autosave waits to save, or saved last.
    pending: Option<Arc<Document>>,
    /// Saves the document once the canvas has been still for [`SAVE_DELAY`].
    save_later: Option<Task<()>>,
    /// A message at the bottom of the canvas (a failed save, what opening the file dropped), up
    /// for [`TOAST_TIME`].
    pub(crate) toast: Option<SharedString>,
    /// Takes the toast down.
    toast_timer: Option<Task<()>>,
    /// The toolbar's flyout that is open, if any ([`toolbar`]).
    open_flyout: Option<Flyout>,
    /// The toolbar's Templates & table picker, while it is open ([`toolbar`]).
    picker: Option<Picker>,
    /// The kind of file the chrome was last drawn for: when another kind is on show, an open
    /// flyout or picker closes ([`Self::settle_chrome`]).
    shown_kind: Option<DocKind>,
    /// The Design panel shows beside the canvas: at first beside a design file, not beside a
    /// board (spec D11). `view.design` (⌥8, the top bar's toggle) turns it over. Hiding the UI
    /// leaves it as it is, and the panel comes back with the UI (ruling P5-C18).
    show_design: bool,
    /// What the Design panel keeps between frames ([`design_panel`]).
    design: design_panel::DesignPanel,
    /// The scrub of a Design panel field that is on ([`BoardView::scrub_begin`]).
    scrub: Option<design_panel::Scrub>,
    /// The context menu, the Actions palette and the keyboard-shortcuts sheet: which is open, and
    /// what each keeps between frames ([`menus`]).
    menus: menus::Menus,
    _frames: Task<()>,
    _quit: Subscription,
    _release: Subscription,
}

impl EventEmitter<ToggleCanvasMaximize> for BoardView {}

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
    ///   is guarded from then on.
    pub fn open(path: PathBuf, cx: &mut Context<Self>) -> Self {
        // Stamped before it is read, as FigCraft stamps a file it opens: a change made while it is
        // read makes the next save refuse rather than replace it.
        let read = Stamp::of(&path);
        let opened = store::open_with_issues(&path)
            .map(|(board, issues)| (board, issues_line(&path, &issues)));
        Self::with_board(path, opened, read, cx)
    }

    /// Opens the Excalidraw file at `path` imported ([`store::import_file`]), as a canvas bound to
    /// the `<stem>.figcraft` file beside it ([`store::bound_file`]), which it saves to (spec D14).
    /// The `.excalidraw` file is never written.
    ///
    /// - When that `.figcraft` file is there, the scene was imported before, and the file is what
    ///   opens ([`BoardView::open`]).
    /// - Else the scene is drawn on a new board, and what the import left out shows as a toast
    ///   that names the `.excalidraw` file. The board isn't on disk yet, so its first autosave, or
    ///   a flush, writes the `.figcraft` file, edited or not. A scene with nothing in it is a blank
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
                // As each frame does. Done before the document counts as saved: the blank page
                // of a file nothing was done to takes the dark theme's colour (as FigCraft's new
                // files do), which is no edit to save.
                board.session.set_ui_dark(cx.theme().mode.is_dark());
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
        let show_design = board
            .as_ref()
            .is_ok_and(|board| board.kind() == DocKind::Design);
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
            space_held: false,
            pan: None,
            keyboard_watch: None,
            window: None,
            zooming_until: None,
            zoom_settle: None,
            writer: Arc::new(writer),
            saved,
            pending: None,
            save_later: None,
            toast_timer: toast.is_some().then(|| hide_toast_later(cx)),
            toast: toast.map(SharedString::from),
            open_flyout: None,
            picker: None,
            shown_kind: None,
            show_design,
            design: design_panel::DesignPanel::default(),
            scrub: None,
            menus: menus::Menus::default(),
            _frames: frames,
            // ⌘Q quits without dropping the views: what isn't on disk yet is written first.
            _quit: cx.on_app_quit(|this, _| {
                this.flush();
                async {}
            }),
            _release: cx.on_release(|this, cx| this.release_images(cx)),
        }
    }

    /// The `.figcraft` file the canvas is bound to, and saves to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document. `None` when the file couldn't be opened.
    pub fn board(&self) -> Option<&BoardDoc> {
        self.board.as_ref().ok()
    }

    pub fn board_mut(&mut self) -> Option<&mut BoardDoc> {
        self.board.as_mut().ok()
    }

    /// Runs a control op, `{"op": "get" | "command" | ...}` (`sawit_board::ops::run`), on the
    /// document, and answers it as the control socket replies: `{ok: true, ...}` or
    /// `{ok: false, error}`.
    ///
    /// Then the canvas carries out what the op asked of it, as after the same input in the
    /// window, redraws, and saves whatever the op changed: an op arrives with no input in the
    /// window, perhaps while it is covered and its display link has stopped, so the save can't
    /// wait for the next paint.
    ///
    /// While the Actions palette, the shortcuts sheet or the context menu is open, a `key` op of
    /// Escape closes it, as the key does in the window, and the canvas never gets it ([`menus`]).
    pub fn run_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        if let Some(reply) = self.close_menus_for_op(op, cx) {
            return reply;
        }
        let reply = match self.board_mut() {
            Some(board) => ops::run(board, op),
            None => self.not_opened(),
        };
        self.handle_ui_requests(Vec::new(), cx);
        self.autosave(cx);
        cx.notify();
        reply
    }

    /// Runs engine command `command` with `params`, as a click on the chrome does: a tool's
    /// button, a flyout's or the context bar's choice, an item of the zoom menu. It is the one way
    /// the chrome runs a command (ruling P3-C13), in the order FigCraft's own app runs one
    /// (ui-egui `app/commands.rs`): the engine runs it, the canvas carries out what the engine
    /// asked of it, then the engine's error shows as a toast (last, so that it is the toast that
    /// shows), and the canvas redraws. Any command goes through as it is given.
    pub fn choose(&mut self, command: &str, params: Value, cx: &mut Context<Self>) {
        self.run(|board| board.execute(command, &params), cx);
    }

    /// Runs `f` on the document as [`Self::choose`] runs a command, with what follows it: the
    /// engine's requests, the error's toast and a redraw. What `f` gave, or `None` when it failed
    /// or no file is open.
    fn run<T>(
        &mut self,
        f: impl FnOnce(&mut BoardDoc) -> Result<T, BoardError>,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        let done = f(self.board_mut()?);
        self.handle_ui_requests(Vec::new(), cx);
        let done = match done {
            Ok(done) => Some(done),
            Err(error) => {
                self.show_toast(error.to_string(), cx);
                None
            }
        };
        cx.notify();
        done
    }

    /// Settles the chrome before it is drawn. A flyout or the picker that is open closes while
    /// the UI is hidden, and once another kind of file is on show than the chrome was drawn for
    /// (a board became a design file, or the other way round), whose tools aren't the ones it
    /// belonged to (Review Focus 4). Whether the chrome shows: not while the UI is hidden, nor
    /// without a document.
    fn settle_chrome(&mut self) -> bool {
        let Some(board) = self.board() else {
            return false;
        };
        let kind = board.kind();
        let hidden = board.session.ds().is_ok_and(|ds| ds.view.ui_hidden);
        let other_kind = self
            .shown_kind
            .replace(kind)
            .is_some_and(|shown| shown != kind);
        if hidden || other_kind {
            self.open_flyout = None;
            self.picker = None;
        }
        !hidden
    }

    /// Puts `board` on show in place of the document, as a file opened over it does.
    #[cfg(test)]
    fn replace_doc(&mut self, board: BoardDoc) {
        self.board = Ok(board);
    }

    /// Carries out what the engine asked of the canvas: `requests`, which an input already took
    /// out of the session, then any still waiting in `session.ui_requests`. It runs after every
    /// pointer event, key and op, as FigCraft's own canvas handles its requests after each input.
    ///
    /// `sawit_board::input::handle_requests` does the ones that need no view (`board.editText`
    /// starts typing in a sticky). `panel.design` (from `view.design`: ⌥8, the top bar's toggle,
    /// an agent's command) shows or hides the Design panel. `palette.actions` (`app.actions`: ⌘K,
    /// ⌘/) and `panel.shortcuts` (`help.shortcuts`: ⌃⇧?) open or close the Actions palette and
    /// the shortcuts sheet, and `contextMenu` (`view.contextMenu`: ⇧F10) opens the context menu
    /// under the selection ([`menus`]). The layers panel's requests get their arms here with
    /// Plan 4. A message the engine left for the user shows as a toast.
    fn handle_ui_requests(&mut self, mut requests: Vec<UiRequest>, cx: &mut Context<Self>) {
        let Some(board) = self.board_mut() else {
            return;
        };
        requests.append(&mut board.session.ui_requests);
        let rest = input::handle_requests(board, &requests);
        let message = board.session.message.take();
        for request in rest {
            match request.kind.as_str() {
                "panel.design" => self.show_design = !self.show_design,
                "palette.actions" => self.toggle_palette(cx),
                "panel.shortcuts" => self.toggle_sheet(cx),
                "contextMenu" => self.open_context_menu_from_keys(cx),
                kind => log::debug!("the canvas has no handler yet for request {kind}"),
            }
        }
        if let Some(message) = message {
            self.show_toast(message, cx);
        }
    }

    /// A key pressed while the canvas has the keyboard, which no key binding took ([`bindings`]
    /// lets the canvas's own through). It goes where FigCraft's keymap sends it, under Sawit's
    /// command policy ([`input::route_key`]), and then the canvas carries out what the engine
    /// asked of it, as after every input:
    ///
    /// - A key the canvas uses stops here. The shortcut of a command Sawit carries out itself
    ///   says it is coming, for now, and a command the engine refuses says why, in a toast.
    /// - Any other key goes on: while text is edited, typing reaches the input method.
    ///
    /// Space held down without modifiers, while no text is edited, arms a pan instead
    /// ([`events`]).
    ///
    /// The canvas takes only the keys pressed while it has the keyboard itself: a key typed in a
    /// field of the Design panel or in the Actions palette, inside the canvas's view, goes on to
    /// the field, and the menus and the shortcuts sheet take their own keys. While a label's
    /// drag scrubs a field, Escape puts the value back and no key runs a command, which would end
    /// up in the scrub's undo step (ruling P5-C3).
    pub(super) fn on_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.scrubbing_label() {
            if event.keystroke.key == "escape" {
                self.scrub_end(false, cx);
            }
            cx.stop_propagation();
            return;
        }
        if !self.focus.is_focused(window) {
            return;
        }
        if self.hold_space(event, cx) {
            cx.stop_propagation();
            return;
        }
        let keystroke = &event.keystroke;
        let Some(key) = input::key_name(&keystroke.key) else {
            return;
        };
        let Some(board) = self.board_mut() else {
            return;
        };
        let routed = input::route_key(board, &key, events::mods(keystroke.modifiers));
        self.handle_ui_requests(Vec::new(), cx);
        match routed {
            Ok(KeyOutcome::Unhandled) => {}
            Ok(KeyOutcome::Handled) => cx.stop_propagation(),
            Ok(KeyOutcome::Ui(id)) => {
                cx.stop_propagation();
                self.run_ui_command(id, cx);
            }
            // After the requests' toast, so that the error's is the one that shows.
            Err(error) => {
                cx.stop_propagation();
                self.show_toast(error.to_string(), cx);
            }
        }
        cx.notify();
    }

    /// Carries out `id`, a command Sawit does itself instead of the engine: copy and paste
    /// ([`Self::run_clipboard`]), and import, export, save and copy as
    /// (`sawit_board::policy::UI_COMMANDS`), none of which is done yet, so each says it is coming.
    fn run_ui_command(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if policy::is_clipboard(id) {
            self.run_clipboard(id, None, cx);
            return;
        }
        log::debug!("the canvas doesn't carry out {id} yet");
        self.show_toast("Coming soon", cx);
    }

    /// Copies, cuts or pastes (`id` is one of `sawit_board::policy::CLIPBOARD`), with the system
    /// clipboard and the other canvas tabs (`sawit_board::clipboard`). The keyboard's handler for
    /// these ids, which the menus share with Plan 6:
    ///
    /// - Copy and cut run in the engine, then the copy becomes the app's, which any canvas tab
    ///   pastes, and the system clipboard gets its marker.
    /// - Paste pastes what was copied last: an image, SVG or text another app copied since the
    ///   canvas's copy, else the copied layers. `at` is the page point to paste at (Paste here);
    ///   `None` pastes where FigCraft does. Paste over the selection and paste to replace it take
    ///   the copied layers only, as in FigCraft.
    /// - While text is edited, they act on its characters, as in any text field: the system
    ///   clipboard gets the selected text, and a paste types its text.
    ///
    /// A paste with nothing to paste does nothing and says nothing. A command the engine refuses
    /// says why, in a toast.
    fn run_clipboard(&mut self, id: &str, at: Option<(f64, f64)>, cx: &mut Context<Self>) {
        let done = if self.editing_text() {
            self.clipboard_text(id, cx)
        } else {
            self.clipboard_layers(id, at, cx)
        };
        self.handle_ui_requests(Vec::new(), cx);
        if let Err(error) = done {
            self.show_toast(error.to_string(), cx);
        }
        cx.notify();
    }

    /// Copy and paste of the canvas's layers ([`Self::run_clipboard`]).
    fn clipboard_layers(
        &mut self,
        id: &str,
        at: Option<(f64, f64)>,
        cx: &mut Context<Self>,
    ) -> Result<(), BoardError> {
        let pasted = (id == "edit.paste").then(|| system_clipboard(cx));
        let Some(board) = self.board_mut() else {
            return Ok(());
        };
        match id {
            "edit.copy" | "edit.cut" => {
                board.execute(id, &json!({}))?;
                let marker = clipboard::after_copy(board);
                cx.write_to_clipboard(ClipboardItem::new_string(marker));
            }
            "edit.paste" => {
                let (text, image) = pasted.unwrap_or_default();
                let marker = clipboard::marker();
                clipboard::before_paste(board);
                let source =
                    clipboard::paste_source(text.as_deref(), image.as_deref(), marker.as_deref());
                // Nothing newer from another app, and no layers copied: nothing to paste.
                if source == PasteSource::Canvas && board.session.clipboard.nodes.is_empty() {
                    return Ok(());
                }
                clipboard::paste(board, source, at)?;
            }
            // Paste over the selection and paste to replace it.
            _ => {
                clipboard::before_paste(board);
                if board.session.clipboard.nodes.is_empty() {
                    return Ok(());
                }
                board.execute(id, &json!({}))?;
            }
        }
        Ok(())
    }

    /// Copy and paste of the characters of the text being edited ([`Self::run_clipboard`]).
    fn clipboard_text(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), BoardError> {
        let pasted = matches!(id, "edit.paste" | "edit.pasteOver")
            .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
            .flatten();
        let Some(board) = self.board_mut() else {
            return Ok(());
        };
        match id {
            "edit.copy" | "edit.cut" => {
                let selected = if id == "edit.cut" {
                    clipboard::cut_text(board)?
                } else {
                    clipboard::copy_text(board)?
                };
                if let Some(text) = selected {
                    cx.write_to_clipboard(ClipboardItem::new_string(text));
                }
            }
            "edit.paste" | "edit.pasteOver" => {
                // Text, SVG among it; the canvas's marker stands for layers, which no text takes.
                let marker = clipboard::marker();
                match clipboard::paste_source(pasted.as_deref(), None, marker.as_deref()) {
                    PasteSource::Text(text) | PasteSource::Svg(text) => {
                        clipboard::paste_text(board, &text)?;
                    }
                    PasteSource::Canvas | PasteSource::Image(_) => {}
                }
            }
            // Paste to replace replaces layers: nothing while text is edited.
            _ => {}
        }
        Ok(())
    }

    /// Whether the canvas is editing a text: typing goes into it.
    fn editing_text(&self) -> bool {
        self.board()
            .is_some_and(|board| board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()))
    }

    /// The answer to an op when the file couldn't be opened: why, as `{ok: false, error}`.
    fn not_opened(&self) -> Value {
        let reason = self.board.as_ref().err().map_or("", |error| error.as_ref());
        json!({
            "ok": false,
            "error": format!(
                "The canvas file {} couldn't be opened: {reason}",
                self.path().display()
            ),
        })
    }

    /// Saves the document now if it isn't the one in the file, and waits until it is on disk.
    /// When that fails, or the save is refused (it would delete images the file holds, or replace
    /// what another app or window wrote to it since), the error is logged, and the next flush or
    /// edit tries again; the writer's [`Writer::last_error`] still says why a write failed.
    pub fn flush(&mut self) {
        if let Err(err) = self.queue_save() {
            log::warn!("the canvas couldn't be saved: {err}");
        }
        self.writer.flush();
        if let Some(err) = self.writer.last_error() {
            log::warn!("{err}");
            self.saved = None;
        }
    }

    /// Hands the document to the writer if it isn't the one saved last. The document handed
    /// over, if any. It is the document itself: a version the engine shows in its place is no
    /// edit, and is never saved.
    ///
    /// A save over a file that holds images the loader left out (past what it reads from one
    /// file) would delete them, so it is refused, as FigCraft's own save refuses it: the refusal
    /// is the error, and the file stays as it is.
    fn queue_save(&mut self) -> Result<Option<Arc<Document>>, BoardError> {
        let Ok(board) = &self.board else {
            return Ok(None);
        };
        let doc = board.document_itself()?;
        if same(&self.saved, &doc) {
            return Ok(None);
        }
        board.refuse_dropping_images(&self.path)?;
        let bytes = board.to_bytes()?;
        self.writer
            .save(self.path.clone(), bytes, store::is_empty(&doc));
        self.saved = Some(doc.clone());
        self.pending = Some(doc.clone());
        Ok(Some(doc))
    }

    /// After a frame: when the document isn't the one saved, saves it once the canvas has been
    /// still for [`SAVE_DELAY`]. Every new document (each edit makes one) starts the wait over,
    /// so a drag is saved once it ends. A failed or refused save shows its error as a toast: the
    /// queue's refusal (images the file holds) or the writer's (the file changed on disk).
    fn autosave(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.board().and_then(|board| board.document_itself().ok()) else {
            return;
        };
        if same(&self.saved, &doc) || same(&self.pending, &doc) {
            return;
        }
        self.pending = Some(doc);
        self.save_later = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DELAY).await;
            let queued = this.update(cx, |this, cx| match this.queue_save() {
                Ok(queued) => queued.map(|doc| (doc, this.writer.clone())),
                Err(err) => {
                    this.show_toast(err.to_string(), cx);
                    None
                }
            });
            let Ok(Some((doc, writer))) = queued else {
                return;
            };
            // The write is waited for off the UI thread.
            let error = cx
                .background_spawn(async move {
                    writer.flush();
                    writer.last_error()
                })
                .await;
            if let Some(error) = error {
                let _ = this.update(cx, |this, cx| {
                    // Not on disk: the next flush (closing the tab, quitting) tries again.
                    if same(&this.saved, &doc) {
                        this.saved = None;
                    }
                    this.show_toast(error, cx);
                });
            }
        }));
    }

    /// Shows `text` at the bottom of the canvas for [`TOAST_TIME`], in place of any toast up.
    fn show_toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toast = Some(text.into());
        self.toast_timer = Some(hide_toast_later(cx));
        cx.notify();
    }

    /// A frame from the render thread replaces the one shown, if it is of the document and page
    /// on show. One of another page (the page changed while it rendered) would never be painted:
    /// it is dropped, and the frame shown stays until the page's own comes.
    fn show_frame(&mut self, frame: Frame, cx: &mut Context<Self>) {
        // Which request the frame answers, to follow a stutter from its request to its paint.
        log::trace!("canvas frame of request {:016x}", frame.key);
        let on_show = self.board().and_then(|board| board.session.ds().ok());
        if !on_show.is_some_and(|ds| frame.shows(ds.uid, ds.page)) {
            return;
        }
        if let Some(old) = self.frame.replace(frame) {
            self.stale.push(old.image);
        }
        cx.notify();
    }

    /// The view is going: its images leave the window's sprite atlas with it.
    fn release_images(&mut self, cx: &mut App) {
        let images: Vec<_> = (self.frame.take().map(|frame| frame.image))
            .into_iter()
            .chain(self.stale.drain(..))
            .collect();
        if let Some(window) = self.window
            && !images.is_empty()
        {
            let _ = window.update(cx, |_, window, _| {
                for image in images {
                    let _ = window.drop_image(image);
                }
            });
        }
    }

    /// Each frame, before the paint: tells the engine the theme and the canvas's size, fits the
    /// view to the content the first time, asks for a render when the view or the document isn't
    /// the one asked for last, lays out the overlay and hands the engine its frames' names as they
    /// will be painted, and starts the autosave after an edit. While an input method composes,
    /// the document rendered, and the text the overlay lays out, have the composition in them. What
    /// to paint, if there is a document.
    fn prepaint(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Painting> {
        self.bounds = bounds;
        self.window = Some(window.window_handle());
        let dark = cx.theme().mode.is_dark();
        let scale_factor = f64::from(window.scale_factor());
        let now = cx.background_executor().now();
        let zooming = self.zooming_until.is_some_and(|until| until > now);
        let board = self.board.as_mut().ok()?;
        board.session.set_ui_dark(dark);
        board.session.apply_view_prefs();
        let kind = board.kind();
        Composition::refresh(&mut self.composition, &board.session);
        let ds = board.session.ds_mut().ok()?;
        let (width, height) = (f64::from(bounds.size.width), f64::from(bounds.size.height));
        // The chrome is laid out before this, from the view the last paint left: when the canvas
        // has changed size, or the view is fitted now, it is drawn again for the zoom it shows and
        // the context bar's place to catch up. A redraw asked for while painting would be lost,
        // so it is asked for once the paint is over.
        let mut moved = (ds.view.vw, ds.view.vh) != (width, height);
        ds.view.vw = width;
        ds.view.vh = height;
        let (doc, page) = (ds.doc.clone(), ds.page);
        if !self.fitted && width >= 1.0 && height >= 1.0 {
            view::zoom_to_fit(&mut ds.view, &doc, page);
            self.fitted = true;
            moved = true;
        }
        if moved {
            let this = cx.entity_id();
            cx.defer(move |cx| cx.notify(this));
        }
        // The page as the screen shows it; the render takes it as the document holds it, and
        // the worker converts its pixels the same way.
        let screen_rgb = view::screen_rgb(doc.color_profile, view::page_rgb(&doc, page));
        let viewport = Viewport {
            width,
            height,
            scale_factor,
        };
        // Pixel preview's pixels are the point of it: no softer preview of them while zooming.
        let quality = if zooming && ds.view.pixel_preview_scale().is_none() {
            PREVIEW_QUALITY
        } else {
            1.0
        };
        if let Some(wanted) = view::render_view(&ds.view, viewport, quality) {
            let job = render_job(ds, self.composition.as_ref(), kind, wanted);
            let key = job_key(&job, ds.view.zoom, dark);
            let asked = (self.requested.as_ref())
                .is_some_and(|(asked, of)| *asked == key && Arc::ptr_eq(of, &job.doc));
            if !asked {
                self.requested = Some((key, job.doc.clone()));
                self.worker.request(Job { key, ..job });
            }
        }
        // Only an image of the document and page on show: until the page's own frame comes, one
        // of the page shown before stays off screen (FigCraft's canvas: `tex_of == shown`).
        let frame = (self.frame.as_ref())
            .filter(|frame| frame.shows(ds.uid, page))
            .map(|frame| {
                let at = paint::image_bounds(bounds, &frame.view, &ds.view, scale_factor);
                (frame.image.clone(), at)
            });
        // What goes over the frame, its labels measured. The frames' names go back to the engine
        // as they are painted: it takes a press or a hover on one as on its frame.
        // On a panic, guard::run logs it and this frame paints no overlay and no names to press.
        let composition = self.composition.as_ref();
        let shapes = figcraft_engine::guard::run("ui.overlay", || {
            Ok(overlay::overlay_with(&board.session, composition))
        })
        .unwrap_or_default();
        let ds = board.session.ds().ok()?;
        let font = &cx.theme().font_family;
        let (overlay, names) = overlay_paint::layout(shapes, font, &ds.doc, &ds.view, window);
        board.session.tool.labels = names;
        let painting = Painting {
            page: screen_rgb,
            frame,
            stale: std::mem::take(&mut self.stale),
            overlay,
        };
        self.autosave(cx);
        Some(painting)
    }

    /// Pans by a scroll's delta: the content follows the fingers. With ⌘ held it zooms about the
    /// pointer instead, by e^(-dy/100) for a scroll of dy px.
    fn on_scroll(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(SCROLL_LINE);
        let (dx, dy) = (f64::from(delta.x), f64::from(delta.y));
        if event.modifiers.platform {
            self.zoom_by((-dy * 0.01).exp(), event.position, cx);
        } else if let Some(view) = self.view_mut() {
            view::pan(view, dx, dy);
            cx.notify();
        }
    }

    /// Zooms by a pinch's delta (0.1 is 10% closer) about the pointer.
    fn on_pinch(&mut self, event: &PinchEvent, cx: &mut Context<Self>) {
        self.zoom_by(1.0 + f64::from(event.delta), event.position, cx);
    }

    /// Zooms by `factor` about `position` (in the window). Frames render at preview quality until
    /// the zoom has been still for [`ZOOM_SETTLE`], then at full quality.
    fn zoom_by(&mut self, factor: f64, position: Point<Pixels>, cx: &mut Context<Self>) {
        let pointer = position - self.bounds.origin;
        let Some(view) = self.view_mut() else {
            return;
        };
        view::zoom_at(view, factor, (f64::from(pointer.x), f64::from(pointer.y)));
        self.zooming_until = Some(cx.background_executor().now() + ZOOM_SETTLE);
        self.zoom_settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ZOOM_SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.zooming_until = None;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn view_mut(&mut self) -> Option<&mut ViewState> {
        let ds = self.board_mut()?.session.ds_mut().ok()?;
        Some(&mut ds.view)
    }
}

impl Drop for BoardView {
    fn drop(&mut self) {
        // The tab is closing: what isn't on disk yet is written now.
        self.flush();
    }
}

impl Focusable for BoardView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BoardView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Err(error) = &self.board {
            return Placeholder::error("The canvas couldn't be opened.")
                .detail(error.clone())
                .into_any_element();
        }
        self.watch_keyboard(window, cx);
        let chrome = self.settle_chrome();
        // The keys the canvas takes while it edits text are bound in a context of their own.
        let key_context = if self.editing_text() {
            "Board BoardText"
        } else {
            "Board"
        };
        let top = chrome.then(|| top_bar::render_top_bar(self, window, cx));
        let canvas = self.render_canvas(chrome, window, cx);
        // Goes with the rest of the chrome while the UI is hidden, and comes back with it.
        let design = (chrome && self.show_design)
            .then(|| design_panel::render_design_panel(self, window, cx));
        // Over all of it, whether the UI is hidden or not (ruling P6-C6).
        let menus = self.render_menus(window, cx);
        div()
            .id("board")
            .key_context(key_context)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_key_up(cx.listener(Self::on_key_up))
            .children(top)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(canvas)
                    .children(design),
            )
            .children(menus)
            .into_any_element()
    }
}

impl BoardView {
    /// The canvas, under the top bar and beside the Design panel: the document as rendered and
    /// its overlay, and over them, while the `chrome` shows, the context bar and the toolbar, then
    /// the toast. The pointer on the canvas drives the tools ([`events`]); the chrome floating over
    /// it takes its own clicks, so none of them reaches the tools.
    fn render_canvas(
        &mut self,
        chrome: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let this = cx.entity().downgrade();
        let painted = this.clone();
        let cursor = self.cursor();
        let over_selection = chrome
            .then(|| context_bar::render_context_bar(self, window, cx))
            .flatten();
        let tools = chrome.then(|| toolbar::render_toolbar(self, window, cx));
        let toast = self.render_toast(chrome, cx);
        div()
            .id("board-canvas")
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .cursor(cursor)
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.on_scroll(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.on_pinch(event, cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            // A key pressed with the pointer resting on the canvas isn't the pointer leaving it.
            .hover_listener_mode(HoverListenerMode::InputModalityIndependent)
            .on_hover(cx.listener(Self::on_hover))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        this.update(cx, |this, cx| this.prepaint(bounds, window, cx))
                            .ok()
                            .flatten()
                    },
                    move |bounds, painting, window, cx| {
                        events::track_drags(&painted, window);
                        design_panel::track_scrub(&painted, window);
                        text_input::handle_input(&painted, bounds, window, cx);
                        if let Some(painting) = painting {
                            paint::paint(bounds, painting, window, cx);
                        }
                    },
                )
                .size_full(),
            )
            .children(over_selection)
            .children(tools)
            .children(toast)
    }

    /// The toast at the bottom of the canvas, or just above the toolbar while that shows (ruling
    /// P3-C10).
    fn render_toast(&self, above_toolbar: bool, cx: &App) -> Option<impl IntoElement + use<>> {
        let text = self.toast.clone()?;
        let bottom = if above_toolbar {
            toolbar::BOTTOM + toolbar::HEIGHT + TOAST_OVER_TOOLBAR
        } else {
            TOAST_BOTTOM
        };
        let theme = cx.theme();
        Some(
            div()
                .absolute()
                .bottom(px(bottom))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .debug_selector(|| "board-toast".into())
                        .max_w(px(560.))
                        .px_3()
                        .py_1p5()
                        .rounded_md()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.popover)
                        .text_color(theme.popover_foreground)
                        .text_xs()
                        .shadow_md()
                        .child(text),
                ),
        )
    }
}

/// Takes the toast down after [`TOAST_TIME`]. Dropping the task leaves it up.
fn hide_toast_later(cx: &mut Context<BoardView>) -> Task<()> {
    cx.spawn(async move |this, cx| {
        cx.background_executor().timer(TOAST_TIME).await;
        let _ = this.update(cx, |this, cx| {
            this.toast = None;
            cx.notify();
        });
    })
}

/// What the system clipboard holds for a paste: its text, and its image when FigCraft reads the
/// image's format (PNG, JPEG, WebP, GIF, BMP or TIFF). SVG copied as an image comes back as its
/// text, as which a paste takes SVG in.
fn system_clipboard(cx: &App) -> (Option<String>, Option<Vec<u8>>) {
    let Some(item) = cx.read_from_clipboard() else {
        return (None, None);
    };
    let mut text = item.text();
    let mut image = None;
    for entry in item.entries() {
        let ClipboardEntry::Image(picture) = entry else {
            continue;
        };
        match picture.format {
            ImageFormat::Svg if text.is_none() => {
                text = String::from_utf8(picture.bytes.clone()).ok();
            }
            ImageFormat::Png
            | ImageFormat::Jpeg
            | ImageFormat::Webp
            | ImageFormat::Gif
            | ImageFormat::Bmp
            | ImageFormat::Tiff
                if image.is_none() =>
            {
                image = Some(picture.bytes.clone());
            }
            _ => {}
        }
    }
    (text, image)
}

/// Whether `kept` is `doc`: the same document, not only an equal one.
fn same(kept: &Option<Arc<Document>>, doc: &Arc<Document>) -> bool {
    kept.as_ref().is_some_and(|kept| Arc::ptr_eq(kept, doc))
}

/// The render of the page on show in `ds` at `wanted` ([`view::render_view`]), drawn as the View
/// menu has it, as FigCraft's own canvas asks for its renders (ui-egui `canvas/render.rs`):
/// - Outlines ▸ Show outlines draws every layer as its outline, and Show outlines for selection
///   the layers selected; Include hidden layers and Include object bounds add to either.
/// - Soft-proof sRGB stripes the colours of a document in another profile that the screen can't
///   show.
/// - Pixel preview, zoomed in past its resolution, shows the page at 1 or 2 pixels per unit.
///
/// It is drawn on the page's colour, and a board on its dot grid. While an input method composes,
/// what is drawn is `composition`'s document, the composition in the edited text, as FigCraft's
/// canvas draws it: the text shows as it will read once committed. The key is left for
/// [`job_key`] to name.
fn render_job(
    ds: &figcraft_engine::DocState,
    composition: Option<&Composition>,
    kind: DocKind,
    wanted: View,
) -> Job {
    let doc = composition.map_or_else(|| ds.doc.clone(), |composition| composition.doc.clone());
    let page_rgb = view::page_rgb(&doc, ds.page);
    let [r, g, b] = page_rgb;
    let shown = &ds.view;
    let opts = Options {
        lod: true,
        magnified_image_pixels: true,
        background: Some(Color::from_rgba8(r, g, b, 255)),
        dots: match kind {
            DocKind::Board => view::dots(shown.zoom, view::screen_rgb(doc.color_profile, page_rgb)),
            DocKind::Design => None,
        },
        outline_mode: shown.outlines,
        outline: figcraft_render::Outline {
            hidden: shown.outline_hidden,
            bounds: shown.outline_bounds,
            // The renderer turns the lines light on a dark page itself.
            light: false,
        },
        outlined: if shown.outlines_selection {
            ds.selection.clone()
        } else {
            Vec::new()
        },
        ..Options::default()
    };
    Job {
        uid: ds.uid,
        page: ds.page,
        view: wanted,
        opts,
        page_rgb,
        proof: shown.proof_srgb && !doc.color_profile.is_srgb(),
        pixels: shown.pixel_preview_scale(),
        doc,
        key: 0,
    }
}

/// The name of render request `job` at `zoom` in the `dark` theme or not: the same for requests
/// that would draw the same frame. Another document (each edit makes one), page, view, way of
/// drawing it (the View menu's outlines, soft proof and pixel preview), set of fonts or theme
/// makes another.
fn job_key(job: &Job, zoom: f64, dark: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    Arc::as_ptr(&job.doc).hash(&mut hasher);
    (job.uid, job.page).hash(&mut hasher);
    let view = &job.view;
    for number in [view.origin.0, view.origin.1, view.scale, zoom] {
        number.to_bits().hash(&mut hasher);
    }
    (view.width, view.height).hash(&mut hasher);
    let opts = &job.opts;
    (opts.outline_mode, opts.outline.hidden, opts.outline.bounds).hash(&mut hasher);
    opts.outlined.hash(&mut hasher);
    (job.proof, job.pixels.map(f64::to_bits)).hash(&mut hasher);
    figcraft_text::layout_generation().hash(&mut hasher);
    dark.hash(&mut hasher);
    hasher.finish()
}

/// The notes from opening `path` as one line for the toast, worded as FigCraft's own `file.open`
/// words them: "<file>: <note>", or how many there are and the first.
fn issues_line(path: &Path, issues: &[String]) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::ops::Range;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use figcraft_doc::color::Color;
    use figcraft_doc::geom::Point as PagePoint;
    use figcraft_doc::{Document, NodeId};
    use figcraft_engine::board_templates::TEMPLATES;
    use figcraft_engine::keymap::{self, KEYS, Platform};
    use figcraft_engine::{ToolId, ViewState};
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{
        App, AppContext as _, AssetSource as _, Bounds, Context, Entity, EntityInputHandler as _,
        KeyBinding, KeyContext, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton,
        MouseDownEvent, MouseExitEvent, MouseUpEvent, OperatingSystem, PinchEvent, Pixels,
        PlatformInput, RenderImage, ScrollDelta, ScrollEvent, TestAppContext, VisualTestContext,
        Window, WindowHandle, point, px, size,
    };
    use sawit_board::input::gpui_keystroke;
    use sawit_board::overlay::{self, Composition, Shape};
    use sawit_board::policy::{self, SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING};
    use sawit_board::toolbar::Flyout;
    use sawit_board::{BoardDoc, DocKind, store};
    use serde_json::{Value, json};

    use super::{BoardView, SAVE_DELAY, TOAST_TIME, issues_line, menus, toolbar, top_bar};

    /// A folder of the test's own, removed when the test ends. The view's other test modules
    /// (`menus`) share this one and the helpers below.
    pub(super) struct Scratch(pub(super) PathBuf);

    impl Scratch {
        pub(super) fn new(test: &str) -> Self {
            let dir = std::env::temp_dir()
                .join(format!("sawit-board-view-{}-{test}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The canvas of `path`, opened as the workspace opens it.
    fn open(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
        // The view's render thread is a real one: it wakes the frame loop when it sends a frame
        // or ends, which the deterministic scheduler otherwise counts as a bug.
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        cx.new(|cx| BoardView::open(path.to_path_buf(), cx))
    }

    /// The names in `dir`, sorted.
    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Puts a sticky note on the board, as an edit from the canvas or an agent does.
    fn add_sticky(view: &Entity<BoardView>, cx: &mut TestAppContext) {
        view.update(cx, |view, _| {
            let board = view.board_mut().unwrap();
            let sticky = json!({"x": 0, "y": 0, "text": "hi"});
            board.execute("board.sticky", &sticky).unwrap();
        });
    }

    /// How many layers the first page of the canvas saved at `path` has.
    fn saved_layers(path: &Path) -> usize {
        let doc = store::open(path).unwrap().doc().unwrap();
        let page = doc.first_page().unwrap();
        doc.node(page).unwrap().children.len()
    }

    /// Sends `op` to the canvas as the control socket does, and returns its answer.
    pub(super) fn run_op(view: &Entity<BoardView>, op: Value, cx: &mut TestAppContext) -> Value {
        view.update(cx, |view, cx| view.run_op(&op, cx))
    }

    /// The op that puts a sticky note on the board.
    fn sticky_op() -> Value {
        json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}})
    }

    pub(super) fn view_state(view: &Entity<BoardView>, cx: &TestAppContext) -> ViewState {
        view.read_with(cx, |view, _| {
            view.board().unwrap().session.ds().unwrap().view.clone()
        })
    }

    /// The canvas of `path` in a 400 × 300 window of its own, drawn as the app draws it.
    fn open_in_window(
        cx: &mut TestAppContext,
        path: &Path,
    ) -> (WindowHandle<BoardView>, Entity<BoardView>) {
        cx.executor().allow_parking();
        let path = path.to_path_buf();
        let window = cx.open_window(size(px(400.), px(300.)), |_, cx| BoardView::open(path, cx));
        let view = window.root(cx).unwrap();
        (window, view)
    }

    /// Runs the app until the canvas has painted a frame whose image isn't `old`, and returns
    /// that image. Fails when the render thread sends none for a while.
    fn next_frame(
        cx: &mut TestAppContext,
        view: &Entity<BoardView>,
        old: Option<&Arc<RenderImage>>,
    ) -> Arc<RenderImage> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            // A frame that arrives is painted before this returns: the test app draws its dirty
            // windows as each update ends.
            cx.run_until_parked();
            let image = view.read_with(cx, |view, _| {
                view.frame.as_ref().map(|frame| frame.image.clone())
            });
            if let Some(image) = image
                && old.is_none_or(|old| !Arc::ptr_eq(old, &image))
            {
                return image;
            }
            assert!(Instant::now() < deadline, "no frame from the render thread");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Whether `image` is in the sprite atlas of `window`: painted, and not released since.
    fn in_atlas(
        cx: &mut TestAppContext,
        window: WindowHandle<BoardView>,
        image: &RenderImage,
    ) -> bool {
        cx.update_window(window.into(), |_, window, _| {
            window.has_image_atlas_entry(image)
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn missing_canvas_opens_as_empty_board(cx: &mut TestAppContext) {
        let tmp = Scratch::new("missing");
        let path = tmp.0.join(".nyawit/canvas.figcraft");
        let view = open(cx, &path);
        let kind = view.read_with(cx, |view, _| view.board().map(|board| board.kind()));
        assert_eq!(kind, Some(DocKind::Board));
        assert!(!path.exists());
        // Opening is no edit: nothing is written, not even the folder.
        view.update(cx, |view, _| view.flush());
        assert!(names(&tmp.0).is_empty(), "{:?}", names(&tmp.0));
    }

    #[gpui_kit::test]
    fn corrupt_file_shows_error_and_leaves_file(cx: &mut TestAppContext) {
        let tmp = Scratch::new("corrupt");
        let path = tmp.0.join("canvas.figcraft");
        std::fs::write(&path, b"not a canvas").unwrap();
        let view = open(cx, &path);
        assert!(view.read_with(cx, |view, _| view.board().is_none()));
        view.update(cx, |view, _| view.flush());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas");
        assert_eq!(names(&tmp.0), ["canvas.figcraft"]);
    }

    /// An old canvas as Excalidraw saved it (sawit-board's import fixture): nine layers on the
    /// page, and an embed that can't be imported.
    const FIXTURE: &str = include_str!("../../../board/tests/fixtures/excalidraw/basic.excalidraw");

    /// The `.excalidraw` file at `path`, imported as the workspace opens it.
    fn import(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
        cx.executor().allow_parking();
        cx.update(gpui_kit::init);
        cx.new(|cx| BoardView::import(path.to_path_buf(), cx))
    }

    fn path_of(view: &Entity<BoardView>, cx: &TestAppContext) -> PathBuf {
        view.read_with(cx, |view, _| view.path().to_path_buf())
    }

    #[gpui_kit::test]
    fn excalidraw_file_opens_bound_to_sibling(cx: &mut TestAppContext) {
        let tmp = Scratch::new("import");
        let old = tmp.0.join("x.excalidraw");
        std::fs::write(&old, FIXTURE).unwrap();
        let view = import(cx, &old);
        let file = tmp.0.join("x.figcraft");
        assert_eq!(path_of(&view, cx), file);
        // The scene is on the canvas. What the import left out is said of the file it is in.
        assert_eq!(layers(&view, cx).len(), 9);
        let toast = view.read_with(cx, |view, _| view.toast.clone());
        assert_eq!(toast.as_deref(), Some("x.excalidraw: skipped 1 embed"));
        // The import isn't on disk yet, so a flush writes it, with no edit, beside the scene.
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&file), 9);
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
        assert_eq!(names(&tmp.0), ["x.excalidraw", "x.figcraft"]);
    }

    #[gpui_kit::test]
    fn an_excalidraw_file_with_its_figcraft_beside_it_opens_that(cx: &mut TestAppContext) {
        let tmp = Scratch::new("import-again");
        let old = tmp.0.join("x.excalidraw");
        let file = tmp.0.join("x.figcraft");
        std::fs::write(&old, FIXTURE).unwrap();
        // Saved there before, and edited since: a sticky, not the scene's nine layers.
        let mut board = BoardDoc::new_board("x").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "edited"}))
            .unwrap();
        let bytes = write_board(&file, &board);
        let view = import(cx, &old);
        assert_eq!(path_of(&view, cx), file);
        assert_eq!(layers(&view, cx).len(), 1);
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
        // Opening it is no edit: neither file is written.
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        view.update(cx, |view, _| view.flush());
        assert!(std::fs::read(&file).unwrap() == bytes, "the file was saved");
        assert_eq!(std::fs::read(&old).unwrap(), FIXTURE.as_bytes());
    }

    #[gpui_kit::test]
    fn an_excalidraw_file_that_cannot_be_imported_shows_error_and_makes_no_figcraft(
        cx: &mut TestAppContext,
    ) {
        let tmp = Scratch::new("import-corrupt");
        let old = tmp.0.join("x.excalidraw");
        std::fs::write(&old, b"{ not json").unwrap();
        let view = import(cx, &old);
        assert!(view.read_with(cx, |view, _| view.board().is_none()));
        assert_eq!(path_of(&view, cx), tmp.0.join("x.figcraft"));
        // Ops answer why, naming the file that couldn't be imported.
        let reply = run_op(&view, json!({"op": "get"}), cx);
        assert_eq!(reply["ok"], json!(false), "{reply}");
        let error = reply["error"].as_str().expect("an error string");
        assert!(
            error.contains("x.excalidraw") && error.contains("isn't valid JSON"),
            "{error}"
        );
        // Nothing is written, and the file is as it was.
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        view.update(cx, |view, _| view.flush());
        assert_eq!(std::fs::read(&old).unwrap(), b"{ not json");
        assert_eq!(names(&tmp.0), ["x.excalidraw"]);
    }

    #[gpui_kit::test]
    fn an_excalidraw_file_with_nothing_in_it_opens_blank(cx: &mut TestAppContext) {
        let tmp = Scratch::new("import-blank");
        let old = tmp.0.join("x.excalidraw");
        std::fs::write(&old, b" \n").unwrap();
        let view = import(cx, &old);
        let kind = view.read_with(cx, |view, _| view.board().map(|board| board.kind()));
        assert_eq!(kind, Some(DocKind::Board));
        assert!(layers(&view, cx).is_empty());
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
        // A blank board is no edit, as a new canvas isn't: it is written once there is one.
        view.update(cx, |view, _| view.flush());
        assert_eq!(names(&tmp.0), ["x.excalidraw"]);
        add_sticky(&view, cx);
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&tmp.0.join("x.figcraft")), 1);
        assert_eq!(std::fs::read(&old).unwrap(), b" \n");
    }

    #[gpui_kit::test]
    fn a_migrated_canvas_is_written_once_and_then_opens_from_its_file(cx: &mut TestAppContext) {
        // A project with only its old canvas: the canvas opens it imported, and saves the import
        // with no edit, so that it is imported once (spec D5).
        let project = |test: &str| {
            let tmp = Scratch::new(test);
            let nyawit = tmp.0.join(".nyawit");
            std::fs::create_dir(&nyawit).unwrap();
            std::fs::write(nyawit.join("canvas.excalidraw"), FIXTURE).unwrap();
            (tmp, nyawit)
        };
        cx.update(gpui_kit::init);

        // Painted: its first autosave writes canvas.figcraft.
        let (_painted, nyawit) = project("migrate-painted");
        let path = nyawit.join("canvas.figcraft");
        let (_, view) = open_in_window(cx, &path);
        next_frame(cx, &view, None);
        assert!(!path.exists(), "saved before the canvas was still");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        // (A canvas file that isn't there would open as the old canvas again: the folder says
        // whether it was written.)
        assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.figcraft"]);
        assert_eq!(saved_layers(&path), 9);
        let old = std::fs::read(nyawit.join("canvas.excalidraw")).unwrap();
        assert_eq!(old, FIXTURE.as_bytes());
        // Opened again, it is read from that file, not imported again: the same layers, and the
        // import's note isn't said again.
        let again = cx.new(|cx| BoardView::open(path.clone(), cx));
        assert_eq!(layers(&again, cx).len(), 9);
        assert!(again.read_with(cx, |view, _| view.toast.is_none()));

        // Never painted (its tab closed, or the app quit, before a frame): a flush writes it.
        let (_unpainted, nyawit) = project("migrate-flush");
        let path = nyawit.join("canvas.figcraft");
        let view = cx.new(|cx| BoardView::open(path.clone(), cx));
        view.update(cx, |view, _| view.flush());
        assert_eq!(names(&nyawit), ["canvas.excalidraw", "canvas.figcraft"]);
        assert_eq!(saved_layers(&path), 9);
        let old = std::fs::read(nyawit.join("canvas.excalidraw")).unwrap();
        assert_eq!(old, FIXTURE.as_bytes());
    }

    #[gpui_kit::test]
    fn edits_are_saved_after_flush(cx: &mut TestAppContext) {
        let tmp = Scratch::new("flush");
        let path = tmp.0.join(".nyawit/canvas.figcraft");
        let view = open(cx, &path);
        add_sticky(&view, cx);
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&path), 1);
    }

    #[gpui_kit::test]
    fn edits_are_saved_once_the_canvas_has_been_still_for_the_delay(cx: &mut TestAppContext) {
        let tmp = Scratch::new("autosave");
        let path = tmp.0.join("canvas.figcraft");
        let view = open(cx, &path);
        let step = Duration::from_millis(200);
        assert!(SAVE_DELAY > step && SAVE_DELAY < 2 * step);
        // Each frame after an edit (re)starts the wait: an edit within it starts it over.
        add_sticky(&view, cx);
        view.update(cx, |view, cx| view.autosave(cx));
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        add_sticky(&view, cx);
        view.update(cx, |view, cx| view.autosave(cx));
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        assert!(!path.exists(), "saved before the canvas was still");
        cx.executor().advance_clock(SAVE_DELAY - step);
        cx.run_until_parked();
        assert_eq!(saved_layers(&path), 2);
    }

    #[gpui_kit::test]
    fn a_failed_save_shows_its_error_for_a_while(cx: &mut TestAppContext) {
        let tmp = Scratch::new("save-error");
        let blocker = tmp.0.join("not-a-folder");
        let path = blocker.join("canvas.figcraft");
        let view = open(cx, &path);
        // The folder can't be made any more: a file has its name.
        std::fs::write(&blocker, b"x").unwrap();
        add_sticky(&view, cx);
        view.update(cx, |view, cx| view.autosave(cx));
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        let toast = view.read_with(cx, |view, _| view.toast.clone());
        assert!(
            toast
                .as_ref()
                .is_some_and(|t| t.contains("canvas.figcraft")),
            "{toast:?}"
        );
        assert_eq!(std::fs::read(&blocker).unwrap(), b"x");
        // The document is kept, and a flush tries again.
        std::fs::remove_file(&blocker).unwrap();
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&path), 1);
        cx.executor().advance_clock(TOAST_TIME);
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    }

    /// A 1 × 1 PNG.
    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

    /// A canvas file with a sticky, and an image whose bytes are damaged: the loader leaves the
    /// image out, and says that a save drops it.
    fn file_with_a_damaged_image() -> Vec<u8> {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
            .unwrap();
        let placed = board
            .execute("image.place", &json!({"data": PNG, "x": 0, "y": 300}))
            .unwrap();
        let hash = placed["image"].as_str().unwrap().to_string();
        let png = board.doc().unwrap().images[&hash].bytes.clone();
        let mut bytes = board.to_bytes().unwrap();
        let at = bytes
            .windows(png.len())
            .position(|w| w == png.as_slice())
            .unwrap();
        bytes[at + png.len() / 2] ^= 0xFF;
        bytes
    }

    /// Writes `board` to `path` as its canvas file, and returns what was written.
    fn write_board(path: &Path, board: &BoardDoc) -> Vec<u8> {
        let bytes = board.to_bytes().unwrap();
        std::fs::write(path, &bytes).unwrap();
        bytes
    }

    #[gpui_kit::test]
    fn a_lossy_open_says_what_saving_drops(cx: &mut TestAppContext) {
        let tmp = Scratch::new("lossy");
        let path = tmp.0.join("canvas.figcraft");
        std::fs::write(&path, file_with_a_damaged_image()).unwrap();
        let view = open(cx, &path);
        let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
        assert!(toast.starts_with("canvas.figcraft: "), "{toast}");
        assert!(toast.contains("saving this file again drops it"), "{toast}");
        cx.executor().advance_clock(TOAST_TIME);
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    }

    #[gpui_kit::test]
    fn an_in_place_save_keeps_the_files_version_history(cx: &mut TestAppContext) {
        let tmp = Scratch::new("history");
        let path = tmp.0.join("canvas.figcraft");
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "then"}))
            .unwrap();
        board
            .execute("file.saveVersion", &json!({"name": "Then"}))
            .unwrap();
        write_board(&path, &board);
        let view = open(cx, &path);
        // The canvas keeps it, so opening has nothing to say, and keeps no copy.
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
        let edit =
            json!({"op": "command", "command": "board.sticky", "params": {"x": 300, "y": 0}});
        let reply = run_op(&view, edit, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        view.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&path), 2);
        let saved = store::open(&path).unwrap();
        let versions: Vec<String> = (saved.session.ds().unwrap().versions.iter())
            .map(|version| version.name.clone())
            .collect();
        assert_eq!(versions, ["Then"]);
        assert_eq!(names(&tmp.0), ["canvas.figcraft"]);
    }

    #[gpui_kit::test]
    fn a_version_shown_in_place_of_the_document_is_not_saved(cx: &mut TestAppContext) {
        let tmp = Scratch::new("preview");
        let path = tmp.0.join("canvas.figcraft");
        // A canvas with a sticky, and a version from when it was blank.
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("file.saveVersion", &json!({"name": "Blank"}))
            .unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "now"}))
            .unwrap();
        let bytes = write_board(&path, &board);
        let view = open(cx, &path);
        // An agent shows the blank version in its place: the canvas shows it, and that is no
        // edit. Nothing is saved, so the file keeps the document, and no backup of a canvas
        // being emptied is made either.
        let preview =
            json!({"op": "command", "command": "version.preview", "params": {"index": 0}});
        let reply = run_op(&view, preview, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(layers(&view, cx).is_empty());
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        view.update(cx, |view, _| view.flush());
        assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
        assert_eq!(names(&tmp.0), ["canvas.figcraft"]);
    }

    #[gpui_kit::test]
    fn a_file_with_images_left_in_it_is_not_saved_over(cx: &mut TestAppContext) {
        let tmp = Scratch::new("images-left");
        let path = tmp.0.join("canvas.figcraft");
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
            .unwrap();
        let bytes = write_board(&path, &board);
        let view = open(cx, &path);
        // As FigCraft's loader records a file whose images it didn't all read, past what it reads
        // from one file (sawit-board's tests open such a file, of 20 001 images): three of them.
        view.update(cx, |view, _| {
            let session = &mut view.board_mut().unwrap().session;
            let ds = session.docs.remove(0);
            let ds = ds.with_images_left_in_file(&path.to_string_lossy(), 3);
            session.docs.insert(0, ds);
        });
        // An edit isn't saved over the file, which would delete them, and the canvas says so in
        // FigCraft's words. Closing the tab doesn't save it either.
        let reply = run_op(&view, sticky_op(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
        assert!(
            toast.starts_with("canvas wasn't saved: 3 of its images weren't loaded"),
            "{toast}"
        );
        view.update(cx, |view, _| view.flush());
        assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
        assert_eq!(names(&tmp.0), ["canvas.figcraft"]);
    }

    /// A board with a sticky for each of `texts`.
    fn board_with(texts: &[&str]) -> BoardDoc {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        for (n, text) in texts.iter().enumerate() {
            let sticky = json!({"x": n as f64 * 300.0, "y": 0, "text": text});
            board.execute("board.sticky", &sticky).unwrap();
        }
        board
    }

    /// What a save over a file that another app or window wrote to since says, after the file's
    /// name: FigCraft's sentence, with Sawit's way out.
    const CHANGED_ON_DISK: &str = "wasn't saved: the file changed on disk since it was opened or \
        saved here (another app or window wrote to it), and saving would replace those changes. \
        Close its tab and open it again to load them, or save it under another name to keep this \
        version as well.";

    #[gpui_kit::test]
    fn a_file_changed_on_disk_since_it_opened_is_not_saved_over(cx: &mut TestAppContext) {
        let tmp = Scratch::new("changed-on-disk");
        let path = tmp.0.join("design.figcraft");
        write_board(&path, &board_with(&["mine"]));
        let view = open(cx, &path);
        // Another app (FigCraft, figcraft-cli, git) writes the file while its canvas is open.
        let theirs = write_board(&path, &board_with(&["theirs", "and more"]));
        let kept_theirs = || std::fs::read(&path).unwrap() == theirs;
        // An edit isn't saved over what it wrote, by a flush or the autosave, which says why.
        add_sticky(&view, cx);
        view.update(cx, |view, _| view.flush());
        assert!(kept_theirs(), "the file was saved over");
        let reply = run_op(&view, sticky_op(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
        assert_eq!(toast.to_string(), format!("design {CHANGED_ON_DISK}"));
        assert!(kept_theirs(), "the file was saved over");
        // Closing the tab doesn't save over it either.
        drop(view);
        cx.update(|_| {});
        assert!(kept_theirs(), "the file was saved over");
        assert_eq!(names(&tmp.0), ["design.figcraft"]);
    }

    #[gpui_kit::test]
    fn of_two_canvases_of_one_file_the_second_to_save_is_refused(cx: &mut TestAppContext) {
        // Two windows have the file open, and each canvas would save over the other's edits.
        let tmp = Scratch::new("two-canvases");
        let path = tmp.0.join("canvas.figcraft");
        write_board(&path, &board_with(&["shared"]));
        let first = open(cx, &path);
        let second = cx.new(|cx| BoardView::open(path.clone(), cx));
        add_sticky(&first, cx);
        first.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&path), 2);
        let saved = std::fs::read(&path).unwrap();
        // The second's edit would replace the first's: it is refused, and says so.
        let reply = run_op(&second, sticky_op(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        let toast = second.read_with(cx, |view, _| view.toast.clone()).unwrap();
        assert_eq!(toast.to_string(), format!("canvas {CHANGED_ON_DISK}"));
        second.update(cx, |view, _| view.flush());
        assert!(
            std::fs::read(&path).unwrap() == saved,
            "the first's edit was replaced"
        );
        // The first goes on saving its own edits.
        add_sticky(&first, cx);
        first.update(cx, |view, _| view.flush());
        assert_eq!(saved_layers(&path), 3);
        assert!(first.read_with(cx, |view, _| view.toast.is_none()));
    }

    #[gpui_kit::test]
    fn a_board_with_no_file_at_open_is_written_by_its_first_flush_and_guarded_after(
        cx: &mut TestAppContext,
    ) {
        // An `.excalidraw` file imported, and a project's old canvas migrated: neither `.figcraft`
        // file is there yet, so nothing else can have written to it.
        let imported = Scratch::new("guard-import");
        let old = imported.0.join("x.excalidraw");
        std::fs::write(&old, FIXTURE).unwrap();
        let migrated = Scratch::new("guard-migrate");
        let nyawit = migrated.0.join(".nyawit");
        std::fs::create_dir(&nyawit).unwrap();
        std::fs::write(nyawit.join("canvas.excalidraw"), FIXTURE).unwrap();
        let canvas = nyawit.join("canvas.figcraft");
        let views = [
            (import(cx, &old), imported.0.join("x.figcraft")),
            (cx.new(|cx| BoardView::open(canvas.clone(), cx)), canvas),
        ];
        for (view, file) in &views {
            assert!(!file.exists());
            view.update(cx, |view, _| view.flush());
            assert_eq!(saved_layers(file), 9);
            // From then on its own saves go through, and one over another app's change doesn't.
            add_sticky(view, cx);
            view.update(cx, |view, _| view.flush());
            assert_eq!(saved_layers(file), 10);
            let theirs = write_board(file, &board_with(&["theirs"]));
            add_sticky(view, cx);
            view.update(cx, |view, _| view.flush());
            assert!(
                std::fs::read(file).unwrap() == theirs,
                "{file:?} was saved over"
            );
        }
    }

    #[gpui_kit::test]
    fn frames_are_painted_and_replaced_ones_leave_the_atlas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("frames");
        cx.update(gpui_kit::init);
        let (window, view) = open_in_window(cx, &tmp.0.join("canvas.figcraft"));
        let first = next_frame(cx, &view, None);
        assert!(in_atlas(cx, window, &first));
        // The engine has the canvas's size, the window's less the top bar, which the first frame
        // fitted the view to.
        let state = view_state(&view, cx);
        let canvas_height = 300.0 - f64::from(top_bar::HEIGHT);
        assert_eq!((state.vw, state.vh), (400.0, canvas_height));
        assert!(view.read_with(cx, |view, _| view.fitted));

        // A pan asks for another frame. Painting it takes the first one out of the atlas.
        let pan = ScrollEvent {
            delta: ScrollDelta::Pixels(point(px(50.), px(0.))),
            ..ScrollEvent::default()
        };
        view.update(cx, |view, cx| view.on_scroll(&pan, cx));
        let second = next_frame(cx, &view, Some(&first));
        assert!(in_atlas(cx, window, &second));
        assert!(!in_atlas(cx, window, &first));
    }

    /// The frame image the canvas's next paint would put on screen, as its prepaint works it out
    /// at the bounds of its last paint.
    fn painted_image(
        cx: &mut TestAppContext,
        window: WindowHandle<BoardView>,
        view: &Entity<BoardView>,
    ) -> Option<Arc<RenderImage>> {
        cx.update_window(window.into(), |_, window, cx| {
            view.update(cx, |view, cx| {
                let bounds = view.bounds;
                let painting = view.prepaint(bounds, window, cx)?;
                painting.frame.map(|(image, _)| image)
            })
        })
        .unwrap()
    }

    #[gpui_kit::test]
    fn only_a_frame_of_the_page_on_show_is_painted(cx: &mut TestAppContext) {
        let tmp = Scratch::new("pages");
        let path = tmp.0.join("design.figcraft");
        // A design file of two pages: a frame on the first, nothing on the second.
        let mut session = figcraft_engine::Session::new();
        let frame =
            json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100});
        session.execute("node.create", &frame).unwrap();
        let added = session.execute("page.add", &json!({})).unwrap();
        let second = NodeId(added["id"].as_u64().unwrap());
        session
            .execute("page.select", &json!({"index": 0}))
            .unwrap();
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        cx.update(gpui_kit::init);
        let (window, view) = open_in_window(cx, &path);
        let first = next_frame(cx, &view, None);
        let (uid, first_page) = view.read_with(cx, |view, _| {
            let ds = view.board().unwrap().session.ds().unwrap();
            (ds.uid, ds.page)
        });
        assert_ne!(first_page, second);
        let painted = painted_image(cx, window, &view);
        assert!(painted.is_some_and(|image| Arc::ptr_eq(&image, &first)));

        // The second page goes on show, as an agent's `page.select` or `replace` shows one. Until
        // its own frame comes, the first page's is not painted over it: the canvas shows the
        // page's colour alone.
        let select = json!({"op": "command", "command": "page.select", "params": {"id": second.0}});
        let reply = run_op(&view, select, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let painted = painted_image(cx, window, &view);
        assert!(
            painted.is_none_or(|image| !Arc::ptr_eq(&image, &first)),
            "the first page's frame was painted over the second page"
        );

        // The second page's own frame comes, and is painted.
        let second_frame = next_frame(cx, &view, Some(&first));
        view.read_with(cx, |view, _| {
            assert!(view.frame.as_ref().unwrap().shows(uid, second));
        });
        assert!(in_atlas(cx, window, &second_frame));

        // A render of the first page that comes late (asked for before the page changed) is
        // dropped: the second page keeps its frame.
        let late = super::worker::Frame {
            view: figcraft_render::View {
                origin: (0.0, 0.0),
                scale: 1.0,
                width: 1,
                height: 1,
            },
            image: first.clone(),
            key: 0,
            uid,
            page: first_page,
        };
        view.update(cx, |view, cx| view.show_frame(late, cx));
        view.read_with(cx, |view, _| {
            let shown = &view.frame.as_ref().unwrap().image;
            assert!(Arc::ptr_eq(shown, &second_frame));
        });
    }

    #[gpui_kit::test]
    fn opening_in_a_dark_theme_is_no_edit(cx: &mut TestAppContext) {
        let tmp = Scratch::new("dark");
        let path = tmp.0.join("design.figcraft");
        // A new design file nothing was done to: FigCraft gives its blank page the dark theme's
        // page colour when it opens.
        let mut session = figcraft_engine::Session::new();
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        cx.update(|cx| {
            gpui_kit::init(cx);
            Theme::change(ThemeMode::Dark, None, cx);
        });
        let (_, view) = open_in_window(cx, &path);
        next_frame(cx, &view, None);
        let page = view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            sawit_board::view::page_rgb(&doc, doc.first_page().unwrap())
        });
        let [r, g, b, _] = Color::DARK_PAGE.to_rgba8();
        assert_eq!(page, [r, g, b]);
        // That is no edit to save: neither the frames after it nor a flush write the file.
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        view.update(cx, |view, _| view.flush());
        assert!(std::fs::read(&path).unwrap() == bytes, "the file was saved");
        assert_eq!(names(&tmp.0), ["design.figcraft"]);
    }

    #[gpui_kit::test]
    fn ops_run_on_the_canvas_document(cx: &mut TestAppContext) {
        let tmp = Scratch::new("ops");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        let layers = |cx: &mut TestAppContext| {
            let reply = run_op(&view, json!({"op": "get"}), cx);
            assert_eq!(reply["ok"], json!(true), "{reply}");
            reply["elements"].as_array().map(Vec::len)
        };
        assert_eq!(layers(cx), Some(0));
        let reply = run_op(&view, sticky_op(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(!reply["result"]["id"].is_null(), "{reply}");
        // The edit is in the canvas's own document, which `get` reads back.
        assert_eq!(layers(cx), Some(1));
        let in_document = view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.node(doc.first_page().unwrap()).unwrap().children.len()
        });
        assert_eq!(in_document, 1);
        // An op the canvas doesn't have is an error reply, not a panic.
        let reply = run_op(&view, json!({"op": "teleport"}), cx);
        assert_eq!(reply["ok"], json!(false), "{reply}");
    }

    #[gpui_kit::test]
    fn an_op_is_saved_without_a_paint(cx: &mut TestAppContext) {
        // No window: the canvas is never painted, as when an agent edits a covered window, whose
        // display link has stopped. The edit must reach the file all the same.
        let tmp = Scratch::new("op-saves");
        let path = tmp.0.join("canvas.figcraft");
        let view = open(cx, &path);
        let step = SAVE_DELAY / 2;

        // Ops that change nothing, or fail, leave nothing to save.
        run_op(&view, json!({"op": "get"}), cx);
        run_op(&view, json!({"op": "command", "command": "no.such"}), cx);
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        assert!(!path.exists(), "saved with nothing to save");

        let reply = run_op(&view, sticky_op(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        cx.executor().advance_clock(step);
        cx.run_until_parked();
        assert!(!path.exists(), "saved before the canvas was still");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        assert_eq!(saved_layers(&path), 1);

        // Each op after that is saved the same way.
        run_op(&view, sticky_op(), cx);
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        assert_eq!(saved_layers(&path), 2);
    }

    #[gpui_kit::test]
    fn an_edit_in_the_window_is_saved_once_the_painted_canvas_has_been_still(
        cx: &mut TestAppContext,
    ) {
        // An edit made in the window (a drag, a key, typing) arrives with no op: the canvas only
        // redraws, and its paint is what starts the save.
        let tmp = Scratch::new("painted-save");
        let path = tmp.0.join("canvas.figcraft");
        cx.update(gpui_kit::init);
        let (_, view) = open_in_window(cx, &path);
        next_frame(cx, &view, None);
        view.update(cx, |view, cx| {
            let sticky = json!({"x": 0, "y": 0, "text": "drawn"});
            view.board_mut()
                .unwrap()
                .execute("board.sticky", &sticky)
                .unwrap();
            cx.notify();
        });
        cx.run_until_parked();
        cx.executor().advance_clock(SAVE_DELAY / 2);
        cx.run_until_parked();
        assert!(!path.exists(), "saved before the canvas was still");
        cx.executor().advance_clock(SAVE_DELAY);
        cx.run_until_parked();
        assert_eq!(saved_layers(&path), 1);
    }

    #[gpui_kit::test]
    fn closing_the_tab_saves_what_is_not_on_disk(cx: &mut TestAppContext) {
        let tmp = Scratch::new("drop-saves");
        let path = tmp.0.join("canvas.figcraft");
        let view = open(cx, &path);
        // An edit that no paint, op or wait has saved.
        add_sticky(&view, cx);
        assert!(!path.exists());
        // The tab closes: the workspace lets go of the view, and the app drops it.
        drop(view);
        cx.update(|_| {});
        assert_eq!(saved_layers(&path), 1);
    }

    #[gpui_kit::test]
    fn quitting_saves_what_is_not_on_disk(cx: &mut TestAppContext) {
        let tmp = Scratch::new("quit-saves");
        let path = tmp.0.join("canvas.figcraft");
        let view = open(cx, &path);
        add_sticky(&view, cx);
        assert!(!path.exists());
        // ⌘Q: the app quits without dropping its views, and asks each to finish first.
        cx.quit();
        assert_eq!(saved_layers(&path), 1);
        assert!(view.read_with(cx, |view, _| view.board().is_some()));
    }

    #[gpui_kit::test]
    fn an_op_redraws_the_canvas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("op-redraw");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        let redraws = Rc::new(Cell::new(0));
        cx.update(|cx| {
            let redraws = redraws.clone();
            cx.observe(&view, move |_, _| redraws.set(redraws.get() + 1))
                .detach();
        });
        // A command that only moves the view changes nothing in the document, but the canvas
        // has to show it.
        let zoom = |cx: &TestAppContext| view_state(&view, cx).zoom;
        let before = zoom(cx);
        run_op(
            &view,
            json!({"op": "command", "command": "view.zoomIn"}),
            cx,
        );
        assert!(zoom(cx) > before);
        assert!(redraws.get() > 0, "the canvas wasn't told to redraw");
    }

    #[gpui_kit::test]
    fn a_canvas_that_could_not_be_opened_answers_ops_with_the_reason(cx: &mut TestAppContext) {
        let tmp = Scratch::new("op-corrupt");
        let path = tmp.0.join("canvas.figcraft");
        std::fs::write(&path, b"not a canvas").unwrap();
        let view = open(cx, &path);
        let reason = store::open(&path)
            .err()
            .expect("a corrupt file")
            .to_string();
        for op in [json!({"op": "get"}), sticky_op()] {
            let reply = run_op(&view, op, cx);
            assert_eq!(reply["ok"], json!(false), "{reply}");
            // It says which file, and what is wrong with it.
            let error = reply["error"].as_str().expect("an error string");
            assert!(error.contains("couldn't be opened"), "{error}");
            assert!(error.contains(&*path.to_string_lossy()), "{error}");
            assert!(error.contains(&reason), "{error}");
        }
        // And the file it couldn't open is as it was.
        cx.executor().advance_clock(SAVE_DELAY * 2);
        cx.run_until_parked();
        view.update(cx, |view, _| view.flush());
        assert_eq!(std::fs::read(&path).unwrap(), b"not a canvas");
        assert_eq!(names(&tmp.0), ["canvas.figcraft"]);
    }

    #[gpui_kit::test]
    fn painted_frame_names_go_back_to_the_engine(cx: &mut TestAppContext) {
        let tmp = Scratch::new("names");
        let path = tmp.0.join("design.figcraft");
        let mut session = figcraft_engine::Session::new();
        let frame =
            json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 100});
        let made = session.execute("node.create", &frame).unwrap();
        let frame = NodeId(made["id"].as_u64().unwrap());
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        cx.update(gpui_kit::init);
        let (_, view) = open_in_window(cx, &path);
        // Selected, it has its outline, handles and size painted over it too.
        view.update(cx, |view, cx| {
            let board = view.board_mut().unwrap();
            board.session.set_selection(vec![frame]);
            cx.notify();
        });
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let board = view.board().unwrap();
            let labels = &board.session.tool.labels;
            let named: Vec<NodeId> = labels.iter().map(|(id, _)| *id).collect();
            assert_eq!(named, [frame]);
            // Its name is just above it on the page, where a press is one on the frame.
            let quad = labels[0].1;
            assert!(quad.iter().all(|p| p.y < 0.0), "{quad:?}");
            let centre = PagePoint::new(
                quad.iter().map(|p| p.x).sum::<f64>() / 4.0,
                quad.iter().map(|p| p.y).sum::<f64>() / 4.0,
            );
            let doc = board.doc().unwrap();
            let page = doc.first_page().unwrap();
            let pressed = figcraft_engine::tools::label_at(&doc, page, labels, centre);
            assert_eq!(pressed, Some(frame));
        });
    }

    #[gpui_kit::test]
    fn long_names_are_cut_to_their_frames(cx: &mut TestAppContext) {
        let tmp = Scratch::new("long-names");
        let path = tmp.0.join("design.figcraft");
        // A frame and a section, each named far wider than it is on screen.
        let long = "Checkout – payment method – error state, ".repeat(8);
        let mut session = figcraft_engine::Session::new();
        let mut make = |layer: serde_json::Value| {
            let made = session.execute("node.create", &layer).unwrap();
            NodeId(made["id"].as_u64().unwrap())
        };
        let frame = make(json!({
            "type": "frame", "name": long, "x": 0, "y": 0, "width": 100, "height": 60,
        }));
        let section = make(json!({
            "type": "section", "name": long, "x": 0, "y": 100, "width": 200, "height": 100,
        }));
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        cx.update(gpui_kit::init);
        let (_, view) = open_in_window(cx, &path);
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let board = view.board().unwrap();
            let doc = board.doc().unwrap();
            let page = doc.first_page().unwrap();
            let labels = &board.session.tool.labels;
            let named: Vec<NodeId> = labels.iter().map(|(id, _)| *id).collect();
            assert_eq!(named, [frame, section]);
            for (id, quad) in labels {
                let n = doc.node(*id).unwrap();
                let world = doc.world_transform(*id);
                let (left, right) = (
                    world * PagePoint::ZERO,
                    world * PagePoint::new(n.width, 0.0),
                );
                // Cut with an ellipsis: from the frame's left side to no further than its right.
                assert!((quad[0].x - left.x).abs() < 1e-3, "{quad:?}");
                assert!(quad[1].x <= right.x + 1e-3, "{quad:?} past {right:?}");
                assert!(quad[1].x - quad[0].x > n.width / 2.0, "{quad:?}");
                // A press past the frame's right side, where the whole name would have run, is
                // not one on the name.
                let past = PagePoint::new(right.x + 10.0, (quad[0].y + quad[3].y) / 2.0);
                let pressed = figcraft_engine::tools::label_at(&doc, page, labels, past);
                assert_eq!(pressed, None);
            }
        });
    }

    #[test]
    fn open_issues_read_as_one_line() {
        let path = Path::new("/p/.nyawit/canvas.figcraft");
        assert_eq!(issues_line(path, &[]), None);
        assert_eq!(
            issues_line(path, &["an image was left out".into()]).as_deref(),
            Some("canvas.figcraft: an image was left out")
        );
        let two = ["a layer was repaired".into(), "a font is missing".into()];
        assert_eq!(
            issues_line(path, &two).as_deref(),
            Some(
                "canvas.figcraft: 2 problems were repaired or left out while opening it, \
                 e.g. a layer was repaired"
            )
        );
    }

    /// A design file in a session of its own, with a 100 × 50 rectangle at the page origin.
    fn design_with_a_rectangle() -> (figcraft_engine::Session, NodeId) {
        let mut session = figcraft_engine::Session::new();
        let rectangle = json!({"type": "rectangle", "x": 0, "y": 0, "width": 100, "height": 50});
        let made = session.execute("node.create", &rectangle).unwrap();
        (session, NodeId(made["id"].as_u64().unwrap()))
    }

    #[test]
    fn the_view_menus_render_toggles_reach_the_render() {
        let (mut session, rectangle) = design_with_a_rectangle();
        let job = |session: &figcraft_engine::Session| {
            let wanted = figcraft_render::View {
                origin: (0.0, 0.0),
                scale: 2.0,
                width: 100,
                height: 100,
            };
            super::render_job(session.ds().unwrap(), None, DocKind::Design, wanted)
        };
        let key = |session: &figcraft_engine::Session| super::job_key(&job(session), 1.0, false);
        let run = |session: &mut figcraft_engine::Session, id: &str, params: Value| {
            session.execute(id, &params).unwrap();
        };

        let plain = job(&session);
        let ds = session.ds().unwrap();
        assert_eq!((plain.uid, plain.page), (ds.uid, ds.page));
        assert!(!plain.opts.outline_mode && plain.opts.outlined.is_empty());
        assert_eq!(plain.opts.outline, figcraft_render::Outline::default());
        assert!(!plain.proof);
        assert_eq!(plain.pixels, None);
        let plain_key = key(&session);

        // Outlines ▸ Show outlines.
        run(&mut session, "view.outlines", json!({}));
        assert!(job(&session).opts.outline_mode);
        assert_ne!(key(&session), plain_key);
        run(&mut session, "view.outlines", json!({}));
        assert_eq!(key(&session), plain_key);

        // Show outlines for selection: the layers selected, and so none while none is.
        run(&mut session, "view.outlinesSelection", json!({}));
        session.set_selection(vec![rectangle]);
        assert_eq!(job(&session).opts.outlined, [rectangle]);
        assert_ne!(key(&session), plain_key);
        session.set_selection(Vec::new());
        assert!(job(&session).opts.outlined.is_empty());
        assert_eq!(key(&session), plain_key);

        // Include hidden layers and Include object bounds.
        run(&mut session, "view.outlinesHiddenLayers", json!({}));
        let hidden_key = key(&session);
        assert!(job(&session).opts.outline.hidden);
        run(&mut session, "view.outlinesObjectBounds", json!({}));
        assert!(job(&session).opts.outline.bounds);
        assert!(![plain_key, hidden_key].contains(&key(&session)));

        // Soft-proof sRGB: nothing to mark in an sRGB file, and the stripes in a Display P3 one.
        run(&mut session, "view.proofSrgb", json!({}));
        assert!(!job(&session).proof);
        let profile = json!({"profile": "displayP3"});
        run(&mut session, "file.colorProfile", profile);
        assert!(job(&session).proof);
        let proofed_key = key(&session);
        run(&mut session, "view.proofSrgb", json!({}));
        assert!(!job(&session).proof);
        assert_ne!(key(&session), proofed_key);

        // Pixel preview: at its resolution once zoomed in past it, and not before.
        session.ds_mut().unwrap().view.zoom = 4.0;
        let unpreviewed_key = key(&session);
        run(&mut session, "view.pixelPreview", json!({}));
        assert_eq!(job(&session).pixels, Some(1.0));
        assert_ne!(key(&session), unpreviewed_key);
        run(&mut session, "view.pixelPreview", json!({"scale": 2}));
        assert_eq!(job(&session).pixels, Some(2.0));
        session.ds_mut().unwrap().view.zoom = 2.0;
        assert_eq!(job(&session).pixels, None);
    }

    #[test]
    fn a_composition_is_rendered_in_place_of_the_document() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let made = board
            .execute("board.sticky", &json!({"x": 0, "y": 0}))
            .unwrap();
        let note = figcraft_engine::params::id_of(&made["id"]).unwrap();
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        figcraft_engine::textedit::preedit(&mut board.session, "ni", None).unwrap();
        let composition = Composition::of(&board.session).expect("a composition");
        let wanted = || figcraft_render::View {
            origin: (0.0, 0.0),
            scale: 2.0,
            width: 100,
            height: 100,
        };
        let ds = board.session.ds().unwrap();
        let composed = super::render_job(ds, Some(&composition), DocKind::Board, wanted());
        let plain = super::render_job(ds, None, DocKind::Board, wanted());
        // The sticky renders with the composition in it, which the document doesn't have.
        assert!(Arc::ptr_eq(&composed.doc, &composition.doc));
        assert!(Arc::ptr_eq(&plain.doc, &ds.doc));
        assert_eq!((composed.uid, composed.page), (plain.uid, plain.page));
        assert_ne!(
            super::job_key(&composed, 1.0, false),
            super::job_key(&plain, 1.0, false)
        );
    }

    /// The colour (BGRA) of the frame shown at page point `at`.
    fn frame_pixel(view: &Entity<BoardView>, cx: &TestAppContext, at: (f64, f64)) -> [u8; 4] {
        view.read_with(cx, |view, _| {
            let frame = view.frame.as_ref().unwrap();
            let pixel = |page: f64, origin: f64| ((page - origin) * frame.view.scale) as usize;
            let (x, y) = (
                pixel(at.0, frame.view.origin.0),
                pixel(at.1, frame.view.origin.1),
            );
            let start = (y * frame.view.width as usize + x) * 4;
            let bytes = frame.image.as_bytes(0).unwrap();
            bytes[start..start + 4].try_into().unwrap()
        })
    }

    #[gpui_kit::test]
    fn show_outlines_redraws_the_page_as_outlines(cx: &mut TestAppContext) {
        let tmp = Scratch::new("outlines");
        let path = tmp.0.join("design.figcraft");
        let (mut session, _) = design_with_a_rectangle();
        session
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        cx.update(gpui_kit::init);
        let (_, view) = open_in_window(cx, &path);
        let filled = next_frame(cx, &view, None);
        let (middle, outside) = ((50.0, 25.0), (-50.0, -50.0));
        let page = frame_pixel(&view, cx, outside);
        assert_ne!(frame_pixel(&view, cx, middle), page, "no rectangle");

        // ⇧⌘O's command: the canvas draws again, and the rectangle is its outline alone.
        let outlines = json!({"op": "command", "command": "view.outlines"});
        let reply = run_op(&view, outlines.clone(), cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let outlined = next_frame(cx, &view, Some(&filled));
        assert_eq!(frame_pixel(&view, cx, middle), page);

        // And back.
        run_op(&view, outlines, cx);
        next_frame(cx, &view, Some(&outlined));
        assert_ne!(frame_pixel(&view, cx, middle), page);
    }

    #[gpui_kit::test]
    fn scrolling_pans_and_zooms_about_the_pointer(cx: &mut TestAppContext) {
        let tmp = Scratch::new("scroll");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        let pointer = point(px(100.), px(50.));
        let scroll = |dx: f32, dy: f32, cmd: bool| ScrollEvent {
            position: pointer,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            modifiers: Modifiers {
                platform: cmd,
                ..Modifiers::default()
            },
            ..ScrollEvent::default()
        };
        let close =
            |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;

        // A plain scroll pans: the content follows the fingers.
        let before = view_state(&view, cx);
        view.update(cx, |view, cx| view.on_scroll(&scroll(30., -20., false), cx));
        let panned = view_state(&view, cx);
        assert_eq!(panned.zoom, before.zoom);
        assert!(close(
            panned.page_to_screen(0., 0.),
            (
                before.page_to_screen(0., 0.).0 + 30.,
                before.page_to_screen(0., 0.).1 - 20.
            )
        ));
        assert!(view.read_with(cx, |view, _| view.zooming_until.is_none()));

        // With ⌘ it zooms by e^(-dy/100), keeping the page point under the pointer there.
        let under = panned.screen_to_page(100., 50.);
        view.update(cx, |view, cx| view.on_scroll(&scroll(0., -20., true), cx));
        let zoomed = view_state(&view, cx);
        assert!((zoomed.zoom - panned.zoom * 0.2f64.exp()).abs() < 1e-9);
        assert!(close(zoomed.screen_to_page(100., 50.), under));
        assert!(view.read_with(cx, |view, _| view.zooming_until.is_some()));

        // A pinch zooms by 1 + its delta, about the pointer too.
        let pinch = PinchEvent {
            position: pointer,
            delta: 0.5,
            ..PinchEvent::default()
        };
        view.update(cx, |view, cx| view.on_pinch(&pinch, cx));
        let pinched = view_state(&view, cx);
        assert!((pinched.zoom - zoomed.zoom * 1.5).abs() < 1e-9);
        assert!(close(pinched.screen_to_page(100., 50.), under));

        // Full quality comes back once the gesture has been still for a moment.
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.zooming_until.is_none()));
    }

    /// A new, empty canvas in a 400 × 300 window, painted once: under the top bar, at 100%, with
    /// page point (0, 0) in the middle of the canvas, (200, 168) in the window. What simulates
    /// input in that window, and the app.
    fn painted_canvas(
        cx: &mut TestAppContext,
        tmp: &Scratch,
    ) -> (Entity<BoardView>, VisualTestContext) {
        cx.update(gpui_kit::init);
        let (window, view) = open_in_window(cx, &tmp.0.join("canvas.figcraft"));
        next_frame(cx, &view, None);
        (view, VisualTestContext::from_window(window.into(), cx))
    }

    /// The top-level layers of the canvas's page.
    pub(super) fn layers(
        view: &Entity<BoardView>,
        cx: &TestAppContext,
    ) -> Vec<figcraft_doc::NodeId> {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            doc.node(doc.first_page().unwrap())
                .unwrap()
                .children
                .clone()
        })
    }

    #[gpui_kit::test]
    fn a_drag_out_of_the_window_ends_as_one_undo_step(cx: &mut TestAppContext) {
        let tmp = Scratch::new("drag-out");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let tool = json!({"op": "command", "command": "tool.rectangle"});
        let reply = run_op(&view, tool, &mut cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let (left, none) = (MouseButton::Left, Modifiers::default());

        cx.simulate_mouse_down(point(px(100.), px(100.)), left, none);
        // The press gave the canvas the keyboard.
        assert!(cx.update(|window, cx| view.read(cx).focus.is_focused(window)));
        cx.simulate_mouse_move(point(px(150.), px(140.)), left, none);
        // Out of the window: only the window's listeners see the last move and the release.
        cx.simulate_mouse_move(point(px(600.), px(500.)), left, none);
        cx.simulate_mouse_up(point(px(600.), px(500.)), left, none);

        let [drawn] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        view.read_with(&cx, |view, _| {
            assert!(!view.dragging);
            let board = view.board().unwrap();
            // The gesture is over: the tool is free, and back to Move once the shape is drawn.
            assert!(!board.session.tool.busy());
            assert_eq!(board.session.tool.tool.id(), "move");
            // From the press to the last move, out of the window: 500 × 400 page units at 100%.
            let doc = board.doc().unwrap();
            let node = doc.node(drawn).unwrap();
            assert_eq!((node.width, node.height), (500.0, 400.0));
            assert_eq!(board.session.undo_depth(), (1, 0));
        });
        let undo = json!({"op": "command", "command": "edit.undo"});
        let reply = run_op(&view, undo, &mut cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert!(layers(&view, &cx).is_empty());
    }

    #[gpui_kit::test]
    fn a_press_ends_a_drag_whose_release_never_came(cx: &mut TestAppContext) {
        let tmp = Scratch::new("lost-release");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let tool = json!({"op": "command", "command": "tool.rectangle"});
        run_op(&view, tool, &mut cx);
        let (left, none) = (MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_down(point(px(100.), px(100.)), left, none);
        cx.simulate_mouse_move(point(px(150.), px(140.)), left, none);
        // The release went elsewhere, as when the tab is hidden mid-drag. The next press ends
        // that drag as it was drawn, then makes its own gesture.
        cx.simulate_mouse_down(point(px(300.), px(200.)), left, none);
        cx.simulate_mouse_up(point(px(300.), px(200.)), left, none);

        let [drawn] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        view.read_with(&cx, |view, _| {
            let board = view.board().unwrap();
            assert!(!board.session.tool.busy());
            let doc = board.doc().unwrap();
            let node = doc.node(drawn).unwrap();
            assert_eq!((node.width, node.height), (50.0, 40.0));
            assert_eq!(board.session.undo_depth(), (1, 0));
        });
    }

    #[gpui_kit::test]
    fn a_double_click_types_in_a_sticky(cx: &mut TestAppContext) {
        let tmp = Scratch::new("double-click");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        // A 200 × 200 sticky about page point (0, 0), the middle of the canvas, not selected.
        let sticky = json!({
            "op": "command", "command": "board.sticky", "params": {"x": -100, "y": -100},
        });
        run_op(&view, sticky, &mut cx);
        run_op(
            &view,
            json!({"op": "command", "command": "select.none"}),
            &mut cx,
        );
        let (middle, button, modifiers) = (
            point(px(200.), px(150.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        for click_count in [1, 2] {
            cx.simulate_event(MouseDownEvent {
                position: middle,
                button,
                modifiers,
                click_count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position: middle,
                button,
                modifiers,
                click_count,
            });
        }

        let [note] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        view.read_with(&cx, |view, _| {
            let ds = view.board().unwrap().session.ds().unwrap();
            let text = figcraft_engine::board::text_of(&ds.doc, note).unwrap();
            assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
        });
    }

    #[gpui_kit::test]
    fn a_triple_click_selects_the_line_being_typed(cx: &mut TestAppContext) {
        let tmp = Scratch::new("triple-click");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        // A 200 × 200 sticky about page point (0, 0), reading one line, not selected.
        let sticky = json!({
            "op": "command", "command": "board.sticky",
            "params": {"x": -100, "y": -100, "text": "one two"},
        });
        run_op(&view, sticky, &mut cx);
        run_op(
            &view,
            json!({"op": "command", "command": "select.none"}),
            &mut cx,
        );
        let [note] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        // The middle of its text, in the window.
        let (text, on_text) = view.read_with(&cx, |view, _| {
            let ds = view.board().unwrap().session.ds().unwrap();
            let text = figcraft_engine::board::text_of(&ds.doc, note).unwrap();
            let b = ds.doc.world_bounds(text).unwrap();
            let (x, y) = ds
                .view
                .page_to_screen((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
            let origin = view.bounds.origin;
            (
                text,
                point(origin.x + px(x as f32), origin.y + px(y as f32)),
            )
        });
        let (button, modifiers) = (MouseButton::Left, Modifiers::default());
        for click_count in [1, 2, 3] {
            cx.simulate_event(MouseDownEvent {
                position: on_text,
                button,
                modifiers,
                click_count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position: on_text,
                button,
                modifiers,
                click_count,
            });
        }

        // The second click started typing with all of the text selected, and the third selected
        // the line it is on, all of "one two", where a press alone leaves a caret.
        view.read_with(&cx, |view, _| {
            let session = &view.board().unwrap().session;
            let selected = figcraft_engine::textedit::selected_range(session);
            assert_eq!(selected, Some((text, 0, 7)));
        });
    }

    #[gpui_kit::test]
    fn leaving_the_canvas_clears_what_hovering_showed(cx: &mut TestAppContext) {
        let tmp = Scratch::new("leave");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        // What Alt measured from over the canvas, outlined.
        let measure = |cx: &mut TestAppContext| {
            view.update(cx, |view, _| {
                let overlay = &mut view.board_mut().unwrap().session.tool.overlay;
                overlay.measure_box = Some([0.0, 0.0, 10.0, 10.0]);
            })
        };
        let measured = |cx: &TestAppContext| {
            view.read_with(cx, |view, _| {
                view.board().unwrap().session.tool.overlay.measure_box
            })
        };
        let (over, none) = (point(px(200.), px(150.)), Modifiers::default());

        // From over the canvas to off it (it is as wide as the window).
        cx.simulate_mouse_move(over, None, none);
        measure(&mut cx);
        cx.simulate_mouse_move(point(px(600.), px(150.)), None, none);
        assert_eq!(measured(&cx), None);

        // From over the canvas out of the window, which comes with no move off the canvas first.
        cx.simulate_mouse_move(over, None, none);
        measure(&mut cx);
        cx.simulate_event(MouseExitEvent {
            position: point(px(200.), px(-1.)),
            pressed_button: None,
            modifiers: none,
        });
        assert_eq!(measured(&cx), None);
    }

    #[gpui_kit::test]
    fn an_ops_requests_reach_the_view(cx: &mut TestAppContext) {
        let tmp = Scratch::new("op-requests");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        run_op(&view, sticky_op(), cx);
        // ⇧⌘↩'s command, with typing going on in the new sticky: the engine asks the view to
        // start it, and an op's requests reach the view as an input's do.
        let next_sticky =
            json!({"op": "command", "command": "board.nextSticky", "params": {"edit": true}});
        let reply = run_op(&view, next_sticky, cx);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        let next = figcraft_engine::params::id_of(&reply["result"]["id"]).unwrap();
        view.read_with(cx, |view, _| {
            let board = view.board().unwrap();
            let ds = board.session.ds().unwrap();
            let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
            assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
            assert!(board.session.ui_requests.is_empty());
        });
    }

    /// The app's key bindings, bound in the app's order: gpui-kit's, then the workspace's, which
    /// binds the canvas's after its own, then the three `cli::init` binds for the whole app.
    fn bind_app_keys(cx: &mut App) {
        gpui_kit::init(cx);
        crate::workspace::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-q", crate::cli::Quit, None),
            KeyBinding::new("cmd-shift-n", crate::cli::NewWindow, None),
            KeyBinding::new("cmd-o", crate::cli::OpenFolder, None),
        ]);
    }

    /// The key contexts from the window's root to the focused view.
    fn contexts(names: &[&str]) -> Vec<KeyContext> {
        names
            .iter()
            .map(|name| KeyContext::parse(name).unwrap())
            .collect()
    }

    /// The key contexts of the canvas in the workspace: gpui-kit's root, the workspace, then the
    /// canvas, which is `Board BoardText` while it edits text.
    fn canvas_contexts(editing: bool) -> Vec<KeyContext> {
        let canvas = if editing { "Board BoardText" } else { "Board" };
        contexts(&["Root", "Workspace", canvas])
    }

    /// What pressing `keystroke` (written as `gpui_keystroke` writes it) does with the focus in
    /// `contexts`: the actions its bindings run, by name, and whether it waits for the next key of
    /// a chord. A key no action takes goes on to the focused view's `on_key_down`.
    fn resolve(cx: &App, keystroke: &str, contexts: &[KeyContext]) -> (Vec<&'static str>, bool) {
        let typed = Keystroke::parse_with_os(keystroke, OperatingSystem::Mac).unwrap();
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, pending) = keymap.bindings_for_input(&[typed], contexts);
        let actions = bindings.iter().map(|b| b.action().name()).collect();
        (actions, pending)
    }

    #[gpui_kit::test]
    fn board_keys_reach_the_canvas(cx: &mut TestAppContext) {
        cx.update(bind_app_keys);
        let kept = cx.update(|cx| {
            let mut kept = Vec::new();
            for binding in KEYS.iter().filter(|b| !policy::is_hidden(b.id)) {
                for spec in binding.keys(Platform::Mac) {
                    let keystroke = gpui_keystroke(spec).unwrap();
                    let sawits = |keys: &[&str]| keys.contains(&keystroke.as_str());
                    // On the canvas, each key FigCraft uses there, but the keys Sawit keeps.
                    // While text is edited, each of its keys but those Sawit always keeps: the
                    // text shortcuts, and the canvas's keys the text editor takes (⌘C, ⌘A, ⌘Z…).
                    let live_on_canvas = binding.context != keymap::Context::Text
                        && !sawits(SAWIT_WINS)
                        && !sawits(SAWIT_WINS_UNLESS_EDITING);
                    let live_while_editing = !sawits(SAWIT_WINS);
                    for (live, editing) in [(live_on_canvas, false), (live_while_editing, true)] {
                        let (actions, pending) = resolve(cx, &keystroke, &canvas_contexts(editing));
                        if live && (!actions.is_empty() || pending) {
                            kept.push(format!(
                                "{keystroke} ({}, editing {editing}): {actions:?}{}",
                                binding.id,
                                if pending { ", waits for a chord" } else { "" },
                            ));
                        }
                    }
                }
            }
            kept
        });
        assert!(
            kept.is_empty(),
            "kept from the canvas:\n{}",
            kept.join("\n")
        );
    }

    #[gpui_kit::test]
    fn sawit_keeps_its_shortcuts(cx: &mut TestAppContext) {
        cx.update(bind_app_keys);
        let workspace = contexts(&["Root", "Workspace"]);
        cx.update(|cx| {
            for keystroke in SAWIT_WINS {
                let (in_workspace, _) = resolve(cx, keystroke, &workspace);
                assert!(!in_workspace.is_empty(), "{keystroke} is no Sawit shortcut");
                // On the canvas, and while it edits text.
                for editing in [false, true] {
                    let (on_canvas, pending) = resolve(cx, keystroke, &canvas_contexts(editing));
                    assert_eq!(on_canvas, in_workspace, "{keystroke}, editing {editing}");
                    assert!(!pending, "{keystroke} waits for a chord, editing {editing}");
                }
            }
        });
    }

    #[gpui_kit::test]
    fn text_editing_takes_text_keys(cx: &mut TestAppContext) {
        cx.update(bind_app_keys);
        cx.update(|cx| {
            for (keystroke, sawits) in [
                ("cmd-b", "workspace::ToggleSidebar"),
                ("alt-cmd-t", "workspace::CloseOtherTabs"),
                ("shift-cmd-enter", "workspace::ToggleMaximizePane"),
            ] {
                // While text is edited they are bold, centre and the next sticky: the canvas's.
                let (editing, pending) = resolve(cx, keystroke, &canvas_contexts(true));
                assert!(editing.is_empty() && !pending, "{keystroke}: {editing:?}");
                // Otherwise they stay Sawit's.
                let (on_canvas, _) = resolve(cx, keystroke, &canvas_contexts(false));
                assert_eq!(on_canvas.first(), Some(&sawits), "{keystroke}");
            }
        });
    }

    /// Gives the canvas the keyboard, as a press on it does.
    pub(super) fn focus_canvas(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
    }

    /// Starts typing in the sticky that a command op made, `made` being its answer, and returns
    /// the sticky.
    fn type_in(view: &Entity<BoardView>, made: &Value, cx: &mut TestAppContext) -> NodeId {
        let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        view.update(cx, |view, cx| {
            let board = view.board_mut().unwrap();
            assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
            // Redrawn, as after the input that starts typing.
            cx.notify();
        });
        note
    }

    #[gpui_kit::test]
    fn a_keys_requests_reach_the_view(cx: &mut TestAppContext) {
        let tmp = Scratch::new("key-requests");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        let first = type_in(&view, &made, &mut cx);
        focus_canvas(&view, &mut cx);
        // ⇧⌘↩ while typing makes the next sticky, and the engine asks the view to go on typing
        // there.
        cx.simulate_keystrokes("shift-cmd-enter");
        let [one, next] = layers(&view, &cx)[..] else {
            panic!("layers {:?}", layers(&view, &cx))
        };
        assert_eq!(one, first);
        view.read_with(&cx, |view, _| {
            let board = view.board().unwrap();
            let ds = board.session.ds().unwrap();
            let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
            assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
            assert!(board.session.ui_requests.is_empty());
        });
    }

    #[gpui_kit::test]
    fn keys_reach_the_tools_and_sawits_own_commands_say_coming_soon(cx: &mut TestAppContext) {
        let tmp = Scratch::new("key-tools");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("r");
        let tool = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| view.board().unwrap().session.tool.tool.id())
        };
        assert_eq!(tool(&cx), "rectangle");
        // ⌘S is the save, which Sawit carries out itself, once Plan 6 has it.
        cx.simulate_keystrokes("cmd-s");
        let toast = view.read_with(&cx, |view, _| view.toast.clone());
        assert_eq!(toast.as_deref(), Some("Coming soon"));
    }

    /// Releases the keys of `keystrokes`, last first, as `simulate_keystrokes` presses them.
    fn release_keys(cx: &mut VisualTestContext, keystrokes: &str) {
        for source in keystrokes.split(' ').rev() {
            let keystroke = Keystroke::parse_with_os(source, OperatingSystem::Mac).unwrap();
            cx.simulate_event(KeyUpEvent { keystroke });
        }
    }

    /// A drag of `button` from (100, 100) to (160, 140) in the window, which the canvas fills.
    fn drag(cx: &mut VisualTestContext, button: MouseButton) {
        let none = Modifiers::default();
        cx.simulate_mouse_down(point(px(100.), px(100.)), button, none);
        cx.simulate_mouse_move(point(px(130.), px(120.)), button, none);
        cx.simulate_mouse_move(point(px(160.), px(140.)), button, none);
        cx.simulate_mouse_up(point(px(160.), px(140.)), button, none);
    }

    /// Whether `a` and `b` are the same page point, but for rounding.
    fn same_point(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    #[gpui_kit::test]
    fn a_middle_button_drag_pans_with_any_tool(cx: &mut TestAppContext) {
        let tmp = Scratch::new("middle-pan");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let tool = json!({"op": "command", "command": "tool.rectangle"});
        run_op(&view, tool, &mut cx);
        let pressed = view_state(&view, &cx).screen_to_page(100., 100.);
        drag(&mut cx, MouseButton::Middle);
        // The page point under the press is under the release.
        let released = view_state(&view, &cx).screen_to_page(160., 140.);
        assert!(same_point(released, pressed), "{released:?} != {pressed:?}");
        // The tools got nothing: no shape, no undo step, the tool still on.
        assert!(layers(&view, &cx).is_empty());
        view.read_with(&cx, |view, _| {
            assert!(view.pan.is_none());
            let board = view.board().unwrap();
            assert_eq!(board.session.tool.tool.id(), "rectangle");
            assert_eq!(board.session.undo_depth(), (0, 0));
        });
    }

    #[gpui_kit::test]
    fn space_and_a_drag_pan_instead_of_drawing(cx: &mut TestAppContext) {
        let tmp = Scratch::new("space-pan");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let tool = json!({"op": "command", "command": "tool.rectangle"});
        run_op(&view, tool, &mut cx);
        focus_canvas(&view, &mut cx);

        // Held, Space turns a drag into a pan.
        cx.simulate_keystrokes("space");
        let pressed = view_state(&view, &cx).screen_to_page(100., 100.);
        drag(&mut cx, MouseButton::Left);
        let released = view_state(&view, &cx).screen_to_page(160., 140.);
        assert!(same_point(released, pressed), "{released:?} != {pressed:?}");
        assert!(layers(&view, &cx).is_empty());

        // Released, the same drag draws.
        release_keys(&mut cx, "space");
        drag(&mut cx, MouseButton::Left);
        assert_eq!(layers(&view, &cx).len(), 1);
    }

    #[gpui_kit::test]
    fn space_is_typed_while_text_is_edited(cx: &mut TestAppContext) {
        let tmp = Scratch::new("space-typed");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        type_in(&view, &made, &mut cx);
        focus_canvas(&view, &mut cx);
        let space = KeyDownEvent {
            keystroke: Keystroke::parse("space").unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        let result =
            cx.update(|window, cx| window.dispatch_event(PlatformInput::KeyDown(space), cx));
        // It goes on past the canvas, to the input method, and arms no pan.
        assert!(result.propagate);
        view.read_with(&cx, |view, _| {
            assert!(!view.space_held);
            let ds = view.board().unwrap().session.ds().unwrap();
            assert!(ds.text_edit.is_some());
        });
    }

    #[gpui_kit::test]
    fn losing_the_keyboard_lets_go_of_space(cx: &mut TestAppContext) {
        let tmp = Scratch::new("space-lost");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        // The window has the keyboard, as the one the user types in.
        cx.update(|window, _| window.activate_window());
        cx.run_until_parked();
        let held = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.space_held);
        // The canvas loses the keyboard in the window: Space's release goes elsewhere.
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("space");
        assert!(held(&cx));
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!held(&cx));
        // The window loses it to another.
        focus_canvas(&view, &mut cx);
        cx.simulate_keystrokes("space");
        assert!(held(&cx));
        cx.deactivate_window();
        assert!(!held(&cx));
    }

    /// The tests that copy layers, or paste the app's copy, share that copy
    /// (`sawit_board::clipboard`): they take turns.
    static CLIPBOARD_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(super) fn clipboard_turn() -> std::sync::MutexGuard<'static, ()> {
        (CLIPBOARD_TURN.lock()).unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The text on the system clipboard.
    fn system_text(cx: &TestAppContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    /// Puts `text` on the system clipboard, as copying it in another app does.
    fn copy_in_another_app(cx: &TestAppContext, text: &str) {
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text.into()));
    }

    /// What board object `id` (a sticky) or text layer `id` reads.
    fn reads(view: &Entity<BoardView>, id: NodeId, cx: &TestAppContext) -> String {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            let text = figcraft_engine::board::text_of(&doc, id).unwrap_or(id);
            let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
            text.characters.clone()
        })
    }

    /// Calls the canvas through `input`, as the platform's text input calls it.
    fn with_input<R>(
        view: &Entity<BoardView>,
        cx: &mut VisualTestContext,
        input: impl FnOnce(&mut BoardView, &mut Window, &mut Context<BoardView>) -> R,
    ) -> R {
        cx.update(|window, cx| view.update(cx, |view, cx| input(view, window, cx)))
    }

    /// An input method composes `text`, its cursor at the end.
    fn mark(view: &Entity<BoardView>, text: &str, cx: &mut VisualTestContext) {
        with_input(view, cx, |view, window, cx| {
            view.replace_and_mark_text_in_range(None, text, None, window, cx)
        });
    }

    /// The input method takes `text` as typed, in place of its composition or the selection.
    fn commit(view: &Entity<BoardView>, text: &str, cx: &mut VisualTestContext) {
        with_input(view, cx, |view, window, cx| {
            view.replace_text_in_range(None, text, window, cx)
        });
    }

    fn unmark(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
        with_input(view, cx, |view, window, cx| view.unmark_text(window, cx));
    }

    /// What sticky `id` reads in the document.
    fn sticky_reads(view: &Entity<BoardView>, id: NodeId, cx: &VisualTestContext) -> String {
        view.read_with(cx, |view, _| {
            let doc = view.board().unwrap().doc().unwrap();
            let text = figcraft_engine::board::text_of(&doc, id).unwrap();
            let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
            text.characters.clone()
        })
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
        let mut canvas = |name: &str| {
            let (window, view) = open_in_window(cx, &tmp.0.join(name));
            next_frame(cx, &view, None);
            (view, VisualTestContext::from_window(window.into(), cx))
        };
        let (one, mut one_cx) = canvas("one.figcraft");
        let (other, mut other_cx) = canvas("other.figcraft");
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

    /// What an input method composes in the text being edited: shown there, not in the document.
    fn composing(view: &Entity<BoardView>, cx: &VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| {
            let ds = view.board().unwrap().session.ds().unwrap();
            let preedit = ds.text_edit.as_ref()?.preedit.as_ref()?;
            Some(preedit.text.clone())
        })
    }

    fn editing(view: &Entity<BoardView>, cx: &VisualTestContext) -> bool {
        view.read_with(cx, |view, _| view.editing_text())
    }

    #[gpui_kit::test]
    fn input_handler_inserts_text(cx: &mut TestAppContext) {
        let tmp = Scratch::new("input-handler");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        let note = type_in(&view, &made, &mut cx);
        commit(&view, "ok", &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "ok");
        assert!(editing(&view, &cx));
    }

    #[gpui_kit::test]
    fn typed_keys_reach_the_text_being_edited(cx: &mut TestAppContext) {
        let tmp = Scratch::new("typed-keys");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        let note = type_in(&view, &made, &mut cx);
        focus_canvas(&view, &mut cx);
        // The canvas leaves a typed character alone, and it comes back as text through the text
        // input the canvas's paint hands the window while it edits.
        cx.simulate_input("ok");
        assert_eq!(sticky_reads(&view, note, &cx), "ok");
        // Space too, which arms a pan only while no text is edited (ruling P2-C1).
        cx.simulate_keystrokes("space");
        assert_eq!(sticky_reads(&view, note, &cx), "ok ");
        // Named keys are the text editor's own.
        cx.simulate_keystrokes("backspace backspace");
        assert_eq!(sticky_reads(&view, note, &cx), "o");
        cx.simulate_keystrokes("escape");
        assert!(!editing(&view, &cx));
        assert_eq!(sticky_reads(&view, note, &cx), "o");
    }

    /// Review Focus 3 through the platform's text input (ruling P2-C9): a composition the input
    /// method cancels, or one a press away from the text leaves behind, leaves no text.
    #[gpui_kit::test]
    fn a_cancelled_composition_leaves_no_text(cx: &mut TestAppContext) {
        let tmp = Scratch::new("ime-cancel");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        // A 200 × 200 sticky about page point (0, 0), the middle of the canvas, typed in.
        let sticky = json!({
            "op": "command", "command": "board.sticky", "params": {"x": -100, "y": -100},
        });
        let made = run_op(&view, sticky, &mut cx);
        let note = type_in(&view, &made, &mut cx);
        // Marked, the composition shows in the sticky but isn't in the document.
        mark(&view, "ni", &mut cx);
        assert_eq!(composing(&view, &cx).as_deref(), Some("ni"));
        assert_eq!(sticky_reads(&view, note, &cx), "");
        // The input method cancels it (Escape while it composes): marked empty, it is gone.
        mark(&view, "", &mut cx);
        assert_eq!(composing(&view, &cx), None);
        assert_eq!(sticky_reads(&view, note, &cx), "");
        assert!(editing(&view, &cx));

        // Marked again, then a press on the canvas away from the sticky ends the edit, and the
        // unmark the input method sends after that commits nothing.
        mark(&view, "ni", &mut cx);
        let (away, left, none) = (
            point(px(20.), px(60.)),
            MouseButton::Left,
            Modifiers::default(),
        );
        cx.simulate_mouse_down(away, left, none);
        cx.simulate_mouse_up(away, left, none);
        assert!(!editing(&view, &cx));
        unmark(&view, &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "");
        assert_eq!(layers(&view, &cx), [note]);

        // Each time typing in the sticky starts, it is another edit of the same text. The input
        // method marks text in one; once a second has started, the composition there isn't the
        // one it made, and its unmark doesn't commit it.
        type_in(&view, &made, &mut cx);
        mark(&view, "ni", &mut cx);
        type_in(&view, &made, &mut cx);
        view.update(&mut cx, |view, _| {
            let session = &mut view.board_mut().unwrap().session;
            figcraft_engine::textedit::preedit(session, "ni", None).unwrap();
        });
        unmark(&view, &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "");
    }

    #[gpui_kit::test]
    fn the_input_method_commits_its_composition(cx: &mut TestAppContext) {
        let tmp = Scratch::new("ime-commit");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        let note = type_in(&view, &made, &mut cx);
        // Composing, the canvas renders the sticky with the composition in it.
        mark(&view, "ni", &mut cx);
        view.read_with(&cx, |view, _| {
            let composition = view.composition.as_ref().expect("the composition shown");
            let (_, asked) = view.requested.as_ref().expect("a render");
            assert!(Arc::ptr_eq(asked, &composition.doc));
        });
        // Committed in its final form, that takes the composition's place, in the document.
        commit(&view, "你", &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "你");
        assert_eq!(composing(&view, &cx), None);
        view.read_with(&cx, |view, _| {
            assert!(view.composition.is_none());
            let doc = view.board().unwrap().doc().unwrap();
            let (_, asked) = view.requested.as_ref().unwrap();
            assert!(Arc::ptr_eq(asked, &doc));
        });
        // Unmarked, the input method keeps its composition as it is: it goes in as composed.
        mark(&view, "hao", &mut cx);
        unmark(&view, &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "你hao");
        assert_eq!(composing(&view, &cx), None);
        // Once committed there is nothing left to commit.
        unmark(&view, &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "你hao");
        assert!(editing(&view, &cx));
    }

    #[gpui_kit::test]
    fn the_input_method_reads_the_text_in_utf16(cx: &mut TestAppContext) {
        let tmp = Scratch::new("ime-utf16");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        type_in(&view, &made, &mut cx);
        // An emoji is two UTF-16 units: "a😀b" is four, and the caret is after the b.
        commit(&view, "a😀b", &mut cx);
        let selected = |cx: &mut VisualTestContext| {
            with_input(&view, cx, |view, window, cx| {
                view.selected_text_range(false, window, cx)
            })
            .expect("a selection")
        };
        let read = |range: Range<usize>, cx: &mut VisualTestContext| {
            let mut adjusted = None;
            let text = with_input(&view, cx, |view, window, cx| {
                view.text_for_range(range, &mut adjusted, window, cx)
            });
            (text.expect("the text"), adjusted.expect("the range read"))
        };
        let marked = |cx: &mut VisualTestContext| {
            with_input(&view, cx, |view, window, cx| {
                view.marked_text_range(window, cx)
            })
        };
        let caret = selected(&mut cx);
        assert_eq!((caret.range, caret.reversed), (4..4, false));
        assert_eq!(read(0..4, &mut cx), ("a😀b".to_string(), 0..4));
        // Half the emoji reads as all of it.
        assert_eq!(read(2..3, &mut cx), ("😀".to_string(), 1..3));
        assert_eq!(marked(&mut cx), None);

        // Composing, the composition is in the text it reads, marked, with its cursor in it.
        mark(&view, "ni", &mut cx);
        assert_eq!(marked(&mut cx), Some(4..6));
        assert_eq!(selected(&mut cx).range, 6..6);
        assert_eq!(read(0..6, &mut cx).0, "a😀bni");

        // Its candidates show by the caret, where the canvas draws it, in the window.
        let canvas = Bounds::new(point(px(10.), px(20.)), size(px(400.), px(300.)));
        let bounds = with_input(&view, &mut cx, |view, window, cx| {
            view.bounds_for_range(6..6, canvas, window, cx)
        })
        .expect("where the caret is");
        let (top, bottom) = view.read_with(&cx, |view, _| {
            let shapes = overlay::overlay(&view.board().unwrap().session);
            let caret = |shape: &Shape| match shape {
                Shape::Caret { top, bottom, .. } => Some((*top, *bottom)),
                _ => None,
            };
            shapes.iter().find_map(caret).expect("a caret")
        });
        let at = |(x, y): (f64, f64)| point(px(10. + x as f32), px(20. + y as f32));
        assert_eq!(bounds.origin, at(top));
        assert_eq!(bounds.bottom_left(), at(bottom));
    }

    #[gpui_kit::test]
    fn an_accent_held_down_replaces_the_letter_before_the_caret(cx: &mut TestAppContext) {
        let tmp = Scratch::new("ime-replace");
        let (view, mut cx) = painted_canvas(cx, &tmp);
        let made = run_op(&view, sticky_op(), &mut cx);
        let note = type_in(&view, &made, &mut cx);
        commit(&view, "cafe", &mut cx);
        let replace = |range: Range<usize>, text: &str, cx: &mut VisualTestContext| {
            with_input(&view, cx, |view, window, cx| {
                view.replace_text_in_range(Some(range), text, window, cx)
            });
        };
        let caret = |cx: &mut VisualTestContext| {
            with_input(&view, cx, |view, window, cx| {
                view.selected_text_range(false, window, cx)
            })
            .expect("a selection")
            .range
        };
        // The accents menu (e held down, then é chosen) puts its letter in place of the e.
        replace(3..4, "é", &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "café");
        assert_eq!(caret(&mut cx), 4..4);
        // A range apart from the caret is replaced where it is, the caret after it.
        replace(0..1, "C", &mut cx);
        assert_eq!(sticky_reads(&view, note, &cx), "Café");
        assert_eq!(caret(&mut cx), 1..1);
    }

    /// A new, empty canvas in an 800 × 600 window, wide enough for the whole toolbar, painted
    /// once: under the top bar, at 100%, with page point (0, 0) in the middle of the canvas.
    pub(super) fn chrome_canvas(
        cx: &mut TestAppContext,
        tmp: &Scratch,
    ) -> (Entity<BoardView>, VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.executor().allow_parking();
        let path = tmp.0.join("canvas.figcraft");
        let window = cx.open_window(size(px(800.), px(600.)), |_, cx| BoardView::open(path, cx));
        let view = window.root(cx).unwrap();
        next_frame(cx, &view, None);
        (view, VisualTestContext::from_window(window.into(), cx))
    }

    /// Clicks the middle of what the window drew with debug selector `selector`.
    pub(super) fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let drawn = cx.debug_bounds(selector);
        let bounds = drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    /// The tool the canvas's pointer events go to.
    fn active_tool(view: &Entity<BoardView>, cx: &TestAppContext) -> ToolId {
        view.read_with(cx, |view, _| {
            sawit_board::toolbar::active(&view.board().unwrap().session)
        })
    }

    #[gpui_kit::test]
    fn toolbar_click_selects_tool(cx: &mut TestAppContext) {
        let tmp = Scratch::new("toolbar-click");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        assert_eq!(active_tool(&view, &cx), ToolId::Move);
        // What the Sticky button does.
        view.update(&mut cx, |view, cx| {
            view.choose("tool.sticky", json!({}), cx)
        });
        assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
        cx.run_until_parked();
        // A click on a tool's button chooses it.
        click(&mut cx, "tool.hand");
        assert_eq!(active_tool(&view, &cx), ToolId::Hand);
        // The toolbar takes its clicks: with the sticky tool on, a click on a button puts no
        // sticky on the canvas under it.
        click(&mut cx, "tool.sticky");
        assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
        click(&mut cx, "tool.hand");
        assert_eq!(active_tool(&view, &cx), ToolId::Hand);
        assert!(layers(&view, &cx).is_empty());
    }

    #[gpui_kit::test]
    fn toggle_ui_hides_chrome(cx: &mut TestAppContext) {
        let tmp = Scratch::new("toggle-ui");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        // ⌘\, as the canvas gets it while it has the keyboard (ruling P5-C3).
        focus_canvas(&view, &mut cx);
        let toggle = |cx: &mut VisualTestContext| {
            let event = KeyDownEvent {
                keystroke: Keystroke::parse("cmd-\\").unwrap(),
                is_held: false,
                prefer_character_input: false,
            };
            cx.update(|window, cx| view.update(cx, |view, cx| view.on_key(&event, window, cx)));
            cx.run_until_parked();
        };
        let hidden = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                view.board().unwrap().session.ds().unwrap().view.ui_hidden
            })
        };
        let chrome = |cx: &mut VisualTestContext| {
            ["board-top-bar", "board-toolbar"].map(|selector| cx.debug_bounds(selector).is_some())
        };
        let canvas_height = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| f32::from(view.bounds.size.height))
        };
        assert!(!hidden(&cx));
        assert_eq!(chrome(&mut cx), [true, true]);
        // The canvas is the window, less the top bar.
        assert_eq!(canvas_height(&cx), 600. - top_bar::HEIGHT);

        // Only the canvas renders, and it fills the window.
        toggle(&mut cx);
        assert!(hidden(&cx));
        assert_eq!(chrome(&mut cx), [false, false]);
        assert_eq!(canvas_height(&cx), 600.);

        toggle(&mut cx);
        assert!(!hidden(&cx));
        assert_eq!(chrome(&mut cx), [true, true]);
        assert_eq!(canvas_height(&cx), 600. - top_bar::HEIGHT);
    }

    #[gpui_kit::test]
    fn flyout_closes_on_kind_change(cx: &mut TestAppContext) {
        let tmp = Scratch::new("flyout-kind");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let open = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.open_flyout);
        view.update(&mut cx, |view, cx| {
            view.open_flyout = Some(Flyout::StickyColors);
            cx.notify();
        });
        cx.run_until_parked();
        // Drawn over the board whose tool it belongs to, it stays open.
        assert_eq!(open(&cx), Some(Flyout::StickyColors));
        assert!(cx.debug_bounds("board-flyout").is_some());

        // A design file takes the board's place: drawn again, the flyout is closed, and the
        // toolbar is the design file's.
        view.update(&mut cx, |view, cx| {
            let design = figcraft_engine::Session::with_document(Document::new("Design"));
            view.replace_doc(BoardDoc { session: design });
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(open(&cx), None);
        assert!(cx.debug_bounds("board-flyout").is_none());
        assert!(cx.debug_bounds("tool.frame").is_some());
        assert!(cx.debug_bounds("tool.sticky").is_none());
    }

    #[gpui_kit::test]
    fn a_flyout_choice_chooses_its_tool_and_closes_the_flyout(cx: &mut TestAppContext) {
        let tmp = Scratch::new("flyout-choice");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let open = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.open_flyout);
        let sticky_colour = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                let session = &view.board().unwrap().session;
                let items = sawit_board::toolbar::flyout(session, Flyout::StickyColors);
                let chosen = items.into_iter().find(|item| item.selected);
                chosen.map(|item| item.label)
            })
        };
        assert_eq!(sticky_colour(&cx).as_deref(), Some("yellow"));
        // The chevron beside the Sticky button opens the colours of new stickies.
        click(&mut cx, "board-flyout-StickyColors");
        cx.run_until_parked();
        assert_eq!(open(&cx), Some(Flyout::StickyColors));
        // A colour picked chooses the tool with that colour, and the flyout closes.
        click(&mut cx, "board-flyout-item-blue");
        cx.run_until_parked();
        assert_eq!(active_tool(&view, &cx), ToolId::Sticky);
        assert_eq!(sticky_colour(&cx).as_deref(), Some("blue"));
        assert_eq!(open(&cx), None);
        assert!(cx.debug_bounds("board-flyout").is_none());
        // A list of labels works the same way.
        click(&mut cx, "board-flyout-Shapes");
        cx.run_until_parked();
        click(&mut cx, "board-flyout-item-Ellipse");
        cx.run_until_parked();
        assert_eq!(active_tool(&view, &cx), ToolId::BoardShape);
        let shape = view.read_with(&cx, |view, _| {
            view.board().unwrap().session.tool.board.shape
        });
        assert_eq!(shape, figcraft_doc::BoardShape::Ellipse);
        assert_eq!(open(&cx), None);
    }

    #[gpui_kit::test]
    fn the_context_bar_runs_the_options_it_shows(cx: &mut TestAppContext) {
        use sawit_board::context_bar::{Control, context_bar};

        let tmp = Scratch::new("context-options");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        // A rectangle with text, selected.
        let shape = json!({
            "op": "command", "command": "board.shape",
            "params": {"shape": "rectangle", "x": -80, "y": 0},
        });
        run_op(&view, shape, &mut cx);
        cx.run_until_parked();
        let controls = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                context_bar(&view.board().unwrap().session).map(|bar| bar.controls)
            })
        };
        let undo_steps = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| view.board().unwrap().session.undo_depth().0)
        };
        let toast = |cx: &VisualTestContext| view.read_with(cx, |view, _| view.toast.clone());
        let steps = undo_steps(&cx);
        // Its colours, shape and text size fill the bar, inside its border and padding: the bar is
        // as wide as the place it is given in the canvas takes it to be.
        let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
        let first = cx
            .debug_bounds("board-context-color-yellow")
            .expect("a colour");
        let last = cx
            .debug_bounds("board-context-size")
            .expect("the text size");
        assert_eq!(first.left(), bar.left() + px(7.));
        assert_eq!(last.right(), bar.right() - px(7.));

        // A swatch sets the selection's colour.
        click(&mut cx, "board-context-color-blue");
        cx.run_until_parked();
        let colours = controls(&cx).and_then(|controls| controls.into_iter().next());
        assert!(
            matches!(
                colours,
                Some(Control::Colors {
                    current: Some("blue"),
                    ..
                })
            ),
            "{colours:?}"
        );
        assert_eq!(undo_steps(&cx), steps + 1);

        // The shape it has already, picked from its menu, does nothing: no command the engine
        // would refuse, and no toast (ruling P3-C2).
        click(&mut cx, "board-context-shape");
        cx.run_until_parked();
        click(&mut cx, "board-context-shape-Rectangle");
        cx.run_until_parked();
        assert_eq!(toast(&cx), None);
        assert_eq!(undo_steps(&cx), steps + 1);
        // Another one changes it.
        click(&mut cx, "board-context-shape");
        cx.run_until_parked();
        click(&mut cx, "board-context-shape-Ellipse");
        cx.run_until_parked();
        assert_eq!(toast(&cx), None);
        let shape = controls(&cx).and_then(|controls| {
            controls.into_iter().find_map(|control| match control {
                Control::Shape { current } => current,
                _ => None,
            })
        });
        assert_eq!(shape, Some(figcraft_doc::BoardShape::Ellipse));
        assert_eq!(undo_steps(&cx), steps + 2);

        // A connector's bar, the widest, fits its controls the same way.
        let connector = json!({
            "op": "command", "command": "board.connector",
            "params": {"from": {"x": -200, "y": 300}, "to": {"x": 200, "y": 300}},
        });
        run_op(&view, connector, &mut cx);
        cx.run_until_parked();
        let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
        let first = cx
            .debug_bounds("board-context-color-yellow")
            .expect("a colour");
        let last = cx.debug_bounds("board-context-end").expect("the end cap");
        assert!(cx.debug_bounds("board-context-routing").is_some());
        assert_eq!(first.left(), bar.left() + px(7.));
        assert_eq!(last.right(), bar.right() - px(7.));
    }

    #[gpui_kit::test]
    fn the_top_bar_buttons_run_their_commands(cx: &mut TestAppContext) {
        let tmp = Scratch::new("top-bar");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let zoom = |cx: &VisualTestContext| view_state(&view, cx).zoom;
        assert_eq!(zoom(&cx), 1.0);
        // The zoom menu.
        click(&mut cx, "board-zoom");
        cx.run_until_parked();
        click(&mut cx, "board-zoom-view.zoom200");
        cx.run_until_parked();
        assert_eq!(zoom(&cx), 2.0);
        // Maximize asks the workspace to fill the window with the canvas.
        let asked = Rc::new(Cell::new(0));
        cx.update(|_, cx| {
            let asked = asked.clone();
            cx.subscribe(&view, move |_, _: &super::ToggleCanvasMaximize, _| {
                asked.set(asked.get() + 1)
            })
            .detach();
        });
        click(&mut cx, "board-maximize");
        assert_eq!(asked.get(), 1);
        // Hide UI hides the chrome, itself with it.
        click(&mut cx, "board-hide-ui");
        cx.run_until_parked();
        let hidden = view.read_with(&cx, |view, _| {
            view.board().unwrap().session.ds().unwrap().view.ui_hidden
        });
        assert!(hidden);
        assert!(cx.debug_bounds("board-top-bar").is_none());
    }

    #[gpui_kit::test]
    fn choose_carries_out_what_its_command_asks(cx: &mut TestAppContext) {
        let tmp = Scratch::new("choose");
        let view = open(cx, &tmp.0.join("canvas.figcraft"));
        // A sticky, selected, then ⇧⌘↩'s command with typing to go on in the next one: the
        // engine asks the view to start it, which `choose` carries out at once.
        run_op(&view, sticky_op(), cx);
        view.update(cx, |view, cx| {
            view.choose("board.nextSticky", json!({"edit": true}), cx);
        });
        let [_, next] = layers(&view, cx)[..] else {
            panic!("layers {:?}", layers(&view, cx))
        };
        view.read_with(cx, |view, _| {
            let board = view.board().unwrap();
            let ds = board.session.ds().unwrap();
            let text = figcraft_engine::board::text_of(&ds.doc, next).unwrap();
            assert_eq!(ds.text_edit.as_ref().map(|edit| edit.id), Some(text));
            assert!(board.session.ui_requests.is_empty());
            assert_eq!(view.toast, None);
        });
        // A command the engine refuses says why, in a toast.
        view.update(cx, |view, cx| view.choose("no.such", json!({}), cx));
        assert!(view.read_with(cx, |view, _| view.toast.is_some()));
    }

    #[gpui_kit::test]
    fn the_context_bar_floats_over_the_selection_inside_the_canvas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("context-bar");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        assert!(cx.debug_bounds("board-context-bar").is_none());
        // A sticky that an op makes is selected. Its top is on page point (0, 0).
        let sticky = json!({
            "op": "command", "command": "board.sticky", "params": {"x": -100, "y": 0},
        });
        let made = run_op(&view, sticky, &mut cx);
        cx.run_until_parked();
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        // Where page point (x, y) is in the window.
        let on_screen = |cx: &VisualTestContext, (x, y): (f64, f64)| {
            let (x, y) = view_state(&view, cx).page_to_screen(x, y);
            point(
                canvas.origin.x + px(x as f32),
                canvas.origin.y + px(y as f32),
            )
        };
        let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
        // Its bottom 12 px over the sticky's top, its middle over the sticky's.
        let top = on_screen(&cx, (0.0, 0.0));
        assert_eq!(bar.bottom(), top.y - px(12.));
        assert_eq!(bar.center().x, top.x);

        // With the sticky's top 20 px under the canvas's, the bar stays in the canvas, 8 px from
        // its edge.
        let pan = json!({
            "op": "command", "command": "view.set",
            "params": {"cx": 0, "cy": f64::from(canvas.size.height) / 2.0 - 20.0},
        });
        run_op(&view, pan, &mut cx);
        cx.run_until_parked();
        assert_eq!(on_screen(&cx, (0.0, 0.0)).y, canvas.top() + px(20.));
        let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
        assert_eq!(bar.top(), canvas.top() + px(8.));
        assert!(bar.left() >= canvas.left() + px(8.) && bar.right() <= canvas.right() - px(8.));

        // A drag hides it until the drag ends.
        let (left, none) = (MouseButton::Left, Modifiers::default());
        let middle = on_screen(&cx, (0.0, 100.0));
        cx.simulate_mouse_down(middle, left, none);
        cx.simulate_mouse_move(middle + point(px(20.), px(10.)), left, none);
        assert!(cx.debug_bounds("board-context-bar").is_none());
        cx.simulate_mouse_up(middle + point(px(20.), px(10.)), left, none);
        assert!(cx.debug_bounds("board-context-bar").is_some());

        // So does typing in the sticky.
        type_in(&view, &made, &mut cx);
        cx.run_until_parked();
        assert!(cx.debug_bounds("board-context-bar").is_none());
    }

    #[gpui_kit::test]
    fn the_toast_sits_over_the_toolbar(cx: &mut TestAppContext) {
        let tmp = Scratch::new("toast-place");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        view.update(&mut cx, |view, cx| view.choose("no.such", json!({}), cx));
        cx.run_until_parked();
        let toast = cx.debug_bounds("board-toast").expect("a toast");
        let toolbar = cx.debug_bounds("board-toolbar").expect("the toolbar");
        assert_eq!(toast.bottom(), toolbar.top() - px(8.));
        // With the UI hidden, it is at the bottom of the canvas, where the toolbar was.
        view.update(&mut cx, |view, cx| {
            view.choose("view.toggleUi", json!({}), cx)
        });
        cx.run_until_parked();
        let toast = cx.debug_bounds("board-toast").expect("a toast");
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        assert_eq!(toast.bottom(), canvas.bottom() - px(16.));
    }

    #[gpui_kit::test]
    fn the_context_bar_stays_over_the_toolbar(cx: &mut TestAppContext) {
        let tmp = Scratch::new("context-bar-toolbar");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        // A sticky that an op makes is selected. Its top is on page point (0, 0).
        let sticky = json!({
            "op": "command", "command": "board.sticky", "params": {"x": -100, "y": 0},
        });
        run_op(&view, sticky, &mut cx);
        // The view moves until the sticky is below the canvas, where the bar would follow it to
        // the canvas's bottom edge, under the toolbar.
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        let pan = json!({
            "op": "command", "command": "view.set",
            "params": {"cx": 0, "cy": -f64::from(canvas.size.height)},
        });
        run_op(&view, pan, &mut cx);
        cx.run_until_parked();
        let bar = cx.debug_bounds("board-context-bar").expect("a context bar");
        let toolbar = cx.debug_bounds("board-toolbar").expect("the toolbar");
        // It stays in sight, 8 px over the toolbar, as the toast does.
        assert_eq!(bar.bottom(), toolbar.top() - px(8.));
        assert_eq!(bar.center().x, toolbar.center().x);
    }

    #[gpui_kit::test]
    fn the_toolbar_scrolls_inside_a_narrow_canvas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("toolbar-narrow");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let drawn = |cx: &mut VisualTestContext, selector: &'static str| {
            let drawn = cx.debug_bounds(selector);
            drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"))
        };
        let resize = |cx: &mut VisualTestContext, width: f32| {
            cx.simulate_resize(size(px(width), px(600.)));
            view.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
            view.read_with(&*cx, |view, _| view.bounds)
        };
        // Its whole width shows: the first and the last buttons just inside its border and
        // padding, as the context bar's controls are, and in the middle of its height.
        let whole = |cx: &mut VisualTestContext| {
            let toolbar = drawn(cx, "board-toolbar");
            let (first, last) = (drawn(cx, "tool.move"), drawn(cx, "board-picker"));
            assert_eq!(first.left(), toolbar.left() + px(7.));
            assert_eq!(last.right(), toolbar.right() - px(7.));
            assert_eq!(first.center().y, toolbar.center().y);
            toolbar
        };
        // The 800 × 600 window has room for all of it, centred in the canvas.
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        let toolbar = whole(&mut cx);
        assert_eq!(toolbar.center().x, canvas.center().x);
        assert!(toolbar.size.width < canvas.size.width - px(16.));

        // A canvas narrower than the toolbar: it is as wide as the canvas less 8 px at each side,
        // and its buttons keep their size, the last ones past its right edge.
        let canvas = resize(&mut cx, 400.);
        let inside = |bounds: Bounds<Pixels>| {
            bounds.left() >= canvas.left() + px(8.) && bounds.right() <= canvas.right() - px(8.)
        };
        let toolbar = drawn(&mut cx, "board-toolbar");
        assert_eq!(toolbar.left(), canvas.left() + px(8.));
        assert_eq!(toolbar.right(), canvas.right() - px(8.));
        assert!(inside(drawn(&mut cx, "tool.move")));
        assert!(!inside(drawn(&mut cx, "board-picker")));
        // The wheel over it scrolls them sideways, and the picker's button comes into sight. The
        // toolbar takes the scroll: the canvas under it doesn't pan.
        let before = view_state(&view, &cx);
        cx.simulate_event(ScrollEvent {
            position: toolbar.center(),
            delta: ScrollDelta::Lines(point(0., -50.)),
            ..ScrollEvent::default()
        });
        cx.run_until_parked();
        let picker = drawn(&mut cx, "board-picker");
        assert!(inside(picker), "{picker:?} in {canvas:?}");
        // Scrolled to its end, it sits where it does in a toolbar of full width.
        assert_eq!(picker.right(), toolbar.right() - px(7.));
        assert!(!inside(drawn(&mut cx, "tool.move")));
        let after = view_state(&view, &cx);
        assert_eq!(
            (after.cx, after.cy, after.zoom),
            (before.cx, before.cy, before.zoom)
        );

        // With room again, it shows its whole width once more, from its first button.
        resize(&mut cx, 800.);
        whole(&mut cx);
    }

    #[gpui_kit::test]
    fn the_context_bar_scrolls_inside_a_narrow_canvas(cx: &mut TestAppContext) {
        use sawit_board::context_bar::{Control, context_bar};

        let tmp = Scratch::new("context-bar-narrow");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        let drawn = |cx: &mut VisualTestContext, selector: &'static str| {
            let drawn = cx.debug_bounds(selector);
            drawn.unwrap_or_else(|| panic!("{selector} isn't drawn"))
        };
        let resize = |cx: &mut VisualTestContext, width: f32| {
            cx.simulate_resize(size(px(width), px(600.)));
            view.update(cx, |_, cx| cx.notify());
            cx.run_until_parked();
            view.read_with(&*cx, |view, _| view.bounds)
        };
        // Its whole width shows: the first and the last controls just inside its border and
        // padding.
        let whole = |cx: &mut VisualTestContext| {
            let bar = drawn(cx, "board-context-bar");
            let first = drawn(cx, "board-context-color-yellow");
            let last = drawn(cx, "board-context-end");
            assert_eq!(first.left(), bar.left() + px(7.));
            assert_eq!(last.right(), bar.right() - px(7.));
            bar
        };
        // A connector across the middle of the canvas, selected. Its bar is the widest: its
        // colours, how it runs and both its caps.
        let connector = json!({
            "op": "command", "command": "board.connector",
            "params": {"from": {"x": -100, "y": 0}, "to": {"x": 100, "y": 0}},
        });
        run_op(&view, connector, &mut cx);
        cx.run_until_parked();
        // The 800 px canvas has room for all of it.
        let canvas = view.read_with(&cx, |view, _| view.bounds);
        let bar = whole(&mut cx);
        assert!(bar.size.width < canvas.size.width - px(16.));

        // A 400 px canvas, narrower than the bar: the bar is as wide as the canvas less 8 px at
        // each side, and its controls keep their size, the end cap's menu past its right edge.
        let canvas = resize(&mut cx, 400.);
        assert_eq!(canvas.size.width, px(400.));
        let inside = |bounds: Bounds<Pixels>| {
            bounds.left() >= canvas.left() + px(8.) && bounds.right() <= canvas.right() - px(8.)
        };
        let bar = drawn(&mut cx, "board-context-bar");
        assert_eq!(bar.left(), canvas.left() + px(8.));
        assert_eq!(bar.right(), canvas.right() - px(8.));
        let first = drawn(&mut cx, "board-context-color-yellow");
        assert_eq!(first.left(), bar.left() + px(7.));
        assert!(!inside(drawn(&mut cx, "board-context-end")));
        // The wheel over it scrolls them sideways, and the end cap's menu comes into sight. The
        // bar takes the scroll: the canvas under it doesn't pan.
        let before = view_state(&view, &cx);
        cx.simulate_event(ScrollEvent {
            position: bar.center(),
            delta: ScrollDelta::Lines(point(0., -50.)),
            ..ScrollEvent::default()
        });
        cx.run_until_parked();
        let end = drawn(&mut cx, "board-context-end");
        assert!(inside(end), "{end:?} in {canvas:?}");
        // Scrolled to its end, it sits where it does in a bar of full width.
        assert_eq!(end.right(), bar.right() - px(7.));
        assert!(!inside(drawn(&mut cx, "board-context-color-yellow")));
        let after = view_state(&view, &cx);
        assert_eq!(
            (after.cx, after.cy, after.zoom),
            (before.cx, before.cy, before.zoom)
        );
        // Its menu opens there, and the cap picked from it is the connector's.
        let end_cap = |cx: &VisualTestContext| {
            view.read_with(cx, |view, _| {
                let bar = context_bar(&view.board().unwrap().session)?;
                bar.controls.into_iter().find_map(|control| match control {
                    Control::EndCap { current } => current,
                    _ => None,
                })
            })
        };
        assert_eq!(end_cap(&cx).as_deref(), Some("triangleArrow"));
        click(&mut cx, "board-context-end");
        cx.run_until_parked();
        click(&mut cx, "board-context-end-Circle");
        cx.run_until_parked();
        assert_eq!(end_cap(&cx).as_deref(), Some("circleFilled"));

        // With room again, it shows its whole width once more, from its first control.
        resize(&mut cx, 800.);
        whole(&mut cx);
    }

    #[gpui_kit::test]
    fn the_picker_lists_the_templates_once_and_places_one(cx: &mut TestAppContext) {
        let tmp = Scratch::new("picker");
        let (view, mut cx) = chrome_canvas(cx, &tmp);
        click(&mut cx, "board-picker");
        cx.run_until_parked();
        assert!(cx.debug_bounds("board-picker-content").is_some());
        let listed = view.read_with(&cx, |view, _| {
            view.picker.as_ref().map(|picker| picker.templates.len())
        });
        assert_eq!(listed, Some(TEMPLATES.len()));
        // The frames drawn while it is open ask the engine for nothing more: the list is the one
        // it gave when the picker opened (ruling P3-C14).
        let revision =
            |cx: &VisualTestContext| view.read_with(cx, |view, _| view.board().unwrap().revision());
        let before = revision(&cx);
        for _ in 0..3 {
            view.update(&mut cx, |_, cx| cx.notify());
            cx.run_until_parked();
        }
        assert_eq!(revision(&cx), before);
        // A template picked is placed, and the picker closes.
        view.update(&mut cx, |view, cx| view.place_template(TEMPLATES[0].id, cx));
        cx.run_until_parked();
        assert!(!layers(&view, &cx).is_empty());
        assert!(view.read_with(&cx, |view, _| view.picker.is_none()));
        assert!(cx.debug_bounds("board-picker-content").is_none());
        // And so does a table.
        let placed = layers(&view, &cx).len();
        click(&mut cx, "board-picker");
        cx.run_until_parked();
        view.update(&mut cx, |view, cx| view.place_table(2, 3, cx));
        cx.run_until_parked();
        assert_eq!(layers(&view, &cx).len(), placed + 1);
        assert!(view.read_with(&cx, |view, _| view.picker.is_none()));
    }

    #[test]
    fn every_icon_of_the_chrome_is_embedded() {
        let tools = [DocKind::Board, DocKind::Design]
            .into_iter()
            .flat_map(sawit_board::toolbar::groups)
            .flat_map(|group| group.tools)
            .map(|tool| toolbar::tool_icon(tool.icon));
        let chrome = [
            toolbar::CHEVRON_ICON,
            toolbar::PICKER_ICON,
            toolbar::CHECK_ICON,
            top_bar::HIDE_UI_ICON,
            top_bar::MAXIMIZE_ICON,
            top_bar::MAIN_MENU_ICON,
            menus::SEARCH_ICON,
        ];
        for icon in tools.chain(chrome) {
            let path = icon.path();
            let svg = crate::assets::AppAssets.load(&path).unwrap();
            assert!(svg.is_some(), "{path} isn't embedded");
        }
    }

    #[test]
    fn the_top_bar_names_the_canvas_and_shows_its_zoom() {
        let name = |path: &str| top_bar::document_name(Path::new(path));
        // The canvas of a project, and the one of a window without a project.
        assert_eq!(name("/p/.nyawit/canvas.figcraft").as_ref(), "Canvas");
        assert_eq!(name("/sawit/canvas.figcraft").as_ref(), "Canvas");
        assert_eq!(
            name("/p/design/ridgeline-app.figcraft").as_ref(),
            "ridgeline-app.figcraft"
        );
        assert_eq!(top_bar::zoom_label(1.0), "100%");
        assert_eq!(top_bar::zoom_label(0.5), "50%");
        assert_eq!(top_bar::zoom_label(1.234), "123%");
    }
}
