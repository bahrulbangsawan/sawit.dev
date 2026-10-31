//! Project file tree (left panel), loaded lazily per folder.

pub(crate) mod ops;
pub mod search;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use futures::channel::oneshot;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

use sawit_core::files::{self, Entry, list_dir};

use crate::rt;
use crate::states::{Banner, Placeholder};
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
        KeyBinding::new("alt-cmd-backspace", Delete, tree),
    ]);
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
const ROW_HEIGHT: f32 = 22.;
/// Folders Expand All reads before it stops.
const MAX_EXPAND: usize = 2000;

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
/// landed, not the one before it.
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
    /// Top folder of the Git repository the project is in.
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
}

impl FilesPanel {
    pub fn new(cx: &mut Context<Self>) -> Self {
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
            focus: cx.focus_handle(),
            scroll: UniformListScrollHandle::new(),
            _refresh: refresh,
        }
    }

    /// Re-reads every open folder; the tree only changes if they did.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<PathBuf> = self
            .expanded
            .iter()
            .filter(|d| self.children.contains_key(*d))
            .cloned()
            .collect();
        for dir in dirs {
            self.load(dir, cx);
        }
    }

    pub fn prompt_open(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Project Folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.set_root(dir, cx));
        })
        .detach();
    }

    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.forget_folder();
        self.expanded.insert(root.clone());
        self.load(root.clone(), cx);
        let dir = root.clone();
        let folder = self.folder;
        let rx = rt::spawn(cx, async move { sawit_core::git::discover(&dir).await });
        cx.spawn(async move |this, cx| {
            if let Ok(repo) = rx.await {
                let _ = this.update(cx, |this, _| this.found_repo(folder, repo));
            }
        })
        .detach();
        cx.emit(FilesEvent::RootChanged(root.clone()));
        self.root = Some(root);
        cx.notify();
    }

    /// Forgets the folder shown so far, for [`Self::set_root`]: its tree and how far it was
    /// scrolled, its repository, the entry selected and the one cut or copied there, its undo and
    /// redo, a name being typed and the last error. No row of the next folder's tree would show
    /// the old selection or Cut, yet ⌫ (Trash), Rename and Paste would act on them.
    fn forget_folder(&mut self) {
        self.folder += 1;
        self.expanded.clear();
        self.children.clear();
        self.scroll = UniformListScrollHandle::new();
        self.selected = None;
        self.clip = None;
        self.root_error = None;
        self.repo = None;
        self.undo.clear();
        self.redo.clear();
        self.queue.clear();
        self.edit = None;
        self.error = None;
    }

    fn load(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let folder = self.folder;
        let rx = (self.disk.list)(vec![dir], cx);
        cx.spawn(async move |this, cx| {
            let Ok(listings) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for (dir, listing) in listings {
                    this.listed(folder, dir, listing, cx);
                }
            });
        })
        .detach();
    }

    /// The repository of folder number `folder` ([`Self::folder`]) was looked up.
    fn found_repo(&mut self, folder: u64, repo: Option<PathBuf>) {
        if folder == self.folder {
            self.repo = repo;
        }
    }

    /// Shows what reading `dir`, of folder number `folder`, found.
    fn listed(&mut self, folder: u64, dir: PathBuf, listing: Listing, cx: &mut Context<Self>) {
        if folder != self.folder {
            return;
        }
        match listing {
            Ok(entries) => {
                if self.root.as_ref() == Some(&dir) && self.root_error.take().is_some() {
                    cx.notify();
                }
                if self.children.get(&dir) != Some(&entries) {
                    self.children.insert(dir, entries);
                    cx.notify();
                }
            }
            // The folder was deleted or renamed: forget it (or, for the
            // project folder, say so).
            Err(e) => {
                if self.root.as_ref() == Some(&dir) {
                    self.root_error = Some(e.to_string());
                }
                self.children.remove(&dir);
                self.expanded.remove(&dir);
                cx.notify();
            }
        }
    }

    fn toggle(&mut self, entry: &Entry, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selected = Some(entry.path.clone());
        if entry.is_dir {
            if !self.expanded.remove(&entry.path) {
                // Read again, though it was read before: it may have changed since it closed.
                self.expanded.insert(entry.path.clone());
                self.load(entry.path.clone(), cx);
            }
        } else {
            cx.emit(FilesEvent::OpenFile(entry.path.clone()));
        }
        cx.notify();
    }

    fn rows(&self, dir: &Path, depth: usize, out: &mut Vec<Row>) {
        if let Some(edit) = &self.edit
            && matches!(edit.kind, EditKind::New { .. })
            && edit.parent == dir
        {
            out.push(Row::Edit(depth));
        }
        let Some(entries) = self.children.get(dir) else {
            return;
        };
        for entry in entries {
            let renaming = self
                .edit
                .as_ref()
                .is_some_and(|e| matches!(&e.kind, EditKind::Rename(p) if *p == entry.path));
            out.push(if renaming {
                Row::Edit(depth)
            } else {
                Row::Entry(entry.clone(), depth)
            });
            if entry.is_dir && self.expanded.contains(&entry.path) {
                self.rows(&entry.path, depth + 1, out);
            }
        }
    }
}

impl FilesPanel {
    fn flat(&self) -> Vec<Row> {
        let mut flat = Vec::new();
        if let Some(root) = &self.root {
            self.rows(root, 0, &mut flat);
        }
        flat
    }

    fn render_row(
        &self,
        ix: usize,
        entry: Entry,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.selected.as_ref() == Some(&entry.path);
        let muted = cx.theme().muted_foreground;
        let open = entry.is_dir && self.expanded.contains(&entry.path);
        let (chevron, icon) = if entry.is_dir {
            (
                Some(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                }),
                Icon::new(if open {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            )
        } else {
            (None, crate::file_icons::file_icon(&entry.path, cx))
        };
        let name = entry.name.clone();
        let cut = self
            .clip
            .as_ref()
            .is_some_and(|c| c.cut && c.path == entry.path);
        let path = entry.path.clone();
        let menu = self.menu_state();
        let focus = self.focus.clone();
        let dragged = DraggedFile {
            path: entry.path.clone(),
            name: entry.name.clone().into(),
        };
        h_flex()
            .id(("file-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(4. + depth as f32 * 12.))
            .pr_2()
            .gap_1()
            .text_sm()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .when(cut, |el| el.opacity(0.5))
            .child(
                div()
                    .w(px(14.))
                    .flex_none()
                    .children(chevron.map(|c| Icon::new(c).xsmall().text_color(muted))),
            )
            .child(icon)
            .child(div().truncate().child(name))
            .on_drag(dragged, |dragged, _, _, cx| cx.new(|_| dragged.clone()))
            .on_click(cx.listener(move |this, _, window, cx| this.toggle(&entry, window, cx)))
            // The menu's commands act on the row clicked.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    this.selected = Some(path.clone());
                    cx.notify();
                }),
            )
            .context_menu(move |m, _, _| file_menu(m, focus.clone(), menu))
            .into_any_element()
    }

