//! Project file tree (left panel), loaded lazily per folder.

mod changes;
mod commands;
mod naming;
pub(crate) mod ops;
mod render;
pub mod search;
mod tree;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::channel::oneshot;

use gpui_kit::component::input::InputState;
use gpui_kit::component::*;
use gpui_kit::*;
use std::time::Duration;

use sawit_core::files::{Entry, list_dir};

use crate::rt;
use ops::Op;

actions!(
    files,
    [
        NewFile,
        NewFolder,
        RevealInFinder,
        OpenInDefaultApp,
        OpenInTerminal,
        FindInFolder,
        Cut,
        CopyEntry,
        Duplicate,
        Paste,
        Undo,
        Redo,
        CopyPath,
        CopyRelativePath,
        AddToGitignore,
        AddToGitExclude,
        ViewHistory,
        Rename,
        Trash,
        TrashWithoutAsking,
        Delete,
        ExpandAll,
        CollapseAll
    ]
);

/// Zed's project panel bindings for the same commands. They act on the tree only while no name
/// is being typed, like Config Files' list keys and the keymap editor's table keys: the name box
/// passes on the keys it has no use for (F2, ⌘N, ⌘D, …), and ⌫ with nothing before the cursor (a
/// new name starts empty). F2 would start a rename over the name typed, ⌘D duplicate the
/// selected entry and ⌫ move it to the Trash.
///
/// ⌫ asks before it moves anything to the Trash, as in Zed: the tree has the keyboard after a
/// click in it, which nothing on screen says, and ⌫ meant for a text field would move a file, or
/// a whole folder, away. ⌘⌫ doesn't ask, as in Finder.
pub fn init(cx: &mut App) {
    let tree = Some("FilesPanel && !Input");
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewFile, tree),
        KeyBinding::new("alt-cmd-n", NewFolder, tree),
        KeyBinding::new("alt-cmd-r", RevealInFinder, tree),
        KeyBinding::new("ctrl-shift-enter", OpenInDefaultApp, tree),
        KeyBinding::new("alt-cmd-shift-f", FindInFolder, tree),
        KeyBinding::new("cmd-x", Cut, tree),
        KeyBinding::new("cmd-c", CopyEntry, tree),
        KeyBinding::new("cmd-d", Duplicate, tree),
        KeyBinding::new("cmd-v", Paste, tree),
        KeyBinding::new("cmd-z", Undo, tree),
        KeyBinding::new("cmd-shift-z", Redo, tree),
        KeyBinding::new("alt-cmd-c", CopyPath, tree),
        KeyBinding::new("alt-cmd-shift-c", CopyRelativePath, tree),
        KeyBinding::new("f2", Rename, tree),
        KeyBinding::new("backspace", Trash, tree),
        KeyBinding::new("cmd-backspace", TrashWithoutAsking, tree),
        KeyBinding::new("alt-cmd-backspace", Delete, tree),
    ]);
}

/// Asks before `path` goes to the Trash (⌫ in Files and Config Files): 0 is yes. `detail` says
/// how to get it back.
pub(crate) fn ask_to_trash(
    path: &Path,
    detail: &str,
    window: &mut Window,
    cx: &mut App,
) -> oneshot::Receiver<usize> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let question = if path.is_dir() {
        format!("Move the folder “{name}” and everything in it to the Trash?")
    } else {
        format!("Move “{name}” to the Trash?")
    };
    window.prompt(
        PromptLevel::Warning,
        &question,
        Some(detail),
        &["Move to Trash", "Cancel"],
        cx,
    )
}

pub enum FilesEvent {
    OpenFile(PathBuf),
    RootChanged(PathBuf),
    /// A file or folder was renamed or moved; its open tabs follow it.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    OpenTerminal(PathBuf),
    FindInFolder(PathBuf),
    ViewHistory(PathBuf),
    /// A folder picked to open as the project (the tab's Open Project Folder).
    OpenFolder(PathBuf),
}

impl EventEmitter<FilesEvent> for FilesPanel {}

/// A file or folder dragged out of the tree (the terminal takes it as a path).
#[derive(Clone)]
pub struct DraggedFile {
    pub path: PathBuf,
    name: SharedString,
}

impl Render for DraggedFile {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_0p5()
            .text_sm()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .child(self.name.clone())
    }
}

/// How often open folders are re-read, so files made in the terminal show up.
const REFRESH: Duration = Duration::from_secs(3);

#[derive(Clone)]
enum Row {
    Entry(Entry, usize),
    /// The name field of a new or renamed entry, at this depth.
    Edit(usize),
}

#[derive(Clone)]
enum EditKind {
    New { dir: bool },
    Rename(PathBuf),
}

