//! Tests of the Files panel: shared setup here, the tests by what they cover in the modules.

mod changes;
mod folders;
mod keys;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::channel::oneshot;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Entity, FocusHandle, Subscription, TestAppContext,
};
use sawit_core::files::Entry;

use super::{After, Disk, FilesEvent, FilesPanel, Op, Outcome, Request};

/// A project the tree shows without reading it, at a path that can't exist: nothing in it is
/// there, so a file change that did run would change nothing.
const ROOT: &str = "/sawit-test-no-such-folder";

/// Another such project, for the panel to change to.
const OTHER_ROOT: &str = "/sawit-test-no-such-other-folder";

/// A Files panel with its keys bound, in a window that is active, as a window being typed in
/// is (a name box that loses the keyboard ends its edit), showing [`ROOT`] with `files` in it.
///
/// No Tokio runtime is installed. A file change runs on its pool and answers after the test
/// has ended, which the test scheduler can't allow, so one a test starts fails at once
/// instead ("no state of type sawit::rt::Rt exists").
fn open_panel(cx: &mut TestAppContext, files: &[&str]) -> (AnyWindowHandle, Entity<FilesPanel>) {
    open_panel_at(cx, Path::new(ROOT), files)
}

/// [`open_panel`], showing `root`.
fn open_panel_at(
    cx: &mut TestAppContext,
    root: &Path,
    files: &[&str],
) -> (AnyWindowHandle, Entity<FilesPanel>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        super::init(cx);
    });
    let (window, panel) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| FilesPanel::new(window, cx))
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, _| window.activate_window())
        .unwrap();
    cx.update(|cx| {
        panel.update(cx, |p, cx| {
            show(p, root, files);
            cx.notify();
        })
    });
    cx.run_until_parked();
    (window, panel)
}

/// The file changes a panel starts, which the test lands when it likes, in place of the
/// runtime's, and the folders it reads, which get no answer.
#[derive(Clone, Default)]
struct Changes(Rc<RefCell<Vec<Started>>>, Rc<RefCell<Vec<Vec<PathBuf>>>>);

/// A change the panel started, and how to land it, until it has landed.
type Started = (Op, Option<oneshot::Sender<Outcome>>);

impl Changes {
    fn install(panel: &Entity<FilesPanel>, cx: &mut TestAppContext) -> Self {
        let changes = Self::default();
        let (started, read) = (changes.clone(), changes.clone());
        cx.update(|cx| {
            panel.update(cx, |p, _| {
                p.disk = Disk {
                    run: Rc::new(move |op, _| {
                        let (tx, rx) = oneshot::channel();
                        started.0.borrow_mut().push((op, Some(tx)));
                        rx
                    }),
                    list: Rc::new(move |dirs, _| {
                        read.1.borrow_mut().push(dirs);
                        oneshot::channel().1
                    }),
                }
            })
        });
        changes
    }

    /// The folders read so far, in order.
    fn read(&self) -> Vec<PathBuf> {
        self.1.borrow().concat()
    }

    /// The reads so far, each the folders it read.
    fn reads(&self) -> Vec<Vec<PathBuf>> {
        self.1.borrow().clone()
    }

    /// The changes started so far, in order.
    fn started(&self) -> Vec<Op> {
        self.0.borrow().iter().map(|(op, _)| op.clone()).collect()
    }

    /// Lands the change started `n`th (from 0) with `outcome`.
    fn land(&self, n: usize, outcome: Outcome, cx: &mut TestAppContext) {
        let tx = self.0.borrow_mut()[n]
            .1
            .take()
            .expect("a change lands once");
        tx.send(outcome).unwrap();
        cx.run_until_parked();
    }
}