    fn render_edit(&self, ix: usize, depth: usize, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let icon = match &self.edit {
            Some(Edit {
                kind: EditKind::New { dir: true },
                ..
            }) => Icon::new(IconName::Folder)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            Some(Edit {
                kind: EditKind::Rename(path),
                ..
            }) if path.is_dir() => Icon::new(IconName::Folder)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
            Some(Edit {
                kind: EditKind::Rename(path),
                ..
            }) => crate::file_icons::file_icon(path, cx),
            _ => Icon::new(IconName::File)
                .xsmall()
                .text_color(muted)
                .into_any_element(),
        };
        h_flex()
            .id(("file-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .pl(px(4. + depth as f32 * 12.))
            .pr_2()
            .gap_1()
            .child(div().w(px(14.)).flex_none())
            .child(icon)
            .children(self.edit.as_ref().map(|e| {
                div()
                    .flex_1()
                    .min_w_0()
                    .on_action(
                        cx.listener(|this, _: &Escape, window, cx| this.cancel_edit(window, cx)),
                    )
                    .child(Input::new(&e.input).xsmall())
            }))
            .into_any_element()
    }
}

impl Render for FilesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rows = Rc::new(self.flat());
        let count = self.rows.len();
        let title = self
            .root
            .as_ref()
            .and_then(|r| r.file_name())
            .map(|n| n.to_string_lossy().to_uppercase())
            .unwrap_or_else(|| "FILES".into());

        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .key_context("FilesPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &NewFile, w, cx| this.start_new(false, w, cx)))
            .on_action(cx.listener(|this, _: &NewFolder, w, cx| this.start_new(true, w, cx)))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| {
                if let Some(path) = this.target() {
                    cx.reveal_path(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInDefaultApp, _, cx| {
                if let Some(path) = this.target() {
                    cx.open_with_system(&path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInTerminal, _, cx| {
                if let Some(dir) = this.target_dir() {
                    cx.emit(FilesEvent::OpenTerminal(dir));
                }
            }))
            .on_action(cx.listener(|this, _: &FindInFolder, _, cx| {
                if let Some(dir) = this.target_dir() {
                    cx.emit(FilesEvent::FindInFolder(dir));
                }
            }))
            .on_action(cx.listener(|this, _: &Cut, _, cx| this.pick(true, cx)))
            .on_action(cx.listener(|this, _: &CopyEntry, _, cx| this.pick(false, cx)))
            .on_action(cx.listener(|this, _: &Duplicate, w, cx| this.duplicate(w, cx)))
            .on_action(cx.listener(|this, _: &Paste, w, cx| this.paste(w, cx)))
            .on_action(cx.listener(|this, _: &Undo, w, cx| this.request(Request::Undo, w, cx)))
            .on_action(cx.listener(|this, _: &Redo, w, cx| this.request(Request::Redo, w, cx)))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| this.copy_path(false, cx)))
            .on_action(cx.listener(|this, _: &CopyRelativePath, _, cx| this.copy_path(true, cx)))
            .on_action(cx.listener(|this, _: &AddToGitignore, _, cx| this.ignore(false, cx)))
            .on_action(cx.listener(|this, _: &AddToGitExclude, _, cx| this.ignore(true, cx)))
            .on_action(cx.listener(|this, _: &ViewHistory, _, cx| {
                if let Some(path) = this.target() {
                    cx.emit(FilesEvent::ViewHistory(path));
                }
            }))
            .on_action(cx.listener(|this, _: &Rename, w, cx| this.start_rename(w, cx)))
            .on_action(cx.listener(|this, _: &Trash, w, cx| {
                if let Some(path) = this.trash_target() {
                    this.request(Request::Do(Op::Trash(path), After::default()), w, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &Delete, w, cx| this.delete(w, cx)))
            .on_action(cx.listener(|this, _: &ExpandAll, _, cx| this.expand_all(cx)))
            .on_action(cx.listener(|this, _: &CollapseAll, _, cx| this.collapse_all(cx)))
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(title),
                    )
                    .when(self.busy, |el| {
                        el.child(
                            div()
                                .id("files-busy")
                                .px_1()
                                .child(Spinner::new().xsmall().color(cx.theme().muted_foreground))
                                .tooltip(|window, cx| {
                                    Tooltip::new("Changing files…").build(window, cx)
                                }),
                        )
                    })
                    .when(self.root.is_some(), |el| {
                        el.child(
                            Button::new("refresh-files")
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .tooltip("Reload File List")
                                .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                        )
                    })
                    .child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .icon(IconName::FolderOpen)
                            .tooltip("Open Project Folder")
                            .on_click(cx.listener(|this, _, _, cx| this.prompt_open(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("files-error", e).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    },
                )))
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .py_1()
                    .when(count > 0, |el| {
                        // Virtual: only visible rows are built.
                        el.child(
                            uniform_list(
                                "file-rows",
                                count,
                                cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                                    let rows = this.rows.clone();
                                    range
                                        .filter_map(|ix| {
                                            Some(match rows.get(ix)?.clone() {
                                                Row::Entry(entry, depth) => {
                                                    this.render_row(ix, entry, depth, cx)
                                                }
                                                Row::Edit(depth) => this.render_edit(ix, depth, cx),
                                            })
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .map(|el| match (&self.root, &self.root_error) {
                        (None, _) => el.child(
                            Placeholder::new(IconName::FolderOpen, "No folder open")
                                .compact()
                                .detail("Open a project to browse and edit its files here.")
                                .action(
                                    Button::new("empty-open-folder")
                                        .small()
                                        .primary()
                                        .label("Open Folder…")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.prompt_open(cx)),
                                        ),
                                ),
                        ),
                        (Some(root), Some(e)) => {
                            let root = root.clone();
                            el.child(
                                Placeholder::error("Unable to read this folder")
                                    .compact()
                                    .detail(e.clone())
                                    .action(
                                        Button::new("retry-root")
                                            .small()
                                            .label("Try Again")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_root(root.clone(), cx)
                                            })),
                                    ),
                            )
                        }
                        (Some(root), None)
                            if self.children.get(root).is_some_and(|c| c.is_empty()) =>
                        {
                            el.child(
                                Placeholder::new(IconName::Folder, "This folder is empty")
                                    .compact()
                                    .detail("Files you add here show up automatically."),
                            )
                        }
                        _ => el,
                    }),
            )
    }
}

/// What the context menu can offer, read when the row is drawn.
#[derive(Clone, Copy)]
struct MenuState {
    paste: bool,
    undo: bool,
    redo: bool,
    git: bool,
}

fn file_menu(menu: PopupMenu, focus: FocusHandle, can: MenuState) -> PopupMenu {
    menu.action_context(focus)
        .menu("New File", Box::new(NewFile))
        .menu("New Folder", Box::new(NewFolder))
        .separator()
        .menu("Reveal in Finder", Box::new(RevealInFinder))
        .menu("Open in Default App", Box::new(OpenInDefaultApp))
        .menu("Open in Terminal", Box::new(OpenInTerminal))
        .separator()
        .menu("Find in Folder…", Box::new(FindInFolder))
        .separator()
        .menu("Cut", Box::new(Cut))
        .menu("Copy", Box::new(CopyEntry))
        .menu("Duplicate", Box::new(Duplicate))
        .menu_with_disabled("Paste", Box::new(Paste), !can.paste)
        .menu_with_disabled("Undo", Box::new(Undo), !can.undo)
        .menu_with_disabled("Redo", Box::new(Redo), !can.redo)
        .separator()
        .menu("Copy Path", Box::new(CopyPath))
        .menu("Copy Relative Path", Box::new(CopyRelativePath))
        .separator()
        .menu_with_disabled("Add to .gitignore", Box::new(AddToGitignore), !can.git)
        .menu_with_disabled(
            "Add to .git/info/exclude",
            Box::new(AddToGitExclude),
            !can.git,
        )
        .menu_with_disabled("View History", Box::new(ViewHistory), !can.git)
        .separator()
        .menu("Rename", Box::new(Rename))
        .menu("Trash", Box::new(Trash))
        .menu("Delete", Box::new(Delete))
        .separator()
        .menu("Expand All", Box::new(ExpandAll))
        .menu("Collapse All", Box::new(CollapseAll))
}

/// Every folder under `dir` with its entries, breadth first, up to `MAX_EXPAND`.
fn read_tree(dir: PathBuf) -> Vec<(PathBuf, Vec<Entry>)> {
    let mut out = Vec::new();
    let mut queue = std::collections::VecDeque::from([dir]);
    while let Some(dir) = queue.pop_front() {
        if out.len() >= MAX_EXPAND {
            break;
        }
        let Ok(entries) = list_dir(&dir) else {
            continue;
        };
        queue.extend(entries.iter().filter(|e| e.is_dir).map(|e| e.path.clone()));
        out.push((dir, entries));
    }
    out
}

/// File operations from the context menu and their shortcuts.
impl FilesPanel {
    fn menu_state(&self) -> MenuState {
        MenuState {
            paste: self.clip.is_some(),
            undo: !self.undo.is_empty(),
            redo: !self.redo.is_empty(),
            git: self.repo.is_some(),
        }
    }

    /// The selected entry, if its row is shown. The commands that act on an entry (Trash,
    /// Delete, Rename, Duplicate, Cut, Copy) never act on anything else: not on an entry in a
    /// folder that isn't open or that went, nor on the project folder or anything outside it.
    fn selection(&self) -> Option<PathBuf> {
        self.selected.clone().filter(|path| self.shown(path))
    }

    /// Whether `path` has a row in the tree: it is listed, and its folder and the folders
    /// above it, up to the project folder, are open.
    fn shown(&self, path: &Path) -> bool {
        let (Some(root), Some(dir)) = (&self.root, path.parent()) else {
            return false;
        };
        dir.starts_with(root)
            && self.root_error.is_none()
            && (self.children.get(dir))
                .is_some_and(|entries| entries.iter().any(|e| e.path == path))
            && dir
                .ancestors()
                .take_while(|d| d.starts_with(root))
                .all(|d| self.expanded.contains(d))
    }

    /// The selected entry, else the project folder.
    fn target(&self) -> Option<PathBuf> {
        self.selection().or_else(|| self.root.clone())
    }

    /// What Trash moves: the selected entry, or nothing when none is selected. Never the project
    /// folder, which [`Self::target`] falls back to for the commands that show, open or copy a
    /// path: ⌫ in a tree with no selection would move the whole project to the Trash.
    fn trash_target(&self) -> Option<PathBuf> {
        self.selection()
    }

    /// The target if it is a folder, else the folder it is in.
    fn target_dir(&self) -> Option<PathBuf> {
        let path = self.target()?;
        if path.is_dir() {
            Some(path)
        } else {
            path.parent().map(Path::to_path_buf)
        }
    }

    /// Opens `dir` (and the folders above it, up to the project) in the tree.
    fn expand_to(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        for dir in dir.ancestors().take_while(|d| d.starts_with(&root)) {
            if self.expanded.insert(dir.to_path_buf()) && !self.children.contains_key(dir) {
                self.load(dir.to_path_buf(), cx);
            }
        }
    }

    fn start_new(&mut self, dir: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(parent) = self.target_dir() else {
            return;
        };
        self.expand_to(&parent, cx);
        self.begin_edit(EditKind::New { dir }, parent, "", window, cx);
    }

    fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selection() else {
            return;
        };
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return;
        };
        let (parent, name) = (parent.to_path_buf(), name.to_string_lossy().into_owned());
        self.begin_edit(EditKind::Rename(path), parent, &name, window, cx);
    }

    fn begin_edit(
        &mut self,
        kind: EditKind,
        parent: PathBuf,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Rename selects the name without its extension.
        let stem = match &kind {
            EditKind::Rename(path) if !path.is_dir() => {
                Path::new(name).file_stem().map_or(name.len(), |s| s.len())
            }
            _ => name.len(),
        };
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx);
            state.set_value(name.to_string(), window, cx);
            state.set_selected_range(0..stem, cx);
            state
        });
        let subscription = cx.subscribe_in(
            &input,
            window,
            |this, _, e: &InputEvent, window, cx| match e {
                InputEvent::PressEnter { .. } => this.confirm_edit(window, cx),
                InputEvent::Blur => this.cancel_edit(window, cx),
                _ => {}
            },
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        self.error = None;
        self.edit = Some(Edit {
            kind,
            parent,
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.edit.take().is_some() {
            window.focus(&self.focus, cx);
            cx.notify();
        }
    }

    fn confirm_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.edit else {
            return;
        };
        let name = edit.input.read(cx).value().trim().to_string();
        if name.contains('/') || name == "." || name == ".." {
            self.error = Some(format!("“{name}” can't be used as a name."));
            cx.notify();
            return;
        }
        let path = edit.parent.join(&name);
        let op = match &edit.kind {
            _ if name.is_empty() => None,
            EditKind::New { dir } => Some(Op::Create { path, dir: *dir }),
            EditKind::Rename(from) if *from == path => None,
            EditKind::Rename(from) => Some(Op::Move {
                from: from.clone(),
                to: path,
            }),
        };
        let named = Named {
            kind: edit.kind.clone(),
            parent: edit.parent.clone(),
            name,
        };
        self.cancel_edit(window, cx);
        if let Some(op) = op {
            let after = After {
                named: Some(named),
                ..After::default()
            };
            self.request(Request::Do(op, after), window, cx);
        }
    }

    /// Opens the name box again with the name typed for a change that failed, unless a name
    /// is being typed again already or the box would be drawn nowhere.
    fn retype(&mut self, named: Named, window: &mut Window, cx: &mut Context<Self>) {
        let drawn = match &named.kind {
            EditKind::Rename(path) => self.shown(path),
            EditKind::New { .. } => {
                Some(&named.parent) == self.root.as_ref()
                    || self.shown(&named.parent) && self.expanded.contains(&named.parent)
            }
        };
        if self.edit.is_none() && drawn {
            self.begin_edit(named.kind, named.parent, &named.name, window, cx);
        }
    }

    /// Asks for a file change. It runs off the UI thread once the changes asked for before it
    /// have landed, so that each undo or redo applies to the history as the changes before it
    /// left it.
    fn request(&mut self, request: Request, window: &mut Window, cx: &mut Context<Self>) {
        self.queue.push_back(request);
        self.next(window, cx);
    }

    /// Starts the next change asked for, unless one is running.
    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while !self.busy {
            let Some(request) = self.queue.pop_front() else {
                return;
            };
            let (op, step, after) = match request {
                // A copy takes the first free name when it starts: one before it may have
                // taken the name it was given.
                Request::Do(Op::Copy { from, to }, after) => {
                    let name = from.file_name().unwrap_or_default().to_string_lossy();
                    let dir = to.parent().unwrap_or(Path::new("/"));
                    let to = files::free_name(dir, &name, from.is_dir());
                    (Op::Copy { from, to }, Step::Do, after)
                }
                Request::Do(op, after) => (op, Step::Do, after),
                Request::Undo => match self.undo.pop() {
                    Some(op) => (op, Step::Undo, After::default()),
                    None => continue,
                },
                Request::Redo => match self.redo.pop() {
                    Some(op) => (op, Step::Redo, After::default()),
                    None => continue,
                },
            };
            self.start(op, step, after, window, cx);
        }
    }

