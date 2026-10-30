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
//! [`BoardView::on_key`]).

mod events;
mod overlay_paint;
mod paint;
mod worker;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::color::Color;
use figcraft_doc::{Document, NodeId};
use figcraft_engine::{UiRequest, ViewState};
use figcraft_render::{Options, View};
use futures::StreamExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::input::KeyOutcome;
use sawit_board::store::{self, Writer};
use sawit_board::view::{self, PREVIEW_QUALITY, Viewport};
use sawit_board::{BoardDoc, BoardError, DocKind, input, ops};
use serde_json::{Value, json};

use crate::canvas::ToggleCanvasMaximize;
use crate::states::Placeholder;
use paint::Painting;
use worker::{Frame, Job, Worker};

/// How long the canvas must be still after an edit before the edit is saved.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// How long after the last step of a zoom gesture frames are rendered at full quality again.
const ZOOM_SETTLE: Duration = Duration::from_millis(150);
/// How long a toast stays up.
const TOAST_TIME: Duration = Duration::from_secs(3);
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
/// and saved as it is edited.
pub struct BoardView {
    path: PathBuf,
    /// The document, or why the file couldn't be opened: the tab then says so, and never saves.
    board: Result<BoardDoc, SharedString>,
    focus: FocusHandle,
    worker: Worker,
    /// The newest render request, and its document. The document is kept so that no other one
    /// can take its address, which the request's key holds.
    requested: Option<(u64, Arc<Document>)>,
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
    /// The document as loaded or last saved. `None` when the last save failed: the file holds
    /// something older.
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
    _frames: Task<()>,
    _quit: Subscription,
    _release: Subscription,
}

impl EventEmitter<ToggleCanvasMaximize> for BoardView {}