/// Asks `panel` for `op`, as one of the tree's commands does.
fn ask(cx: &mut TestAppContext, window: AnyWindowHandle, panel: &Entity<FilesPanel>, op: Op) {
    cx.update_window(window, |_, window, cx| {
        panel.update(cx, |p, cx| {
            p.request(Request::Do(op, After::default()), window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
}

/// The panel's undo and redo stacks, and whether a change runs.
fn history(panel: &Entity<FilesPanel>, cx: &mut TestAppContext) -> (Vec<Op>, Vec<Op>, bool) {
    cx.update(|cx| {
        let p = panel.read(cx);
        (p.undo.clone(), p.redo.clone(), p.busy)
    })
}

/// Selects `path`, as a click on its row does, and gives the tree the keyboard.
fn select(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<FilesPanel>,
    path: &Path,
) {
    cx.update(|cx| panel.update(cx, |p, _| p.selected = Some(path.to_path_buf())));
    let tree = cx.update(|cx| panel.read(cx).focus.clone());
    focus(cx, window, &tree);
}

/// Makes `root` the panel's folder with `src/lib/x.rs` and `notes.txt` in it, every folder
/// open, as if it had been read.
fn show_tree(p: &mut FilesPanel, root: &Path) {
    let entry = |path: PathBuf, is_dir| Entry {
        name: path.file_name().unwrap().to_string_lossy().into_owned(),
        path,
        is_dir,
    };
    let (src, lib) = (root.join("src"), root.join("src/lib"));
    let notes = entry(root.join("notes.txt"), false);
    p.children
        .insert(root.to_path_buf(), vec![entry(src.clone(), true), notes]);
    p.children
        .insert(src.clone(), vec![entry(lib.clone(), true)]);
    p.children
        .insert(lib.clone(), vec![entry(lib.join("x.rs"), false)]);
    p.expanded.extend([root.to_path_buf(), src, lib]);
    p.root = Some(root.to_path_buf());
}

/// The panel's events from now on, as text.
fn events(
    panel: &Entity<FilesPanel>,
    cx: &mut TestAppContext,
) -> (Rc<RefCell<Vec<String>>>, Subscription) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let subscription = cx.update(|cx| {
        let seen = seen.clone();
        cx.subscribe(panel, move |_, event: &FilesEvent, _| {
            let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
            seen.borrow_mut().push(match event {
                FilesEvent::OpenFile(path) => format!("open {}", name(path)),
                FilesEvent::Moved { from, to } => format!("moved {} {}", name(from), name(to)),
                FilesEvent::RootChanged(path) => format!("root {}", name(path)),
                FilesEvent::OpenTerminal(path) => format!("terminal {}", name(path)),
                FilesEvent::FindInFolder(path) => format!("find {}", name(path)),
                FilesEvent::ViewHistory(path) => format!("history {}", name(path)),
                FilesEvent::OpenFolder(path) => format!("open folder {}", name(path)),
            })
        })
    });
    (seen, subscription)
}

/// What is typed in the name box being typed in, as the user types it, and ↩.
fn name_it(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    panel: &Entity<FilesPanel>,
    name: &str,
) {
    cx.update_window(window, |_, window, cx| {
        let input = panel.read(cx).edit.as_ref().unwrap().input.clone();
        input.update(cx, |s, cx| s.set_value(name.to_string(), window, cx));
    })
    .unwrap();
    cx.simulate_keystrokes(window, "enter");
}

/// The name in the name box, if one is open.
fn typed(panel: &Entity<FilesPanel>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        let p = panel.read(cx);
        p.edit
            .as_ref()
            .map(|e| e.input.read(cx).value().to_string())
    })
}

/// Makes `root` the panel's folder, with `files` in it, as if it had been read.
fn show(p: &mut FilesPanel, root: &Path, files: &[&str]) {
    let entries = files
        .iter()
        .map(|name| Entry {
            name: (*name).into(),
            path: root.join(name),
            is_dir: false,
        })
        .collect();
    p.children.insert(root.to_path_buf(), entries);
    p.expanded.insert(root.to_path_buf());
    p.root = Some(root.to_path_buf());
}

/// The names of the `files::` actions keys run from now on, in order.
fn file_actions(cx: &mut TestAppContext) -> (Rc<RefCell<Vec<&'static str>>>, Subscription) {
    let ran = Rc::new(RefCell::new(Vec::new()));
    let observer = cx.update(|cx| {
        let ran = ran.clone();
        cx.observe_keystrokes(move |e, _, _| {
            if let Some(action) = &e.action
                && action.name().starts_with("files::")
            {
                ran.borrow_mut().push(action.name());
            }
        })
    });
    (ran, observer)
}

/// Gives the keyboard to `handle`.
fn focus(cx: &mut TestAppContext, window: AnyWindowHandle, handle: &FocusHandle) {
    cx.update_window(window, |_, window, cx| window.focus(handle, cx))
        .unwrap();
    cx.run_until_parked();
}