/// A name being typed in the tree.
struct Edit {
    kind: EditKind,
    /// Folder the entry is (or will be) in.
    parent: PathBuf,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// What Cut or Copy picked, for Paste.
#[derive(Clone)]
struct Clip {
    path: PathBuf,
    cut: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Do,
    Undo,
    Redo,
}

/// A file change asked for. Changes run one at a time, in the order asked
/// ([`FilesPanel::request`]): ⌘Z pressed while a change runs undoes that change once it has
/// landed, not the one before it, and is skipped if that change fails
/// ([`FilesPanel::skip_steps_behind`]).
enum Request {
    Do(Op, After),
    /// Undoes the latest change undo holds when it starts.
    Undo,
    Redo,
}

/// What a change the user made does once it lands, besides being kept for undo.
#[derive(Default)]
struct After {
    /// The name typed for it (New File, New Folder, Rename). If the change fails, the name box
    /// comes back with it, to fix rather than to type again.
    named: Option<Named>,
    /// It pastes what was cut: Paste offers the Cut no more once it has landed.
    pastes_cut: bool,
}

/// A name typed in the tree, for what, and in which folder.
struct Named {
    kind: EditKind,
    parent: PathBuf,
    name: String,
}

/// A folder's entries, or why it couldn't be read.
type Listing = sawit_core::Result<Vec<Entry>>;

/// What a file change answers: the change that undoes it, or why it failed.
type Outcome = Result<Op, String>;

/// Runs a file change off the UI thread ([`Op::run`]).
type Run = Rc<dyn Fn(Op, &App) -> oneshot::Receiver<Outcome>>;

/// Lists folders off the UI thread ([`list_dir`]).
type List = Rc<dyn Fn(Vec<PathBuf>, &App) -> oneshot::Receiver<Vec<(PathBuf, Listing)>>>;

/// Where the panel's disk work runs: on the runtime's blocking pool, or in a test, wherever the
/// test says.
#[derive(Clone)]
struct Disk {
    run: Run,
    list: List,
}

impl Default for Disk {
    fn default() -> Self {
        Self {
            run: Rc::new(|op, cx| rt::spawn_blocking(cx, move || op.run())),
            list: Rc::new(|dirs, cx| {
                rt::spawn_blocking(cx, move || {
                    dirs.into_iter()
                        .map(|dir| {
                            let listing = list_dir(&dir);
                            (dir, listing)
                        })
                        .collect()
                })
            }),
        }
    }
}

pub struct FilesPanel {
    pub root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Entry>>,
    selected: Option<PathBuf>,
    /// The tree rows, flattened once per render for the virtual list.
    rows: Rc<Vec<Row>>,
    /// Why the project folder itself can't be listed.
    root_error: Option<String>,
    /// Top folder of the Git repository the project is in, from the project's Git store.
    repo: Option<PathBuf>,
    clip: Option<Clip>,
    /// Changes that reverse the latest ones, newest last.
    undo: Vec<Op>,
    redo: Vec<Op>,
    /// File changes asked for while another runs, in the order asked.
    queue: VecDeque<Request>,
    /// A file change is running.
    busy: bool,
    disk: Disk,
    /// Counts the folders shown ([`Self::forget_folder`]): work started for one that lands
    /// once another is shown leaves that one alone.
    folder: u64,
    edit: Option<Edit>,
    /// Why the last file operation failed.
    error: Option<String>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    _refresh: Task<()>,
    _shown_focus: Vec<Subscription>,
}

impl FilesPanel {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        // The selected row is outlined while the tree has the keyboard.
        let shown_focus = vec![
            cx.on_focus_in(&focus, window, |_, _, cx| cx.notify()),
            cx.on_focus_out(&focus, window, |_, _, _, cx| cx.notify()),
        ];
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    return;
                }
            }
        });
        Self {
            root: None,
            expanded: HashSet::new(),
            children: HashMap::new(),
            selected: None,
            rows: Rc::default(),
            root_error: None,
            repo: None,
            clip: None,
            undo: Vec::new(),
            redo: Vec::new(),
            queue: VecDeque::new(),
            busy: false,
            disk: Disk::default(),
            folder: 0,
            edit: None,
            error: None,
            focus,
            scroll: UniformListScrollHandle::new(),
            _refresh: refresh,
            _shown_focus: shown_focus,
        }
    }
}

/// A folder of a test's own under the temp folder, by its real path, removed when the test ends.
#[cfg(test)]
pub(crate) struct Scratch(pub(crate) PathBuf);

#[cfg(test)]
impl Scratch {
    pub(crate) fn new(test: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("sawit-files-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir.canonicalize().unwrap())
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests;