impl BoardView {
    /// Opens the canvas file at `path` ([`store::open_with_issues`]).
    ///
    /// - A missing file is a new, empty board. Nothing is written until there is an edit.
    /// - A file that can't be read is shown as an error, and stays as it is: this view never saves.
    /// - What loading repaired or left out, or saving would drop, shows as a toast. The original
    ///   was kept as `<path>.lossy.bak` before the file opened.
    pub fn open(path: PathBuf, cx: &mut Context<Self>) -> Self {
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
        let (board, toast) = match store::open_with_issues(&path) {
            Ok((mut board, issues)) => {
                // As each frame does. Done before the document counts as saved: the blank page
                // of a file nothing was done to takes the dark theme's colour (as FigCraft's new
                // files do), which is no edit to save.
                board.session.set_ui_dark(cx.theme().mode.is_dark());
                board.session.apply_view_prefs();
                (Ok(board), issues_line(&path, &issues))
            }
            Err(err) => (Err(err.to_string().into()), None),
        };
        let saved = board.as_ref().ok().and_then(|board| board.doc().ok());
        Self {
            path,
            board,
            focus: cx.focus_handle(),
            worker,
            requested: None,
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
            writer: Arc::new(Writer::spawn()),
            saved,
            pending: None,
            save_later: None,
            toast_timer: toast.is_some().then(|| hide_toast_later(cx)),
            toast: toast.map(SharedString::from),
            _frames: frames,
            // ⌘Q quits without dropping the views: what isn't on disk yet is written first.
            _quit: cx.on_app_quit(|this, _| {
                this.flush();
                async {}
            }),
            _release: cx.on_release(|this, cx| this.release_images(cx)),
        }
    }

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
    pub fn run_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        let reply = match self.board_mut() {
            Some(board) => ops::run(board, op),
            None => self.not_opened(),
        };
        self.handle_ui_requests(Vec::new(), cx);
        self.autosave(cx);
        cx.notify();
        reply
    }

    /// Carries out what the engine asked of the canvas: `requests`, which an input already took
    /// out of the session, then any still waiting in `session.ui_requests`. It runs after every
    /// pointer event, key and op, as FigCraft's own canvas handles its requests after each input.
    ///
    /// `sawit_board::input::handle_requests` does the ones that need no view (`board.editText`
    /// starts typing in a sticky). The panels' and menus' requests get their arms here with Plans
    /// 4, 5 and 6. A message the engine left for the user shows as a toast.
    fn handle_ui_requests(&mut self, mut requests: Vec<UiRequest>, cx: &mut Context<Self>) {
        let Some(board) = self.board_mut() else {
            return;
        };
        requests.append(&mut board.session.ui_requests);
        let rest = input::handle_requests(board, &requests);
        let message = board.session.message.take();
        for request in rest {
            log::debug!("the canvas has no handler yet for request {}", request.kind);
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
    pub(super) fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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

    /// Carries out `id`, a command Sawit does itself instead of the engine
    /// (`sawit_board::policy::UI_COMMANDS`): import, export, save and copy as. None is done yet,
    /// so each says it is coming.
    fn run_ui_command(&mut self, id: &'static str, cx: &mut Context<Self>) {
        log::debug!("the canvas doesn't carry out {id} yet");
        self.show_toast("Coming soon", cx);
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
    /// When that fails the error is logged, and the next flush or edit tries again; the writer's
    /// [`Writer::last_error`] still says why.
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
    /// over, if any.
    fn queue_save(&mut self) -> Result<Option<Arc<Document>>, BoardError> {
        let Ok(board) = &self.board else {
            return Ok(None);
        };
        let doc = board.doc()?;
        if same(&self.saved, &doc) {
            return Ok(None);
        }
        let bytes = board.to_bytes()?;
        self.writer
            .save(self.path.clone(), bytes, store::is_empty(&doc));
        self.saved = Some(doc.clone());
        self.pending = Some(doc.clone());
        Ok(Some(doc))
    }

    /// After a frame: when the document isn't the one saved, saves it once the canvas has been
    /// still for [`SAVE_DELAY`]. Every new document (each edit makes one) starts the wait over,
    /// so a drag is saved once it ends. A failed save shows its error as a toast.
    fn autosave(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
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
    /// will be painted, and starts the autosave after an edit. What to paint, if there is a
    /// document.
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
        let quality = if self.zooming_until.is_some_and(|until| until > now) {
            PREVIEW_QUALITY
        } else {
            1.0
        };
        let board = self.board.as_mut().ok()?;
        board.session.set_ui_dark(dark);
        board.session.apply_view_prefs();
        let kind = board.kind();
        let ds = board.session.ds_mut().ok()?;
        let (width, height) = (f64::from(bounds.size.width), f64::from(bounds.size.height));
        ds.view.vw = width;
        ds.view.vh = height;
        let (doc, page) = (ds.doc.clone(), ds.page);
        if !self.fitted && width >= 1.0 && height >= 1.0 {
            view::zoom_to_fit(&mut ds.view, &doc, page);
            self.fitted = true;
        }
        let page_rgb = view::page_rgb(&doc, page);
        // The page as the screen shows it; the render takes it as the document holds it, and
        // the worker converts its pixels the same way.
        let screen_rgb = view::screen_rgb(doc.color_profile, page_rgb);
        let viewport = Viewport {
            width,
            height,
            scale_factor,
        };
        if let Some(wanted) = view::render_view(&ds.view, viewport, quality) {
            let key = job_key(&doc, page, &wanted, ds.view.zoom, dark);
            let asked = (self.requested.as_ref())
                .is_some_and(|(asked, of)| *asked == key && Arc::ptr_eq(of, &doc));
            if !asked {
                let [r, g, b] = page_rgb;
                let opts = Options {
                    lod: true,
                    magnified_image_pixels: true,
                    background: Some(Color::from_rgba8(r, g, b, 255)),
                    dots: match kind {
                        DocKind::Board => view::dots(ds.view.zoom, screen_rgb),
                        DocKind::Design => None,
                    },
                    ..Options::default()
                };
                self.worker.request(Job {
                    doc: doc.clone(),
                    uid: ds.uid,
                    page,
                    view: wanted,
                    opts,
                    page_rgb,
                    key,
                });
                self.requested = Some((key, doc));
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
        let shapes = figcraft_engine::guard::run("ui.overlay", || {
            Ok(sawit_board::overlay::overlay(&board.session))
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
        let this = cx.entity().downgrade();
        let drags = this.clone();
        let cursor = self.cursor();
        // The keys the canvas takes while it edits text are bound in a context of their own.
        let key_context = if self.editing_text() {
            "Board BoardText"
        } else {
            "Board"
        };
        let theme = cx.theme();
        let toast = self.toast.clone().map(|text| {
            div()
                .absolute()
                .bottom_4()
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
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
                )
        });
        div()
            .id("board")
            .key_context(key_context)
            .relative()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .cursor(cursor)
            .on_key_down(cx.listener(Self::on_key))
            .on_key_up(cx.listener(Self::on_key_up))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.on_scroll(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.on_pinch(event, cx)))
            // The right button is left for the context menu.
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::on_middle_down))
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
                        events::track_drags(&drags, window);
                        if let Some(painting) = painting {
                            paint::paint(bounds, painting, window, cx);
                        }
                    },
                )
                .size_full(),
            )
            .children(toast)
            .into_any_element()
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

/// Whether `kept` is `doc`: the same document, not only an equal one.
fn same(kept: &Option<Arc<Document>>, doc: &Arc<Document>) -> bool {
    kept.as_ref().is_some_and(|kept| Arc::ptr_eq(kept, doc))
}

/// The name of a render request: the same for requests that would draw the same frame. Another
/// document (each edit makes one), page, view, set of fonts or theme makes another.
fn job_key(doc: &Arc<Document>, page: NodeId, view: &View, zoom: f64, dark: bool) -> u64 {
    let mut hasher = DefaultHasher::new();
    Arc::as_ptr(doc).hash(&mut hasher);
    page.hash(&mut hasher);
    for number in [view.origin.0, view.origin.1, view.scale, zoom] {
        number.to_bits().hash(&mut hasher);
    }
    (view.width, view.height).hash(&mut hasher);
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
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use figcraft_doc::NodeId;
    use figcraft_doc::color::Color;
    use figcraft_doc::geom::Point as PagePoint;
    use figcraft_engine::ViewState;
    use figcraft_engine::keymap::{self, KEYS, Platform};
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{
        App, AppContext as _, Entity, KeyBinding, KeyContext, KeyDownEvent, KeyUpEvent, Keystroke,
        Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseUpEvent, OperatingSystem,
        PinchEvent, PlatformInput, RenderImage, ScrollDelta, ScrollEvent, TestAppContext,
        VisualTestContext, WindowHandle, point, px, size,
    };
    use sawit_board::input::gpui_keystroke;
    use sawit_board::policy::{self, SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING};
    use sawit_board::{BoardDoc, DocKind, store};
    use serde_json::{Value, json};

    use super::{BoardView, SAVE_DELAY, TOAST_TIME, issues_line};

    /// A folder of the test's own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(test: &str) -> Self {
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
    fn run_op(view: &Entity<BoardView>, op: Value, cx: &mut TestAppContext) -> Value {
        view.update(cx, |view, cx| view.run_op(&op, cx))
    }

    /// The op that puts a sticky note on the board.
    fn sticky_op() -> Value {
        json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}})
    }

    fn view_state(view: &Entity<BoardView>, cx: &TestAppContext) -> ViewState {
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

    #[gpui_kit::test]
    fn a_lossy_open_says_what_saving_drops(cx: &mut TestAppContext) {
        let tmp = Scratch::new("lossy");
        let path = tmp.0.join("canvas.figcraft");
        // A file with a version history, which the canvas doesn't keep.
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": "kept"}))
            .unwrap();
        board
            .execute("file.saveVersion", &json!({"name": "First"}))
            .unwrap();
        board
            .execute("file.saveAs", &json!({"path": path.to_str().unwrap()}))
            .unwrap();
        let view = open(cx, &path);
        let toast = view.read_with(cx, |view, _| view.toast.clone()).unwrap();
        assert!(toast.starts_with("canvas.figcraft: "), "{toast}");
        assert!(toast.contains("version history"), "{toast}");
        cx.executor().advance_clock(TOAST_TIME);
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.toast.is_none()));
    }

    #[gpui_kit::test]
    fn frames_are_painted_and_replaced_ones_leave_the_atlas(cx: &mut TestAppContext) {
        let tmp = Scratch::new("frames");
        cx.update(gpui_kit::init);
        let (window, view) = open_in_window(cx, &tmp.0.join("canvas.figcraft"));
        let first = next_frame(cx, &view, None);
        assert!(in_atlas(cx, window, &first));
        // The engine has the canvas's size, which the first frame fitted the view to.
        let state = view_state(&view, cx);
        assert_eq!((state.vw, state.vh), (400.0, 300.0));
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
        // An op the canvas doesn't have yet is an error reply, not a panic.
        let reply = run_op(&view, json!({"op": "mermaid", "source": "graph TD"}), cx);
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

    /// A new, empty canvas in a 400 × 300 window, painted once: at 100%, with page point (0, 0)
    /// in the middle of the window, (200, 150). What simulates input in that window, and the app.
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
    fn layers(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<figcraft_doc::NodeId> {
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
        // A 200 × 200 sticky about page point (0, 0), the middle of the window, not selected.
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

        // From over the canvas to off it (it fills the window).
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
    fn focus_canvas(view: &Entity<BoardView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
    }

    /// Starts typing in the sticky that a command op made, `made` being its answer, and returns
    /// the sticky.
    fn type_in(view: &Entity<BoardView>, made: &Value, cx: &mut TestAppContext) -> NodeId {
        let note = figcraft_engine::params::id_of(&made["result"]["id"]).unwrap();
        view.update(cx, |view, _| {
            let board = view.board_mut().unwrap();
            assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
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
}
