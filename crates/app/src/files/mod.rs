//! Project file tree (left panel), loaded lazily per folder.

pub(crate) mod ops;
pub mod search;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::*;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
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

/// Zed's project panel bindings for the same commands.
pub fn init(cx: &mut App) {
    let ctx = Some("FilesPanel");
    cx.bind_keys([
        KeyBinding::new("cmd-n", NewFile, ctx),
        KeyBinding::new("alt-cmd-n", NewFolder, ctx),
        KeyBinding::new("alt-cmd-r", RevealInFinder, ctx),
        KeyBinding::new("ctrl-shift-enter", OpenInDefaultApp, ctx),
        KeyBinding::new("alt-cmd-shift-f", FindInFolder, ctx),
        KeyBinding::new("cmd-x", Cut, ctx),
        KeyBinding::new("cmd-c", CopyEntry, ctx),
        KeyBinding::new("cmd-d", Duplicate, ctx),
        KeyBinding::new("cmd-v", Paste, ctx),
        KeyBinding::new("cmd-z", Undo, ctx),
        KeyBinding::new("cmd-shift-z", Redo, ctx),
        KeyBinding::new("alt-cmd-c", CopyPath, ctx),
        KeyBinding::new("alt-cmd-shift-c", CopyRelativePath, ctx),
        KeyBinding::new("f2", Rename, ctx),
        KeyBinding::new("backspace", Trash, ctx),
        KeyBinding::new("alt-cmd-backspace", Delete, ctx),
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

#[derive(Clone, Copy)]
enum Step {
    Do,
    Undo,
    Redo,
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
        self.expanded.clear();
        self.children.clear();
        self.root_error = None;
        self.repo = None;
        self.undo.clear();
        self.redo.clear();
        self.edit = None;
        self.expanded.insert(root.clone());
        self.load(root.clone(), cx);
        let dir = root.clone();
        let rx = rt::spawn(cx, async move { sawit_core::git::discover(&dir).await });
        cx.spawn(async move |this, cx| {
            if let Ok(repo) = rx.await {
                let _ = this.update(cx, |this, _| this.repo = repo);
            }
        })
        .detach();
        cx.emit(FilesEvent::RootChanged(root.clone()));
        self.root = Some(root);
        cx.notify();
    }

    fn load(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let read = dir.clone();
        let rx = rt::spawn_blocking(cx, move || list_dir(&read));
        cx.spawn(async move |this, cx| {
            match rx.await {
                Ok(Ok(entries)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.root.as_ref() == Some(&dir) && this.root_error.take().is_some() {
                            cx.notify();
                        }
                        if this.children.get(&dir) != Some(&entries) {
                            this.children.insert(dir, entries);
                            cx.notify();
                        }
                    });
                }
                // The folder was deleted or renamed: forget it (or, for the
                // project folder, say so).
                Ok(Err(e)) => {
                    let _ = this.update(cx, |this, cx| {
                        if this.root.as_ref() == Some(&dir) {
                            this.root_error = Some(e.to_string());
                        }
                        this.children.remove(&dir);
                        this.expanded.remove(&dir);
                        cx.notify();
                    });
                }
                Err(_) => {}
            }
        })
        .detach();
    }

    fn toggle(&mut self, entry: &Entry, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.selected = Some(entry.path.clone());
        if entry.is_dir {
            if !self.expanded.remove(&entry.path) {
                self.expanded.insert(entry.path.clone());
                if !self.children.contains_key(&entry.path) {
                    self.load(entry.path.clone(), cx);
                }
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
            .on_action(cx.listener(|this, _: &Duplicate, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &Paste, _, cx| this.paste(cx)))
            .on_action(cx.listener(|this, _: &Undo, _, cx| this.step(Step::Undo, cx)))
            .on_action(cx.listener(|this, _: &Redo, _, cx| this.step(Step::Redo, cx)))
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
            .on_action(cx.listener(|this, _: &Trash, _, cx| {
                if let Some(path) = this.target() {
                    this.run(Op::Trash(path), Step::Do, cx);
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

    /// The selected entry, else the project folder.
    fn target(&self) -> Option<PathBuf> {
        self.selected.clone().or_else(|| self.root.clone())
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
        let Some(path) = self.selected.clone() else {
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
        self.cancel_edit(window, cx);
        if let Some(op) = op {
            self.run(op, Step::Do, cx);
        }
    }

    /// Runs `op` off the UI thread, keeping its inverse for undo (or redo).
    fn run(&mut self, op: Op, step: Step, cx: &mut Context<Self>) {
        let moved = match &op {
            Op::Move { from, to } => Some((from.clone(), to.clone())),
            _ => None,
        };
        let open = matches!((&op, step), (Op::Create { dir: false, .. }, Step::Do));
        let result = op.result().map(Path::to_path_buf);
        let rx = rt::spawn_blocking(cx, move || op.run());
        cx.spawn(async move |this, cx| {
            let Ok(outcome) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match outcome {
                    Ok(inverse) => {
                        match step {
                            Step::Do => {
                                this.undo.push(inverse);
                                this.redo.clear();
                            }
                            Step::Undo => this.redo.push(inverse),
                            Step::Redo => this.undo.push(inverse),
                        }
                        this.error = None;
                        if let Some((from, to)) = moved {
                            cx.emit(FilesEvent::Moved { from, to });
                        }
                        if let Some(path) = result {
                            if let Some(parent) = path.parent() {
                                this.expand_to(parent, cx);
                            }
                            if open {
                                cx.emit(FilesEvent::OpenFile(path.clone()));
                            }
                            this.selected = Some(path);
                        }
                    }
                    Err(e) => this.error = Some(e),
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn step(&mut self, step: Step, cx: &mut Context<Self>) {
        let stack = match step {
            Step::Undo => &mut self.undo,
            _ => &mut self.redo,
        };
        if let Some(op) = stack.pop() {
            self.run(op, step, cx);
        }
    }

    fn pick(&mut self, cut: bool, cx: &mut Context<Self>) {
        if let Some(path) = self.selected.clone() {
            self.clip = Some(Clip { path, cut });
            cx.notify();
        }
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
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
                self.clip = None;
                self.run(
                    Op::Move {
                        from: clip.path,
                        to,
                    },
                    Step::Do,
                    cx,
                );
            }
        } else {
            let to = files::free_name(&dir, &name, clip.path.is_dir());
            self.run(
                Op::Copy {
                    from: clip.path,
                    to,
                },
                Step::Do,
                cx,
            );
        }
    }

    fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(from) = self.selected.clone() else {
            return;
        };
        let (Some(dir), Some(name)) = (from.parent(), from.file_name()) else {
            return;
        };
        let to = files::free_name(dir, &name.to_string_lossy(), from.is_dir());
        self.run(Op::Copy { from, to }, Step::Do, cx);
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
    /// `.git/info/exclude`.
    fn ignore(&mut self, exclude: bool, cx: &mut Context<Self>) {
        let (Some(repo), Some(path)) = (self.repo.clone(), self.target()) else {
            return;
        };
        let rx = rt::spawn_blocking(cx, move || {
            // `git` reports the repository's real path; the tree may not.
            let real = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            let rel = real
                .strip_prefix(&repo)
                .map_err(|_| "This isn't inside the repository.".to_string())?;
            if rel.as_os_str().is_empty() {
                return Err("The repository's own folder can't be ignored.".into());
            }
            let file = if exclude {
                repo.join(".git/info/exclude")
            } else {
                repo.join(".gitignore")
            };
            files::add_ignore_line(&file, &files::ignore_pattern(rel, path.is_dir()))
                .map_err(|e| e.to_string())
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(e);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Deletes the selected entry for good, after asking.
    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
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
        cx.spawn(async move |this, cx| {
            if !matches!(answer.await, Ok(0)) {
                return;
            }
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn_blocking(cx, move || files::remove(&path).map_err(|e| e.to_string()))
            }) else {
                return;
            };
            let outcome = rx.await;
            let _ = this.update(cx, |this, cx| {
                if let Ok(Err(e)) = outcome {
                    this.error = Some(e);
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
            .selected
            .clone()
            .filter(|p| p.is_dir())
            .or_else(|| self.root.clone())
        else {
            return;
        };
        self.expand_to(&dir, cx);
        let rx = rt::spawn_blocking(cx, move || read_tree(dir));
        cx.spawn(async move |this, cx| {
            let Ok(tree) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                for (dir, entries) in tree {
                    this.expanded.insert(dir.clone());
                    this.children.insert(dir, entries);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.expanded.retain(|d| Some(d) == root.as_ref());
        cx.notify();
    }
}
