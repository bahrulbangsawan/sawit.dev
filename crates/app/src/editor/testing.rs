//! Shared setup for the GPUI tests of file editors with unsaved changes: what quitting, Restart to
//! Update and closing a window ask about, and what a quit that can't ask keeps.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use super::FileEditor;

/// How long a test waits for a file to load or save: they go through the Tokio runtime's threads,
/// which take seconds when other test binaries run at once.
const FILES: Duration = Duration::from_secs(30);

/// Registers what an editor's window needs, and the runtime its loads and saves go through.
pub(crate) fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::workspace::init(cx);
    super::init(cx);
    cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
}

/// A fresh, empty folder under the temp dir, by its real path.
pub(crate) fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sawit-editor-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Polls `ok` (files load and save on other threads) for up to [`FILES`].
pub(crate) fn wait_until(cx: &mut TestAppContext, what: &str, ok: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + FILES;
    loop {
        cx.run_until_parked();
        if cx.update(|cx| ok(cx)) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting until {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Opens `path` in a file editor that fills a window of its own, and waits until the file has
/// loaded, when the editor takes the keyboard.
pub(crate) fn open_editor(
    cx: &mut TestAppContext,
    path: &Path,
) -> (AnyWindowHandle, Entity<FileEditor>) {
    cx.executor().allow_parking();
    let path = path.to_path_buf();
    let (window, editor) = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| FileEditor::open(path, None, window, cx))
        })
        .expect("the test window opens")
    });
    wait_until(cx, "the file has loaded", |cx| {
        editor.read(cx).state.is_some()
    });
    (window, editor)
}

/// Types `text` at the cursor of `editor`, which fills `window`: its buffer then has unsaved
/// changes.
pub(crate) fn type_into(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    editor: &Entity<FileEditor>,
    text: &str,
) {
    cx.simulate_input(window, text);
    cx.run_until_parked();
    assert!(
        cx.update(|cx| editor.read(cx).dirty),
        "typing left the buffer unchanged"
    );
}
