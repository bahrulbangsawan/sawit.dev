//! The native canvas tab: a FigCraft document, rendered off the UI thread and painted by GPUI.
//! Only the GPUI side lives here; the document logic is `sawit_board` (`crates/board`).
//!
//! [`BoardView`] shows one canvas file. Each frame it asks the render thread ([`worker`]) for the
//! view when that changed, and paints the newest frame it has, moved and scaled to the view, so
//! pans and zooms show at once and the render catches up ([`paint`]). Over it go the selection,
//! handles, names and previews `sawit_board::overlay` lists, as the view is now
//! ([`overlay_paint`]). Edits are saved once the canvas has been still for a moment, and when the
//! tab closes or the app quits.

mod overlay_paint;
mod paint;
mod worker;

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::color::Color;
use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use figcraft_render::{Options, View};
use futures::StreamExt as _;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_board::store::{self, Writer};
use sawit_board::view::{self, PREVIEW_QUALITY, Viewport};
use sawit_board::{BoardDoc, BoardError, DocKind, ops};
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

/// Starts what every canvas shares, once at startup. The system fonts are read on a background
/// thread, and then every window repaints: text laid out with FigCraft's bundled fonts meanwhile
/// is laid out again ([`figcraft_text::layout_generation`] has changed).
pub fn init(cx: &mut App) {
    let scan = cx.background_spawn(async { figcraft_text::scan_system_fonts() });
    cx.spawn(async move |cx| {
        scan.await;
        cx.update(|cx| cx.refresh_windows());
    })
    .detach();
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
    ///   was kept as `<path>.bak` before the file opened.
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
    /// Then the canvas redraws, and saves whatever the op changed: an op arrives with no input in
    /// the window, perhaps while it is covered and its display link has stopped, so the save
    /// can't wait for the next paint.
    pub fn run_op(&mut self, op: &Value, cx: &mut Context<Self>) -> Value {
        let reply = match self.board_mut() {
            Some(board) => ops::run(board, op),
            None => self.not_opened(),
        };
        self.autosave(cx);
        cx.notify();
        reply
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

    /// A frame from the render thread replaces the one shown.
    fn show_frame(&mut self, frame: Frame, cx: &mut Context<Self>) {
        // Which request the frame answers, to follow a stutter from its request to its paint.
        log::trace!("canvas frame of request {:016x}", frame.key);
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
                    page,
                    view: wanted,
                    opts,
                    page_rgb,
                    key,
                });
                self.requested = Some((key, doc));
            }
        }
        let frame = self.frame.as_ref().map(|frame| {
            let at = paint::image_bounds(bounds, &frame.view, &ds.view, scale_factor);
            (frame.image.clone(), at)
        });
        // What goes over the frame, its labels measured. The frames' names go back to the engine
        // as they are painted: it takes a press or a hover on one as on its frame.
        let shapes = sawit_board::overlay::overlay(&board.session);
        let view = &board.session.ds().ok()?.view;
        let font = &cx.theme().font_family;
        let (overlay, names) = overlay_paint::layout(shapes, font, view, window);
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
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Err(error) = &self.board {
            return Placeholder::error("The canvas couldn't be opened.")
                .detail(error.clone())
                .into_any_element();
        }
        let this = cx.entity().downgrade();
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
            .relative()
            .size_full()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| this.on_scroll(event, cx)))
            .on_pinch(cx.listener(|this, event: &PinchEvent, _, cx| this.on_pinch(event, cx)))
            .child(
                canvas(
                    move |bounds, window, cx| {
                        this.update(cx, |this, cx| this.prepaint(bounds, window, cx))
                            .ok()
                            .flatten()
                    },
                    |bounds, painting, window, cx| {
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
    use gpui_kit::component::{Theme, ThemeMode};
    use gpui_kit::{
        AppContext as _, Entity, Modifiers, PinchEvent, RenderImage, ScrollDelta, ScrollEvent,
        TestAppContext, WindowHandle, point, px, size,
    };
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
}
