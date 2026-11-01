//! The canvas view's tests, by topic, and the helpers they share with each other and with the
//! view's other test modules (`commands`, `menus`, the panels').

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
use figcraft_engine::{ToolId, ViewState};
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{
    App, AppContext as _, AssetSource as _, Bounds, Context, Entity, EntityInputHandler as _,
    FocusHandle, KeyBinding, KeyBindingContextPredicate, KeyContext, KeyDownEvent, KeyUpEvent,
    Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent, MouseUpEvent,
    OperatingSystem, PinchEvent, Pixels, PlatformInput, RenderImage, ScrollDelta, ScrollEvent,
    TestAppContext, Unbind, VisualTestContext, Window, WindowHandle, point, px, size,
};
use sawit_board::overlay::{self, Composition, Shape};
use sawit_board::policy::SAWIT_WINS;
use sawit_board::toolbar::Flyout;
use sawit_board::{BoardDoc, DocKind, shortcuts, store};
use serde_json::{Value, json};

use super::open::{SystemFonts, issues_line};
use super::{
    BoardView, Command, SAVE_DELAY, TOAST_TIME, design_panel, init_keys, layers_panel, menus,
    toolbar, top_bar,
};

mod backups;
mod chrome;
mod clipboard;
mod context_bar;
mod frames;
mod guards;
mod hints;
mod keymap;
mod keys;
mod opening;
mod ops;
mod panning;
mod pointer;
mod renames;
mod render_jobs;
mod saving;
mod typing;

/// A folder of the test's own, removed once the test has ended, after its views have closed
/// ([`crate::testing::scratch_dir`]): a closing canvas saves into it. The view's other test
/// modules (`menus`) share this one and the helpers below.
pub(super) struct Scratch(pub(super) PathBuf);

impl Scratch {
    pub(super) fn new(test: &str) -> Self {
        Self(crate::testing::scratch_dir(&format!("board-view-{test}")))
    }
}

/// The canvas of `path`, opened as the workspace opens it.
fn open(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
    // The view's render thread is a real one: it wakes the frame loop when it sends a frame
    // or ends, which the deterministic scheduler otherwise counts as a bug.
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    cx.new(|cx| BoardView::open(path.to_path_buf(), cx))
}