    /// Runs `op` off the UI thread; [`Self::landed`] takes it from there.
    fn start(
        &mut self,
        op: Op,
        step: Step,
        after: After,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        cx.notify();
        let folder = self.folder;
        let rx = (self.disk.run)(op.clone(), cx);
        cx.spawn_in(window, async move |this, cx| {
            let outcome = rx
                .await
                .unwrap_or_else(|_| Err("The change stopped before it finished.".into()));
            let _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                this.landed(folder, op, step, after, outcome, window, cx);
                this.next(window, cx);
            });
        })
        .detach();
    }

    /// Takes `op`, run as `step` in folder number `folder`, once it has landed: keeps its undo
    /// (or redo), and shows it.
    #[allow(clippy::too_many_arguments)]
    fn landed(
        &mut self,
        folder: u64,
        op: Op,
        step: Step,
        after: After,
        outcome: Outcome,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if folder != self.folder {
            // Another folder is shown now. A move that happened is still told, for the tabs
            // of what moved to follow it; the rest was that folder's.
            if let (Ok(_), Some((from, to))) = (&outcome, op.moved()) {
                cx.emit(FilesEvent::Moved {
                    from: from.to_path_buf(),
                    to: to.to_path_buf(),
                });
            }
            return;
        }
        match outcome {
            Ok(inverse) => {
                match step {
                    Step::Do => {
                        self.undo.push(inverse);
                        self.redo.clear();
                    }
                    Step::Undo => self.redo.push(inverse),
                    Step::Redo => self.undo.push(inverse),
                }
                self.error = None;
                if let Some((from, to)) = op.moved() {
                    cx.emit(FilesEvent::Moved {
                        from: from.to_path_buf(),
                        to: to.to_path_buf(),
                    });
                }
                if let Op::Trash(gone) = &op {
                    self.forget_selection(gone);
                }
                if let (true, Op::Move { from, .. }) = (after.pastes_cut, &op)
                    && self.clip.as_ref().is_some_and(|c| c.cut && c.path == *from)
                {
                    self.clip = None;
                }
                if let Some(path) = op.result().map(Path::to_path_buf) {
                    if let Some(parent) = path.parent() {
                        self.expand_to(parent, cx);
                    }
                    if matches!((&op, step), (Op::Create { dir: false, .. }, Step::Do)) {
                        cx.emit(FilesEvent::OpenFile(path.clone()));
                    }
                    self.selected = Some(path);
                }
            }
            Err(e) => {
                if let Some(named) = after.named {
                    self.retype(named, window, cx);
                }
                self.error = Some(e);
                // An undo or redo that failed stays, to try again once what was in its way is
                // dealt with ("… already exists here"). One whose file is gone never can.
                let still = std::fs::symlink_metadata(op.source()).is_ok();
                match step {
                    Step::Undo if still => self.undo.push(op),
                    Step::Redo if still => self.redo.push(op),
                    _ => {}
                }
            }
        }
        self.refresh(cx);
        cx.notify();
    }

    fn pick(&mut self, cut: bool, cx: &mut Context<Self>) {
        if let Some(path) = self.selection() {
            self.clip = Some(Clip { path, cut });
            cx.notify();
        }
    }

    /// Pastes what was cut (moving it) or copied. The Cut stays until its move lands, so that
    /// one that fails ("… already exists here") can be pasted elsewhere.
    fn paste(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(clip), Some(dir)) = (self.clip.clone(), self.target_dir()) else {
            return;
        };
        let Some(name) = clip
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
        else {
            return;
        };
        if clip.cut {
            let to = dir.join(&name);
            if to != clip.path {
                let after = After {
                    pastes_cut: true,
                    ..After::default()
                };
                let op = Op::Move {
                    from: clip.path,
                    to,
                };
                self.request(Request::Do(op, after), window, cx);
            }
        } else {
            let to = files::free_name(&dir, &name, clip.path.is_dir());
            let op = Op::Copy {
                from: clip.path,
                to,
            };
            self.request(Request::Do(op, After::default()), window, cx);
        }
    }

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.selection() else {
            return;
        };
        let (Some(dir), Some(name)) = (from.parent(), from.file_name()) else {
            return;
        };
        let to = files::free_name(dir, &name.to_string_lossy(), from.is_dir());
        let op = Op::Copy { from, to };
        self.request(Request::Do(op, After::default()), window, cx);
    }

    fn copy_path(&mut self, relative: bool, cx: &mut Context<Self>) {
        let Some(path) = self.target() else {
            return;
        };
        let text = match (&self.root, relative) {
            (Some(root), true) => match path.strip_prefix(root) {
                Ok(rel) if rel.as_os_str().is_empty() => ".".into(),
                Ok(rel) => rel.to_string_lossy().into_owned(),
                Err(_) => path.to_string_lossy().into_owned(),
            },
            _ => path.to_string_lossy().into_owned(),
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
    }

    /// Adds the target to the repository's `.gitignore`, or to its private
    /// `.git/info/exclude` ([`sawit_core::git::ignore`]).
    fn ignore(&mut self, exclude: bool, cx: &mut Context<Self>) {
        let (Some(repo), Some(path)) = (self.repo.clone(), self.target()) else {
            return;
        };
        let rx = rt::spawn(cx, async move {
            sawit_core::git::ignore(&repo, &path, exclude)
                .await
                .map(drop)
                .map_err(|e| e.to_string())
        });
        let folder = self.folder;
        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| this.failed(folder, e, cx));
            }
        })
        .detach();
    }

    /// Selects nothing if the entry selected was `gone` or in it.
    fn forget_selection(&mut self, gone: &Path) {
        if self.selected.as_ref().is_some_and(|s| s.starts_with(gone)) {
            self.selected = None;
        }
    }

    /// Shows why something done in folder number `folder` failed.
    fn failed(&mut self, folder: u64, error: String, cx: &mut Context<Self>) {
        if folder == self.folder {
            self.error = Some(error);
            cx.notify();
        }
    }

    /// Deletes the selected entry for good, after asking.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selection() else {
            return;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete “{name}” permanently?"),
            Some("It won't go to the Trash, and this can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        let folder = self.folder;
        cx.spawn(async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let gone = path.clone();
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn_blocking(cx, move || files::remove(&gone).map_err(|e| e.to_string()))
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(Err(e)) => this.failed(folder, e, cx),
                    Ok(Ok(())) if folder == this.folder => this.forget_selection(&path),
                    _ => {}
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens every folder under the target folder (or the project).
    fn expand_all(&mut self, cx: &mut Context<Self>) {
        let Some(dir) = self
            .selection()
            .filter(|p| p.is_dir())
            .or_else(|| self.root.clone())
        else {
            return;
        };
        self.expand_to(&dir, cx);
        let folder = self.folder;
        let rx = rt::spawn_blocking(cx, move || read_tree(dir));
        cx.spawn(async move |this, cx| {
            let Ok(tree) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.expanded_all(folder, tree, cx));
        })
        .detach();
    }

    /// Opens the folders Expand All read in folder number `folder`.
    fn expanded_all(
        &mut self,
        folder: u64,
        tree: Vec<(PathBuf, Vec<Entry>)>,
        cx: &mut Context<Self>,
    ) {
        if folder != self.folder {
            return;
        }
        for (dir, entries) in tree {
            self.expanded.insert(dir.clone());
            self.children.insert(dir, entries);
        }
        cx.notify();
    }

    /// Closes every folder but the project folder. An entry selected in one is given up for
    /// that folder at the top, which stays shown.
    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.expanded.retain(|d| *d == root);
        if let Some(selected) = &self.selected
            && let Ok(rel) = selected.strip_prefix(&root)
            && let Some(top) = rel.components().next()
        {
            self.selected = Some(root.join(top));
        }
        cx.notify();
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
mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;

    use futures::channel::oneshot;
    use gpui_kit::{
        Action, AnyWindowHandle, AppContext as _, Entity, FocusHandle, Point, Subscription,
        TestAppContext, point, px,
    };
    use sawit_core::files::Entry;

    use super::{After, Clip, Disk, FilesEvent, FilesPanel, Op, Outcome, Request, Scratch, Trash};

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
    fn open_panel(
        cx: &mut TestAppContext,
        files: &[&str],
    ) -> (AnyWindowHandle, Entity<FilesPanel>) {
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
            gpui_kit::open_window(Default::default(), cx, |_, cx| cx.new(FilesPanel::new)).unwrap()
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
    struct Changes(Rc<RefCell<Vec<Started>>>, Rc<RefCell<Vec<PathBuf>>>);

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
                            read.1.borrow_mut().extend(dirs);
                            oneshot::channel().1
                        }),
                    }
                })
            });
            changes
        }

        /// The folders read so far, in order.
        fn read(&self) -> Vec<PathBuf> {
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
                })
            })
        });
        (seen, subscription)
    }

    /// Work started in one folder that lands once the panel shows another leaves that one alone
    /// (FILES-3). A change's undo and selection, an error, Expand All's folders, a folder's
    /// listing and the repository of the old folder all went to the new one: ⌫ and ⌘Z then
    /// trashed the old folder's copy, unseen. A move that landed is still told, for the tabs of
    /// what moved to follow it. A change asked for in the old folder doesn't run.
    #[gpui_kit::test]
    fn work_that_lands_after_a_folder_change_leaves_the_new_folder_alone(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &["notes.txt", "src"]);
        let changes = Changes::install(&panel, cx);
        let (seen, _events) = events(&panel, cx);
        let root = Path::new(ROOT);
        let (notes, kept) = (root.join("notes.txt"), root.join("kept.txt"));
        let rename = Op::Move {
            from: notes.clone(),
            to: kept.clone(),
        };
        ask(cx, window, &panel, rename);
        let duplicate = Op::Copy {
            from: notes.clone(),
            to: root.join("notes copy.txt"),
        };
        ask(cx, window, &panel, duplicate);
        let old = cx.update(|cx| {
            panel.update(cx, |p, _| {
                let old = p.folder;
                // What set_root does, but for reading the new folder.
                p.forget_folder();
                show(p, Path::new(OTHER_ROOT), &["readme.md"]);
                old
            })
        });
        let undo = Op::Move {
            from: kept.clone(),
            to: notes.clone(),
        };
        changes.land(0, Ok(undo), cx);
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.found_repo(old, Some(root.to_path_buf()));
                let entry = Entry {
                    name: "lib.rs".into(),
                    path: root.join("src/lib.rs"),
                    is_dir: false,
                };
                p.listed(old, root.join("src"), Ok(vec![entry]), cx);
                p.expanded_all(old, vec![(root.join("src"), Vec::new())], cx);
                p.failed(old, "“notes.txt” couldn't be renamed.".into(), cx);
            })
        });

        assert_eq!(changes.started().len(), 1, "the duplicate ran");
        assert_eq!(*seen.borrow(), ["moved notes.txt kept.txt"]);
        // Should the old folder's entry be selected all the same, nothing acts on it.
        cx.update(|cx| {
            panel.update(cx, |p, _| {
                p.selected = Some(kept.clone());
                assert_eq!((p.trash_target(), p.selection()), (None, None));
                assert_eq!(p.target().as_deref(), Some(Path::new(OTHER_ROOT)));
                p.selected = None;
            })
        });
        cx.update(|cx| {
            let p = panel.read(cx);
            assert!(p.undo.is_empty() && p.redo.is_empty() && !p.busy);
            assert_eq!((&p.selected, &p.error, &p.repo), (&None, &None, &None));
            let other = Path::new(OTHER_ROOT).to_path_buf();
            assert_eq!(p.expanded.iter().collect::<Vec<_>>(), [&other]);
            assert_eq!(p.children.keys().collect::<Vec<_>>(), [&other]);
        });
    }

    /// Keys act only on an entry whose row is shown (FILES-6). The selection used to outlive its
    /// row: after a Trash, ⌫ again tried the same entry ("couldn't be moved to the Trash");
    /// after Collapse All, ⌫ trashed a file in a folder no longer open, unseen; with the
    /// project folder unreadable, its entries could still be renamed, F2's name box drawn
    /// nowhere, with the keyboard.
    #[gpui_kit::test]
    fn keys_act_only_on_an_entry_whose_row_is_shown(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &[]);
        let changes = Changes::install(&panel, cx);
        let root = Path::new(ROOT);
        let (src, x) = (root.join("src"), root.join("src/lib/x.rs"));
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                show_tree(p, root);
                cx.notify();
            })
        });
        cx.run_until_parked();

        // A Trash of the selected file lands: nothing is selected now.
        select(cx, window, &panel, &x);
        ask(cx, window, &panel, Op::Trash(x.clone()));
        let back = Op::Restore {
            from: PathBuf::from("/Trash/x.rs"),
            to: x.clone(),
        };
        changes.land(0, Ok(back), cx);
        cx.update(|cx| assert_eq!(panel.read(cx).selected, None));

        // Collapse All, with the file selected: the folder it is in, still shown, is selected.
        select(cx, window, &panel, &x);
        cx.update(|cx| panel.update(cx, |p, cx| p.collapse_all(cx)));
        cx.update(|cx| assert_eq!(panel.read(cx).selected.as_ref(), Some(&src)));

        // An entry selected whose row isn't shown: F2 and ⌘D do nothing.
        select(cx, window, &panel, &x);
        cx.simulate_keystrokes(window, "f2 cmd-d");
        cx.update(|cx| {
            let p = panel.read(cx);
            assert!(p.edit.is_none(), "a name box nowhere to be seen");
            assert_eq!((p.selection(), p.trash_target()), (None, None));
        });
        assert_eq!(changes.started().len(), 1, "a duplicate ran");

        // The project folder can't be read: none of its entries is shown.
        cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.selected = Some(root.join("notes.txt"));
                assert!(p.selection().is_some());
                let unreadable = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
                p.listed(p.folder, root.to_path_buf(), Err(unreadable.into()), cx);
                assert_eq!(p.selection(), None);
            })
        });
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

    /// Small slips (FILES-16). A Paste that fails ("… already exists here") keeps the Cut, to
    /// paste elsewhere; a rename or a new name that fails brings its name box back with the
    /// name typed, to fix; and a folder opened again is read again, as it may have changed
    /// while it was closed. The Cut was cleared before its move ran, the name box closed with
    /// the name in it, and a folder opened again showed what it held when it was closed.
    #[gpui_kit::test]
    fn a_change_that_fails_keeps_what_was_cut_or_typed(cx: &mut TestAppContext) {
        let dir = Scratch::new("a-change-that-fails");
        let root = dir.0.as_path();
        std::fs::write(root.join("a.txt"), "").unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        let (window, panel) = open_panel_at(cx, root, &["a.txt", "docs"]);
        let changes = Changes::install(&panel, cx);
        let (a, docs) = (root.join("a.txt"), root.join("docs"));
        let cut = |cx: &mut TestAppContext| {
            cx.update(|cx| (panel.read(cx).clip.as_ref()).map(|c| (c.path.clone(), c.cut)))
        };

        // ⌘X, then ⌘V in a folder that has the name: the Cut is still there to paste.
        select(cx, window, &panel, &a);
        cx.simulate_keystrokes(window, "cmd-x");
        select(cx, window, &panel, &docs);
        cx.simulate_keystrokes(window, "cmd-v");
        let error = "“a.txt” already exists here.";
        changes.land(0, Err(error.into()), cx);
        assert_eq!(cut(cx), Some((a.clone(), true)));
        cx.simulate_keystrokes(window, "cmd-v");
        let back = Op::Move {
            from: docs.join("a.txt"),
            to: a.clone(),
        };
        changes.land(1, Ok(back), cx);
        assert_eq!(cut(cx), None, "pasted");

        // A rename to a name that is taken: the name box is back, with the name typed.
        select(cx, window, &panel, &a);
        cx.simulate_keystrokes(window, "f2");
        name_it(cx, window, &panel, "docs");
        let taken = "“docs” already exists here.";
        changes.land(2, Err(taken.into()), cx);
        assert_eq!(typed(&panel, cx).as_deref(), Some("docs"));
        cx.update(|cx| assert_eq!(panel.read(cx).error.as_deref(), Some(taken)));
        cx.simulate_keystrokes(window, "escape");

        // The same for a new file.
        select(cx, window, &panel, &a);
        cx.simulate_keystrokes(window, "cmd-n");
        name_it(cx, window, &panel, "a.txt");
        assert_eq!(
            changes.started().last(),
            Some(&Op::Create {
                path: a.clone(),
                dir: false
            })
        );
        changes.land(3, Err(error.into()), cx);
        assert_eq!(typed(&panel, cx).as_deref(), Some("a.txt"));
        cx.simulate_keystrokes(window, "escape");

        // A folder opened, closed and opened again is read each time.
        let folder = Entry {
            name: "docs".into(),
            path: docs.clone(),
            is_dir: true,
        };
        let before = changes.read().len();
        for _ in 0..2 {
            cx.update_window(window, |_, window, cx| {
                panel.update(cx, |p, cx| {
                    p.toggle(&folder, window, cx);
                    p.children.insert(docs.clone(), Vec::new());
                    p.toggle(&folder, window, cx);
                })
            })
            .unwrap();
        }
        assert_eq!(changes.read()[before..], [docs.clone(), docs]);
    }

    /// ⌘Z pressed while a change runs undoes that change, once it has landed (FILES-5). Changes
    /// used to run side by side: ⌘Z right after ⌘D on a big folder undid the change before the
    /// copy, a rename here, and the copy, landing later, then wiped the redo of that rename.
    #[gpui_kit::test]
    fn undo_while_a_change_runs_undoes_that_change(cx: &mut TestAppContext) {
        let dir = Scratch::new("undo-while-a-change-runs");
        let root = dir.0.as_path();
        std::fs::create_dir(root.join("big")).unwrap();
        std::fs::write(root.join("config.yml"), "").unwrap();
        let (window, panel) = open_panel_at(cx, root, &["big", "config.yml"]);
        let changes = Changes::install(&panel, cx);
        let (yml, old) = (root.join("config.yml"), root.join("config.old.yml"));
        let (big, copy) = (root.join("big"), root.join("big copy"));
        let renamed = Op::Move {
            from: old.clone(),
            to: yml.clone(),
        };

        // A rename, landed: its undo is kept.
        let rename = Op::Move {
            from: yml.clone(),
            to: old.clone(),
        };
        ask(cx, window, &panel, rename);
        changes.land(0, Ok(renamed.clone()), cx);

        // ⌘D on the folder, then ⌘Z at once: the ⌘Z waits for the copy.
        select(cx, window, &panel, &big);
        cx.simulate_keystrokes(window, "cmd-d cmd-z");
        let copying = Op::Copy {
            from: big.clone(),
            to: copy.clone(),
        };
        assert_eq!(changes.started()[1..], [copying], "⌘Z waits for the copy");
        assert!(history(&panel, cx).2, "the panel shows a change runs");

        // The copy lands, and the ⌘Z undoes it.
        changes.land(1, Ok(Op::Trash(copy.clone())), cx);
        assert_eq!(changes.started()[2..], [Op::Trash(copy.clone())]);
        let restore = Op::Restore {
            from: PathBuf::from("/Trash/big copy"),
            to: copy.clone(),
        };
        changes.land(2, Ok(restore.clone()), cx);
        assert_eq!(history(&panel, cx), (vec![renamed], vec![restore], false));
    }

    /// ⌘D pressed twice on a folder that takes a while to copy makes two copies, as in Finder:
    /// the second waits for the first, then takes the next free name. Both used to start at
    /// once with the same name, and the second failed with "already exists".
    #[gpui_kit::test]
    fn a_second_duplicate_takes_the_next_free_name(cx: &mut TestAppContext) {
        let dir = Scratch::new("a-second-duplicate");
        let root = dir.0.as_path();
        std::fs::create_dir(root.join("big")).unwrap();
        let (window, panel) = open_panel_at(cx, root, &["big"]);
        let changes = Changes::install(&panel, cx);
        select(cx, window, &panel, &root.join("big"));
        cx.simulate_keystrokes(window, "cmd-d cmd-d");
        let copy = |to: &str| Op::Copy {
            from: root.join("big"),
            to: root.join(to),
        };
        assert_eq!(changes.started(), [copy("big copy")]);
        // The first copy made its folder.
        std::fs::create_dir(root.join("big copy")).unwrap();
        changes.land(0, Ok(Op::Trash(root.join("big copy"))), cx);
        assert_eq!(changes.started()[1..], [copy("big copy 2")]);
    }

    /// An undo that fails stays to try again once what was in its way is dealt with (FILES-5):
    /// it used to leave the history. One whose file is gone, which can never be undone, goes.
    #[gpui_kit::test]
    fn an_undo_that_fails_stays_to_try_again(cx: &mut TestAppContext) {
        let dir = Scratch::new("an-undo-that-fails");
        let root = dir.0.as_path();
        std::fs::write(root.join("b.txt"), "").unwrap();
        let (window, panel) = open_panel_at(cx, root, &["b.txt"]);
        let changes = Changes::install(&panel, cx);
        let gone = Op::Move {
            from: root.join("gone.txt"),
            to: root.join("c.txt"),
        };
        let blocked = Op::Move {
            from: root.join("b.txt"),
            to: root.join("a.txt"),
        };
        cx.update(|cx| panel.update(cx, |p, _| p.undo = vec![gone.clone(), blocked.clone()]));
        select(cx, window, &panel, &root.join("b.txt"));

        cx.simulate_keystrokes(window, "cmd-z");
        changes.land(0, Err("“a.txt” already exists here.".into()), cx);
        let undo = vec![gone.clone(), blocked.clone()];
        assert_eq!(history(&panel, cx), (undo, vec![], false));
        cx.update(|cx| {
            assert_eq!(
                panel.read(cx).error.as_deref(),
                Some("“a.txt” already exists here.")
            )
        });

        // Tried again, and then the one whose file is gone.
        cx.simulate_keystrokes(window, "cmd-z");
        assert_eq!(changes.started().last(), Some(&blocked));
        changes.land(1, Ok(blocked.clone()), cx);
        cx.simulate_keystrokes(window, "cmd-z");
        assert_eq!(changes.started()[2..], [gone]);
        changes.land(2, Err("No such file or directory (os error 2)".into()), cx);
        assert_eq!(history(&panel, cx), (vec![], vec![blocked], false));
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

    /// ⌫ while a name is typed in the tree edits the name and runs none of the panel's commands.
    /// The name box passes on ⌫ with nothing before the cursor, which a new name has at once and
    /// a rename once the start of the name is reached, to the panel's bindings, where Trash would
    /// move the selected entry to the Trash without asking.
    #[gpui_kit::test]
    fn backspace_while_naming_trashes_nothing(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &["notes.txt"]);
        let file = Path::new(ROOT).join("notes.txt");
        cx.update(|cx| panel.update(cx, |p, _| p.selected = Some(file.clone())));
        let (ran, _observer) = file_actions(cx);
        let tree = cx.update(|cx| panel.read(cx).focus.clone());
        let name = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let p = panel.read(cx);
                p.edit
                    .as_ref()
                    .map(|e| e.input.read(cx).value().to_string())
            })
        };

        // New File: the name box starts empty.
        focus(cx, window, &tree);
        cx.simulate_keystrokes(window, "cmd-n");
        assert_eq!(name(cx).as_deref(), Some(""));
        cx.simulate_keystrokes(window, "backspace");
        assert_eq!(*ran.borrow(), ["files::NewFile"], "⌫ in a new name");

        // Rename: the name without its extension is selected; the first ⌫ deletes it, and the
        // second is at the start of the name.
        focus(cx, window, &tree);
        cx.simulate_keystrokes(window, "f2");
        assert_eq!(name(cx).as_deref(), Some("notes.txt"));
        cx.simulate_keystrokes(window, "backspace backspace");
        assert_eq!(name(cx).as_deref(), Some(".txt"));
        assert_eq!(
            *ran.borrow(),
            ["files::NewFile", "files::Rename"],
            "⌫ at the start of a name"
        );

        // From the tree, ⌫ is still Trash, which acts on the selected file, and the right-click
        // menu still finds its key. The Trash itself doesn't run: with nothing selected it acts
        // on nothing (the next test), so this ⌫ starts no file change.
        focus(cx, window, &tree);
        assert_eq!(name(cx), None, "leaving the name box ends the edit");
        cx.update(|cx| assert_eq!(panel.read(cx).trash_target(), Some(file.clone())));
        cx.update(|cx| panel.update(cx, |p, _| p.selected = None));
        cx.simulate_keystrokes(window, "backspace");
        assert_eq!(
            *ran.borrow(),
            ["files::NewFile", "files::Rename", "files::Trash"]
        );
        let key = cx
            .update_window(window, |_, window, _| {
                window
                    .highest_precedence_binding_for_action_in(&Trash as &dyn Action, &tree)
                    .map(|b| b.keystrokes()[0].key().to_string())
            })
            .unwrap();
        assert_eq!(key.as_deref(), Some("backspace"));
    }

    /// While a name is typed, the tree's keys leave it alone (FILES-13). F2 started a rename
    /// over it, which dropped the name typed; ⌘N and ⌥⌘N started the name again; ⌘D duplicated
    /// the selected entry and ⌃⇧↩ opened it, the name box still open.
    #[gpui_kit::test]
    fn keys_while_a_name_is_typed_run_no_tree_command(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &["notes.txt"]);
        let changes = Changes::install(&panel, cx);
        select(cx, window, &panel, &Path::new(ROOT).join("notes.txt"));
        let (ran, _observer) = file_actions(cx);
        cx.simulate_keystrokes(window, "cmd-n");
        cx.update_window(window, |_, window, cx| {
            let input = panel.read(cx).edit.as_ref().unwrap().input.clone();
            input.update(cx, |s, cx| s.set_value("draft", window, cx));
        })
        .unwrap();
        cx.simulate_keystrokes(window, "f2 cmd-n alt-cmd-n cmd-d");
        assert_eq!(*ran.borrow(), ["files::NewFile"]);
        // ⌃⇧↩ would open the file in another app, which a test can't.
        cx.simulate_keystrokes(
            window,
            "ctrl-shift-enter alt-cmd-r alt-cmd-c alt-cmd-shift-f",
        );
        assert_eq!(*ran.borrow(), ["files::NewFile"]);
        assert_eq!(changes.started(), []);
        assert_eq!(typed(&panel, cx).as_deref(), Some("draft"));
        cx.update(|cx| {
            let edit = panel.read(cx).edit.as_ref().map(|e| e.kind.clone());
            assert!(matches!(edit, Some(super::EditKind::New { dir: false })));
        });
    }

    /// ⌫ in the tree with nothing selected moves nothing to the Trash. Trash used to fall back
    /// to the project folder, as the commands that show, open or copy a path do, and moved the
    /// whole project to the Trash without asking. Delete, the other command that removes, has
    /// always needed a selection.
    #[gpui_kit::test]
    fn backspace_with_nothing_selected_leaves_the_project_folder(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &["notes.txt"]);
        let (ran, _observer) = file_actions(cx);
        let tree = cx.update(|cx| panel.read(cx).focus.clone());
        focus(cx, window, &tree);
        cx.update(|cx| assert_eq!(panel.read(cx).trash_target(), None));
        // A Trash this starts fails the test at once: open_panel installs no runtime.
        cx.simulate_keystrokes(window, "backspace");
        assert_eq!(*ran.borrow(), ["files::Trash"]);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.root.as_deref(), Some(Path::new(ROOT)));
            assert!(p.error.is_none() && p.undo.is_empty());
        });
    }

    /// ⌫ after the panel changes folder (its Open Project Folder button, or `sawit -r`) moves
    /// nothing to the Trash, and ⌘V pastes nothing. No row of the new folder's tree shows the
    /// previous folder's selected or cut entry, and Trash and Paste used to act on them all the
    /// same. The new tree also starts at its top, with no error from the previous folder.
    #[gpui_kit::test]
    fn backspace_after_a_folder_change_trashes_nothing(cx: &mut TestAppContext) {
        let (window, panel) = open_panel(cx, &["notes.txt"]);
        let file = Path::new(ROOT).join("notes.txt");
        cx.update(|cx| {
            panel.update(cx, |p, _| {
                p.selected = Some(file.clone());
                p.clip = Some(Clip {
                    path: file.clone(),
                    cut: true,
                });
                p.error = Some("“notes.txt” couldn't be renamed.".into());
                p.scroll
                    .0
                    .borrow()
                    .base_handle
                    .set_offset(point(px(0.), px(-66.)));
            })
        });
        // What set_root does, but for reading the new folder, which needs the runtime that
        // open_panel leaves out.
        let offset = cx.update(|cx| {
            panel.update(cx, |p, cx| {
                p.forget_folder();
                // Read before a frame is drawn: drawing a tree shorter than the panel also
                // brings it to the top, and a longer one keeps the offset.
                let offset = p.scroll.0.borrow().base_handle.offset();
                show(p, Path::new(OTHER_ROOT), &["readme.md"]);
                cx.notify();
                offset
            })
        });
        let (ran, _observer) = file_actions(cx);
        let tree = cx.update(|cx| panel.read(cx).focus.clone());
        focus(cx, window, &tree);
        // A Trash or a Paste this starts fails the test at once: there is no runtime.
        cx.simulate_keystrokes(window, "backspace cmd-v");
        assert_eq!(*ran.borrow(), ["files::Trash", "files::Paste"]);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.root.as_deref(), Some(Path::new(OTHER_ROOT)));
            assert_eq!(p.selected, None);
            assert!(p.clip.is_none() && p.undo.is_empty());
            assert_eq!(p.error, None);
        });
        assert_eq!(offset, Point::default(), "the new tree starts at its top");
    }
}