/// The names in `dir`, sorted.
pub(super) fn names(dir: &Path) -> Vec<String> {
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
pub(super) fn saved_layers(path: &Path) -> usize {
    let doc = store::open(path).unwrap().doc().unwrap();
    let page = doc.first_page().unwrap();
    doc.node(page).unwrap().children.len()
}

/// Sends `op` to the canvas as the control socket does, and returns its answer.
pub(super) fn run_op(view: &Entity<BoardView>, op: Value, cx: &mut TestAppContext) -> Value {
    view.update(cx, |view, cx| view.run_op(&op, cx))
}

/// The op that puts a sticky note on the board.
pub(super) fn sticky_op() -> Value {
    json!({"op": "command", "command": "board.sticky", "params": {"x": 0, "y": 0}})
}

pub(super) fn view_state(view: &Entity<BoardView>, cx: &TestAppContext) -> ViewState {
    view.read_with(cx, |view, _| {
        view.board().unwrap().session.ds().unwrap().view.clone()
    })
}

/// The canvas of `path` in a 400 × 300 window of its own, drawn as the app draws it.
pub(super) fn open_in_window(
    cx: &mut TestAppContext,
    path: &Path,
) -> (WindowHandle<BoardView>, Entity<BoardView>) {
    cx.executor().allow_parking();
    let path = path.to_path_buf();
    let window = cx.open_window(size(px(400.), px(300.)), |_, cx| BoardView::open(path, cx));
    let view = window.root(cx).unwrap();
    (window, view)
}

/// The design file at `path` in a window of its own, drawn as the app draws it: as tall as
/// [`open_in_window`]'s, and as much wider as a design file's two panels are (spec D11), so
/// that its canvas is 400 wide too.
fn open_design_in_window(
    cx: &mut TestAppContext,
    path: &Path,
) -> (WindowHandle<BoardView>, Entity<BoardView>) {
    cx.executor().allow_parking();
    let path = path.to_path_buf();
    let width = 400. + layers_panel::WIDTH + design_panel::WIDTH;
    let window = cx.open_window(size(px(width), px(300.)), |_, cx| BoardView::open(path, cx));
    let view = window.root(cx).unwrap();
    (window, view)
}

/// Runs the app until the canvas has painted a frame whose image isn't `old`, and returns
/// that image. Fails when the render thread sends none for a while.
pub(super) fn next_frame(
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

/// An old canvas as Excalidraw saved it (sawit-board's import fixture): nine layers on the
/// page, and an embed that can't be imported.
const FIXTURE: &str = include_str!("../../../../board/tests/fixtures/excalidraw/basic.excalidraw");

/// The `.excalidraw` file at `path`, imported as the workspace opens it.
fn import(cx: &mut TestAppContext, path: &Path) -> Entity<BoardView> {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    cx.new(|cx| BoardView::import(path.to_path_buf(), cx))
}

/// Writes `board` to `path` as its canvas file, and returns what was written.
fn write_board(path: &Path, board: &BoardDoc) -> Vec<u8> {
    let bytes = board.to_bytes().unwrap();
    std::fs::write(path, &bytes).unwrap();
    bytes
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

/// A new, empty canvas in a 400 × 300 window, painted once: under the top bar, at 100%, with
/// page point (0, 0) in the middle of the canvas, (200, 168) in the window. What simulates
/// input in that window, and the app.
fn painted_canvas(
    cx: &mut TestAppContext,
    tmp: &Scratch,
) -> (Entity<BoardView>, VisualTestContext) {
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    let (window, view) = open_in_window(cx, &tmp.0.join("canvas.sawit"));
    next_frame(cx, &view, None);
    (view, VisualTestContext::from_window(window.into(), cx))
}

/// The top-level layers of the canvas's page.
pub(super) fn layers(view: &Entity<BoardView>, cx: &TestAppContext) -> Vec<figcraft_doc::NodeId> {
    view.read_with(cx, |view, _| {
        let doc = view.board().unwrap().doc().unwrap();
        doc.node(doc.first_page().unwrap())
            .unwrap()
            .children
            .clone()
    })
}

/// The workspace hides the canvas's tab, or shows it again, as its render does for each
/// canvas it has.
fn show(view: &Entity<BoardView>, shown: bool, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let home = cx.focus_handle();
        view.update(cx, |view, cx| view.set_shown(shown, &home, window, cx));
    });
    cx.run_until_parked();
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

/// A drag of `button` from (100, 100) to (160, 140) in the window, which the canvas fills.
fn drag(cx: &mut VisualTestContext, button: MouseButton) {
    let none = Modifiers::default();
    cx.simulate_mouse_down(point(px(100.), px(100.)), button, none);
    cx.simulate_mouse_move(point(px(130.), px(120.)), button, none);
    cx.simulate_mouse_move(point(px(160.), px(140.)), button, none);
    cx.simulate_mouse_up(point(px(160.), px(140.)), button, none);
}

/// The tests that copy layers, or paste the app's copy, share that copy
/// (`sawit_board::clipboard`): they take turns.
static CLIPBOARD_TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn clipboard_turn() -> std::sync::MutexGuard<'static, ()> {
    (CLIPBOARD_TURN.lock()).unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// A new, empty canvas in an 800 × 600 window, wide enough for the whole toolbar, painted
/// once: under the top bar, at 100%, with page point (0, 0) in the middle of the canvas.
pub(super) fn chrome_canvas(
    cx: &mut TestAppContext,
    tmp: &Scratch,
) -> (Entity<BoardView>, VisualTestContext) {
    chrome_canvas_at(cx, &tmp.0.join("canvas.sawit"))
}

/// [`chrome_canvas`], of the canvas file at `path`.
pub(super) fn chrome_canvas_at(
    cx: &mut TestAppContext,
    path: &Path,
) -> (Entity<BoardView>, VisualTestContext) {
    cx.update(gpui_kit::init);
    // The canvas's shortcuts are key bindings.
    cx.update(init_keys);
    cx.executor().allow_parking();
    let path = path.to_path_buf();
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
