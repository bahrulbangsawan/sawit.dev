//! The main window, laid out like Zed: title bar, left panel with tabs
//! (Instances / Files / Git / Database / Run / Ports / Network / Prompts / SSH), tabbed center
//! pane, bottom terminal dock and status bar. A new, empty window starts on the welcome screen instead.
//! One window holds several projects (see `project`): the title bar switches
//! between them and tabs are grouped by the project they belong to.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::board::BoardView;
use crate::browser::BrowserView;
use crate::canvas::{CanvasView, ToggleCanvasMaximize};
use crate::changelog_page::ChangelogPage;
use crate::cli::{CliStatus, cli_status};
use crate::db::{DatabasePanel, DbEvent, DbStore, DiagramView, SqlView, TableView};
use crate::editor::{FileEditor, FileEditorEvent};
use crate::files::search::{FolderSearch, SearchEvent};
use crate::files::{FilesEvent, FilesPanel};
use crate::git::{
    CommitView, DiffView, DiffViewEvent, GitHubView, GitPanel, GitPanelEvent, GitStore, GraphEvent,
    GraphView,
};
use crate::layout::{Area, Layout};
use crate::mcp_page::McpPage;
use crate::network::NetworkPanel;
use crate::ports::{PortsEvent, PortsPanel};
use crate::prompts::{PromptStore, PromptStoreEvent, PromptView, PromptsEvent, PromptsPanel};
use crate::run::{RunEvent, RunPanel, RunStore};
use crate::servers::{ServerDetail, ServerList, ServerStore, ShowServerTab};
use crate::shell_config::{ShellConfigEvent, ShellConfigPanel};
use crate::ssh::{SshEvent, SshHostEvent, SshHostView, SshPanel, SshStore, SshStoreEvent};
use crate::states::Placeholder;
use crate::tab_groups::{group_header, group_stripe, project_name};
use crate::tabs::{
    CloseOp, Slot, TabFlags, active_after, active_after_move, insert_index, to_close,
};
use crate::terminal::{TerminalPanel, ToggleMaximize};
use crate::tools_page::{ToolsEvent, ToolsPage};
use crate::update::{Updater, updater};
use crate::viewers::{ImageView, PdfView, VideoView};
use crate::welcome::{Welcome, WelcomeEvent};
use sawit_board::store::{self, board_file};
use sawit_core::files::{FileKind, classify};
use sawit_core::instances::AppPaths;
use sawit_core::recent::RecentProjects;
use sawit_core::update::Version;

mod control;
mod project;

pub use control::Answer;
use project::Project;

actions!(
    workspace,
    [
        CloseActiveTab,
        CloseOtherTabs,
        CloseTabsLeft,
        CloseTabsRight,
        CloseCleanTabs,
        CloseAllTabs,
        TogglePinTab,
        ToggleSidebar,
        ToggleTerminal,
        ToggleCenter,
        ToggleMaximizePane,
        NextTab,
        PreviousTab,
        NextProject,
        PreviousProject,
        AddProject,
        CloseProject,
        CopyProjectPath,
        NewTerminal,
        ShowInstances,
        ShowFiles,
        ShowGit,
        ShowDatabase,
        ShowRun,
        ShowPorts,
        ShowNetwork,
        ShowSsh,
        ShowShellConfig,
        ShowPrompts,
        OpenCanvas,
        OpenDevTools,
        OpenMcp,
        OpenChangelog,
        InstallUpdate
    ]
);

/// Every workspace shortcut: Zed's defaults where Zed has the command; the
/// pages in the title bar are on ⌃⌘, which nothing else here (inputs, the
/// terminal, the canvas) uses. No two share a keystroke (see the tests).
fn bindings() -> Vec<KeyBinding> {
    let ctx = Some("Workspace");
    vec![
        KeyBinding::new("cmd-w", CloseActiveTab, ctx),
        KeyBinding::new("alt-cmd-t", CloseOtherTabs, ctx),
        KeyBinding::new("cmd-k e", CloseTabsLeft, ctx),
        KeyBinding::new("cmd-k t", CloseTabsRight, ctx),
        KeyBinding::new("cmd-k u", CloseCleanTabs, ctx),
        KeyBinding::new("cmd-k w", CloseAllTabs, ctx),
        KeyBinding::new("cmd-k shift-enter", TogglePinTab, ctx),
        KeyBinding::new("ctrl-tab", NextTab, ctx),
        KeyBinding::new("ctrl-shift-tab", PreviousTab, ctx),
        KeyBinding::new("cmd-b", ToggleSidebar, ctx),
        KeyBinding::new("cmd-j", ToggleTerminal, ctx),
        KeyBinding::new("cmd-shift-j", ToggleCenter, ctx),
        KeyBinding::new("cmd-shift-enter", ToggleMaximizePane, ctx),
        KeyBinding::new("cmd-t", NewTerminal, ctx),
        KeyBinding::new("cmd-1", ShowInstances, ctx),
        KeyBinding::new("cmd-2", ShowFiles, ctx),
        KeyBinding::new("cmd-3", ShowGit, ctx),
        KeyBinding::new("cmd-4", ShowDatabase, ctx),
        KeyBinding::new("cmd-5", ShowRun, ctx),
        KeyBinding::new("cmd-6", ShowPorts, ctx),
        KeyBinding::new("cmd-7", ShowNetwork, ctx),
        KeyBinding::new("cmd-8", ShowSsh, ctx),
        KeyBinding::new("cmd-9", ShowShellConfig, ctx),
        KeyBinding::new("cmd-0", ShowPrompts, ctx),
        KeyBinding::new("ctrl-cmd-]", NextProject, ctx),
        KeyBinding::new("ctrl-cmd-[", PreviousProject, ctx),
        KeyBinding::new("alt-cmd-o", AddProject, ctx),
        KeyBinding::new("ctrl-cmd-w", CloseProject, ctx),
        KeyBinding::new("ctrl-cmd-p", CopyProjectPath, ctx),
        KeyBinding::new("ctrl-cmd-c", OpenCanvas, ctx),
        KeyBinding::new("ctrl-cmd-t", OpenDevTools, ctx),
        KeyBinding::new("ctrl-cmd-m", OpenMcp, ctx),
        KeyBinding::new("ctrl-cmd-v", OpenChangelog, ctx),
        KeyBinding::new("ctrl-cmd-u", InstallUpdate, ctx),
        KeyBinding::new("ctrl-cmd-i", crate::cli::InstallCli, ctx),
    ]
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
    crate::browser::init(cx);
    crate::board::init(cx);
}

/// The workspace shortcuts the canvas passes to the app while it has the
/// keyboard, written as the page writes keys (`ctrl-alt-shift-cmd-key`).
/// Not the ⌘K chords or ⌘0, which Excalidraw uses (link, reset zoom), nor
/// ⇧⌘↩, which the page handles itself.
pub fn canvas_passthrough_keys() -> Vec<String> {
    bindings()
        .iter()
        .filter_map(|b| match b.keystrokes() {
            [k] => Some(page_key(k.modifiers(), k.key())),
            _ => None,
        })
        .filter(|k| k != "cmd-0" && k != "shift-cmd-enter")
        .chain(["cmd-o", "shift-cmd-n", "cmd-q"].map(String::from))
        .collect()
}

fn page_key(m: &Modifiers, key: &str) -> String {
    let mut out = String::new();
    for (on, name) in [
        (m.control, "ctrl-"),
        (m.alt, "alt-"),
        (m.shift, "shift-"),
        (m.platform, "cmd-"),
    ] {
        if on {
            out.push_str(name);
        }
    }
    out + key
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Servers,
    Files,
    Git,
    Database,
    Run,
    Ports,
    Network,
    Prompts,
    Ssh,
    ShellConfig,
}

#[derive(Clone)]
struct Tab {
    item: PaneItem,
    pinned: bool,
    /// The project whose group the tab is in; `None` for tabs of no
    /// project (instances, databases, prompts…).
    group: Option<PathBuf>,
}

#[derive(Clone)]
enum PaneItem {
    Server,
    File(Entity<FileEditor>),
    Other {
        view: AnyView,
        /// Identity for "already open?" checks.
        key: SharedString,
        title: SharedString,
        icon: Lucide,
        /// The file shown, for its language logo.
        file: Option<PathBuf>,
    },
}

/// The canvas [`Workspace::open_canvas`] opened or found: the Excalidraw page, or the native
/// canvas while `SAWIT_NATIVE_CANVAS` is set (the Excalidraw page goes once the native canvas
/// replaces it).
pub enum CanvasTab {
    Web(Entity<CanvasView>),
    Native(Entity<BoardView>),
}

impl CanvasTab {
    fn view(&self) -> AnyView {
        match self {
            CanvasTab::Web(view) => view.clone().into(),
            CanvasTab::Native(view) => view.clone().into(),
        }
    }
}

pub struct Workspace {
    servers: Entity<ServerStore>,
    server_list: Entity<ServerList>,
    server_detail: Entity<ServerDetail>,
    /// Never empty; the first may have no folder yet.
    projects: Vec<Project>,
    /// The active project, whose panels the sidebar shows.
    project: usize,
    run_store: Entity<RunStore>,
    db: Entity<DbStore>,
    db_panel: Entity<DatabasePanel>,
    prompt_store: Entity<PromptStore>,
    prompts_panel: Entity<PromptsPanel>,
    ports_panel: Entity<PortsPanel>,
    network_panel: Entity<NetworkPanel>,
    ssh_store: Entity<SshStore>,
    ssh_panel: Entity<SshPanel>,
    shell_config_panel: Entity<ShellConfigPanel>,
    sql_count: usize,
    /// The next SQL tab opened runs its SQL right away (`open_sql` with `run`).
    run_next_sql: bool,
    terminal: Entity<TerminalPanel>,
    sidebar: SidebarTab,
    tabs: Vec<Tab>,
    active: usize,
    /// Groups whose tabs are folded away, by project.
    collapsed: HashSet<PathBuf>,
    layout: Layout,
    /// The status bar's copy-path button is showing its check mark.
    path_copied: bool,
    _path_copied_reset: Task<()>,
    /// Layout to go back to when the maximized terminal or canvas is restored.
    unmaximized: Option<Layout>,
    /// The canvas tabs (owned by their tabs), one per project at most.
    canvases: Vec<WeakEntity<CanvasView>>,
    /// The native canvas tabs (owned by their tabs), one per project at most. `canvases` holds
    /// the Excalidraw ones.
    boards: Vec<WeakEntity<BoardView>>,
    /// The canvas tab is the native canvas instead of the Excalidraw page: the environment
    /// variable `SAWIT_NATIVE_CANVAS` is set (read once, when the window opens).
    native_canvas: bool,
    /// The canvas fills the window: no sidebar, terminal or tab bar.
    canvas_maximized: bool,
    /// The browser tab (owned by its tab), while open.
    browser: Option<WeakEntity<BrowserView>>,
    /// Shown instead of the panels until something is opened.
    welcome: Option<Entity<Welcome>>,
    tab_scroll: ScrollHandle,
    /// Active tab last scrolled into view.
    scrolled_to: Option<usize>,
    /// A table/SQL tab asked for while its connection was being re-made.
    pending_db: Option<DbEvent>,
    cli_status: Entity<CliStatus>,
    updater: Entity<Updater>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let servers = cx.new(ServerStore::new);
        let terminal = cx.new(TerminalPanel::new);
        let db = cx.new(DbStore::new);
        let db_panel = cx.new(|cx| DatabasePanel::new(db.clone(), servers.clone(), window, cx));
        let prompt_store = cx.new(PromptStore::new);
        let prompts_panel = cx.new(|cx| PromptsPanel::new(prompt_store.clone(), window, cx));
        let ports_panel = cx.new(|cx| PortsPanel::new(servers.clone(), window, cx));
        let run_store = cx.new(RunStore::new);
        let project = Self::new_project(&run_store, &terminal, &ports_panel, window, cx);
        let network_panel = cx.new(|cx| NetworkPanel::new(window, cx));
        let ssh_store = cx.new(SshStore::new);
        let ssh_panel = cx.new(|cx| SshPanel::new(ssh_store.clone(), terminal.clone(), window, cx));
        let shell_config_panel = cx.new(ShellConfigPanel::new);
        let cli_status = cli_status(cx);
        let updater = updater(cx);
        let welcome = cx.new(|cx| Welcome::new(servers.clone(), db.clone(), cx));
        let subscriptions = vec![
            cx.observe(&servers, |_, _, cx| cx.notify()),
            cx.observe(&cli_status, |_, _, cx| cx.notify()),
            cx.observe(&updater, |_, _, cx| cx.notify()),
            cx.subscribe_in(
                &welcome,
                window,
                |this, _, event: &WelcomeEvent, window, cx| {
                    this.on_welcome_event(event, window, cx)
                },
            ),
            cx.subscribe_in(
                &ports_panel,
                window,
                |this, _, event: &PortsEvent, window, cx| match event {
                    PortsEvent::OpenInBrowser(url) => {
                        this.open_browser(Some(url.clone()), window, cx)
                    }
                },
            ),
            cx.subscribe(&servers, |this, _, _: &ShowServerTab, cx| {
                this.show_server_tab();
                cx.notify();
            }),
            cx.subscribe_in(
                &terminal,
                window,
                |this, _, _: &ToggleMaximize, window, cx| {
                    this.toggle_terminal_maximized(window, cx)
                },
            ),
            cx.observe_in(&db, window, |this, db, window, cx| {
                let ready = match &this.pending_db {
                    Some(event) => {
                        let profile = event.profile();
                        let store = db.read(cx);
                        if store.state(profile).is_pending() {
                            return;
                        }
                        store.is_connected(profile)
                    }
                    None => return,
                };
                // Replay once connected; drop it if reconnecting failed.
                if let Some(event) = this.pending_db.take()
                    && ready
                {
                    this.on_db_event(&event, window, cx);
                }
            }),
            cx.subscribe_in(&db_panel, window, |this, _, event: &DbEvent, window, cx| {
                this.on_db_event(event, window, cx)
            }),
            cx.subscribe_in(
                &prompts_panel,
                window,
                |this, _, PromptsEvent::Open(id), window, cx| {
                    this.open_prompt(id.clone(), window, cx)
                },
            ),
            // Prompt tabs follow renames and close when deleted.
            cx.observe(&prompt_store, |this, store, cx| {
                let library = &store.read(cx).library;
                for tab in &mut this.tabs {
                    if let PaneItem::Other { key, title, .. } = &mut tab.item
                        && let Some(id) = key.strip_prefix("prompt\u{0}")
                        && let Some(p) = library.get(id)
                    {
                        let name = if p.title.trim().is_empty() {
                            "(untitled)"
                        } else {
                            p.title.as_str()
                        };
                        if title.as_ref() != name {
                            *title = name.to_string().into();
                        }
                    }
                }
                cx.notify();
            }),
            cx.subscribe_in(
                &ssh_panel,
                window,
                |this, _, event: &SshEvent, window, cx| match event {
                    SshEvent::Connect { key, new } => {
                        this.connect_ssh(key.clone(), *new, window, cx)
                    }
                    SshEvent::Edit(id) => this.open_ssh_host(id.clone(), window, cx),
                    SshEvent::OpenFile(path) => {
                        this.show_center(cx);
                        this.open_file(path.clone(), window, cx)
                    }
                },
            ),
            cx.subscribe_in(
                &shell_config_panel,
                window,
                |this, _, event: &ShellConfigEvent, window, cx| match event {
                    ShellConfigEvent::OpenFile(path) => {
                        this.show_center(cx);
                        this.open_file(path.clone(), window, cx)
                    }
                    ShellConfigEvent::OpenTerminal(dir) => {
                        this.open_terminal_in(dir.clone(), window, cx)
                    }
                    ShellConfigEvent::SignInToGitHub => this.open_github(window, cx),
                },
            ),
            // Server tabs follow renames and close when deleted.
            cx.observe(&ssh_store, |this, store, cx| {
                let list = &store.read(cx).list;
                for tab in &mut this.tabs {
                    if let PaneItem::Other { key, title, .. } = &mut tab.item
                        && let Some(id) = key.strip_prefix("ssh-host\u{0}")
                        && let Some(h) = list.get(id)
                    {
                        let name = h.label();
                        if title.as_ref() != name {
                            *title = name.into();
                        }
                    }
                }
                cx.notify();
            }),
            cx.subscribe(&ssh_store, |this, _, event: &SshStoreEvent, cx| {
                let SshStoreEvent::Removed(id) = event;
                let key = format!("ssh-host\u{0}{id}");
                if let Some(ix) = this.tabs.iter().position(
                    |t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key),
                ) {
                    this.remove_tabs(&[ix], cx);
                }
            }),
            cx.subscribe(&prompt_store, |this, _, event: &PromptStoreEvent, cx| {
                let PromptStoreEvent::Removed(id) = event;
                let key = format!("prompt\u{0}{id}");
                if let Some(ix) = this.tabs.iter().position(
                    |t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key),
                ) {
                    this.remove_tabs(&[ix], cx);
                }
            }),
        ];
        Self {
            server_list: cx.new(|cx| ServerList::new(servers.clone(), cx)),
            server_detail: cx.new(|cx| ServerDetail::new(servers.clone(), window, cx)),
            servers,
            projects: vec![project],
            project: 0,
            run_store,
            db,
            db_panel,
            prompt_store,
            prompts_panel,
            ports_panel,
            network_panel,
            ssh_store,
            ssh_panel,
            shell_config_panel,
            sql_count: 0,
            run_next_sql: false,
            terminal,
            sidebar: SidebarTab::Servers,
            tabs: Vec::new(),
            active: 0,
            collapsed: HashSet::new(),
            layout: Layout::default(),
            path_copied: false,
            _path_copied_reset: Task::ready(()),
            unmaximized: None,
            canvases: Vec::new(),
            boards: Vec::new(),
            native_canvas: std::env::var_os("SAWIT_NATIVE_CANVAS").is_some(),
            canvas_maximized: false,
            browser: None,
            welcome: Some(welcome),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            pending_db: None,
            focus: cx.focus_handle(),
            cli_status,
            updater,
            _subscriptions: subscriptions,
        }
    }

    /// The active project's folder, if it has one.
    pub fn root(&self, cx: &App) -> Option<PathBuf> {
        self.current().root(cx)
    }

    /// Files shown in tabs (editors and viewers), for `sawit --wait`.
    pub fn open_files(&self, cx: &App) -> Vec<PathBuf> {
        self.tabs
            .iter()
            .filter_map(|t| match &t.item {
                PaneItem::File(e) => Some(e.read(cx).path.clone()),
                PaneItem::Other { file, .. } => file.clone(),
                PaneItem::Server => None,
            })
            .collect()
    }

    /// Opens `path` (from the CLI) with the cursor on a 1-based line and
    /// column, bringing the tabs back if they were hidden.
    pub fn open_file_at(
        &mut self,
        path: PathBuf,
        position: Option<(u32, u32)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        self.open_file(path, window, cx);
        if let Some((line, column)) = position
            && let Some(PaneItem::File(editor)) = self.tabs.get(self.active).map(|t| t.item.clone())
        {
            editor.update(cx, |e, cx| e.go_to(line, column, window, cx));
        }
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let kind = classify(&path);
        // Canvas files open on the native canvas. Without it they open as any other file does.
        if self.native_canvas && matches!(kind, FileKind::Figcraft | FileKind::Excalidraw) {
            self.open_board_file(path, kind, cx);
            return;
        }
        let viewer = match kind {
            FileKind::Image => Some(Lucide::Image),
            FileKind::Pdf => Some(Lucide::FileText),
            FileKind::Video => Some(Lucide::Film),
            _ => None,
        };
        if let Some(icon) = viewer {
            let key: SharedString = format!("file\u{0}{}", path.display()).into();
            if let Some(ix) = self
                .tabs
                .iter()
                .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
            {
                self.active = ix;
                cx.notify();
                return;
            }
            let title: SharedString = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into();
            let path_for_icon = path.clone();
            let view: AnyView = match kind {
                FileKind::Image => cx.new(|cx| ImageView::new(path, cx)).into(),
                FileKind::Pdf => cx.new(|cx| PdfView::new(path, cx)).into(),
                _ => cx.new(|cx| VideoView::new(path, window, cx)).into(),
            };
            let group = self.group_of(&path_for_icon, cx);
            self.push_tab(
                PaneItem::Other {
                    view,
                    key,
                    title,
                    icon,
                    file: Some(path_for_icon),
                },
                group,
                cx,
            );
            cx.notify();
            return;
        }
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::File(e) if e.read(cx).path == path))
        {
            self.activate_tab(ix, cx);
        } else {
            let group = self.group_of(&path, cx);
            let editor = cx.new(|cx| FileEditor::open(path, group.clone(), window, cx));
            cx.subscribe(&editor, |_, _, _: &FileEditorEvent, cx| cx.notify())
                .detach();
            self.push_tab(PaneItem::File(editor), group, cx);
        }
        cx.notify();
    }

    /// Opens a canvas file in a canvas tab (spec D14): a `.figcraft` file bound to itself, or an
    /// `.excalidraw` file imported and bound to the `<stem>.figcraft` beside it
    /// ([`BoardView::import`]).
    ///
    /// A file has one canvas in the window, whichever of its two names opens it, because two
    /// canvases of one file would each save over the other's edits. A project's canvas file is
    /// that project's canvas tab ([`Self::open_canvas_of`]); any other file's tab is keyed by the
    /// `.figcraft` file it is bound to, and shown again when the file is opened again.
    fn open_board_file(&mut self, path: PathBuf, kind: FileKind, cx: &mut Context<Self>) {
        let file = store::bound_file(&path);
        let roots: Vec<Option<PathBuf>> = self.projects.iter().map(|p| p.root(cx)).collect();
        if let Some(root) = canvas_project(&file, roots, &AppPaths::default_macos().root) {
            self.open_canvas_of(root, cx);
            return;
        }
        if let Some((ix, _)) = self.board_tab(&file, cx) {
            self.activate_tab(ix, cx);
            return;
        }
        let view = cx.new(|cx| match kind {
            FileKind::Excalidraw => BoardView::import(path, cx),
            _ => BoardView::open(path, cx),
        });
        cx.subscribe(&view, |this, _, _: &ToggleCanvasMaximize, cx| {
            this.toggle_canvas_maximized(cx)
        })
        .detach();
        let title: SharedString = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
        let group = self.group_of(&file, cx);
        self.push_tab(
            PaneItem::Other {
                view: view.into(),
                key: format!("file\u{0}{}", file.display()).into(),
                title,
                icon: crate::canvas::ICON,
                file: Some(file),
            },
            group,
            cx,
        );
        cx.notify();
    }

    /// The tab of the canvas bound to the `.figcraft` file `file` ([`BoardView::path`]), and the
    /// canvas, whatever opened it: a project's canvas tab, or a canvas file's.
    fn board_tab(&self, file: &Path, cx: &App) -> Option<(usize, Entity<BoardView>)> {
        self.tabs.iter().enumerate().find_map(|(ix, tab)| {
            let PaneItem::Other { view, .. } = &tab.item else {
                return None;
            };
            let board = view.clone().downcast::<BoardView>().ok()?;
            (board.read(cx).path() == file).then_some((ix, board))
        })
    }

    /// Opens an arbitrary view as a center tab (used by the database tools).
    pub fn open_item(
        &mut self,
        view: AnyView,
        key: SharedString,
        title: SharedString,
        icon: Lucide,
        cx: &mut Context<Self>,
    ) {
        self.open_item_in(view, key, title, icon, None, cx);
    }

    /// Like [`Self::open_item`], in the group of the project at `group`.
    fn open_item_in(
        &mut self,
        view: AnyView,
        key: SharedString,
        title: SharedString,
        icon: Lucide,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let item = PaneItem::Other {
            view,
            key,
            title,
            icon,
            file: None,
        };
        self.push_tab(item, group, cx);
        cx.notify();
    }

    fn slots(&self) -> Vec<Slot<'_>> {
        self.tabs
            .iter()
            .map(|t| Slot {
                pinned: t.pinned,
                group: t.group.as_deref(),
            })
            .collect()
    }

    /// Adds a tab to the end of its project's group and shows it.
    fn push_tab(&mut self, item: PaneItem, group: Option<PathBuf>, cx: &mut Context<Self>) {
        let at = insert_index(&self.slots(), false, group.as_deref());
        self.tabs.insert(
            at,
            Tab {
                item,
                pinned: false,
                group,
            },
        );
        self.activate_tab(at, cx);
    }

    /// Shows the tab at `ix`, unfolding its group and switching to its
    /// project.
    fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix;
        if let Some(root) = self.tabs.get(ix).and_then(|t| t.group.clone()) {
            self.collapsed.remove(&root);
            if let Some(p) = self
                .projects
                .iter()
                .position(|p| p.root(cx) == Some(root.clone()))
            {
                self.activate_project(p, cx);
            }
        }
        cx.notify();
    }

    /// Moves the tab at `ix` into `group` (or out of any), keeping groups
    /// together.
    fn move_to_group(&mut self, ix: usize, group: Option<PathBuf>, cx: &mut Context<Self>) {
        if ix >= self.tabs.len() || self.tabs[ix].group == group {
            return;
        }
        let mut tab = self.tabs.remove(ix);
        tab.group = group;
        let at = insert_index(&self.slots(), tab.pinned, tab.group.as_deref());
        self.tabs.insert(at, tab);
        self.active = active_after_move(self.active, ix, at);
        cx.notify();
    }

    fn toggle_group(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&root) {
            self.collapsed.insert(root);
        }
        cx.notify();
    }

    /// Closes a group's tabs, like Close All: pinned and unsaved ones stay.
    fn close_group(&mut self, root: &Path, cx: &mut Context<Self>) {
        let flags = self.flags(cx);
        let closing: Vec<usize> = (0..self.tabs.len())
            .filter(|&ix| self.tabs[ix].group.as_deref() == Some(root))
            .filter(|&ix| !flags[ix].pinned && !flags[ix].dirty)
            .collect();
        self.remove_tabs(&closing, cx);
    }

    /// Activates the Server tab, reopening it (after pinned tabs) if closed.
    fn show_server_tab(&mut self) {
        match self
            .tabs
            .iter()
            .position(|t| matches!(t.item, PaneItem::Server))
        {
            Some(ix) => self.active = ix,
            None => {
                let at = self.tabs.iter().take_while(|t| t.pinned).count();
                self.tabs.insert(
                    at,
                    Tab {
                        item: PaneItem::Server,
                        pinned: false,
                        group: None,
                    },
                );
                self.active = at;
            }
        }
    }

    /// The profile's (read-only) client, or `None` after starting a
    /// reconnect when the old connection died.
    fn live_client(
        &self,
        profile: &str,
        read_only: bool,
        cx: &mut Context<Self>,
    ) -> Option<std::sync::Arc<tokio_postgres::Client>> {
        let client = self.db.read(cx).client(profile, read_only);
        if client.is_none() {
            self.db.update(cx, |s, cx| s.ensure_connecting(profile, cx));
        }
        client
    }

    fn on_db_event(&mut self, event: &DbEvent, window: &mut Window, cx: &mut Context<Self>) {
        match event {
            DbEvent::OpenTable {
                profile,
                schema,
                table,
            } => {
                // One tab per table per connection.
                let key: SharedString =
                    format!("table\u{0}{profile}\u{0}{schema}\u{0}{table}").into();
                if let Some(ix) = self
                    .tabs
                    .iter()
                    .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
                {
                    self.active = ix;
                    cx.notify();
                    return;
                }
                if self.live_client(profile, true, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                let title: SharedString = format!("{schema}.{table}").into();
                let (db, profile, schema, table) = (
                    self.db.clone(),
                    profile.clone(),
                    schema.clone(),
                    table.clone(),
                );
                let view = cx.new(|cx| {
                    TableView::new(title.clone(), db, profile, schema, table, window, cx)
                });
                // Its "Query" button opens a SQL editor.
                cx.subscribe_in(&view, window, |this, _, event: &DbEvent, window, cx| {
                    this.on_db_event(event, window, cx)
                })
                .detach();
                self.open_item(view.into(), key, title, Lucide::Table, cx);
            }
            DbEvent::OpenSql { profile, sql } => {
                if self.live_client(profile, false, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                self.sql_count += 1;
                let shown = self
                    .db
                    .read(cx)
                    .profile(profile)
                    .map_or(profile.as_str(), |p| p.display_name())
                    .to_string();
                let title: SharedString = format!("SQL {} · {shown}", self.sql_count).into();
                let (db, profile, sql) = (self.db.clone(), profile.clone(), sql.clone());
                let view = cx.new(|cx| SqlView::new(title.clone(), db, profile, sql, window, cx));
                if std::mem::take(&mut self.run_next_sql) {
                    view.update(cx, |v, cx| v.run_now(window, cx));
                }
                self.open_item(
                    view.into(),
                    title.clone(),
                    title,
                    Lucide::SquareTerminal,
                    cx,
                );
            }
            DbEvent::OpenDiagram { profile, schema } => {
                let key: SharedString = format!("diagram\u{0}{profile}\u{0}{schema}").into();
                if let Some(ix) = self
                    .tabs
                    .iter()
                    .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
                {
                    self.active = ix;
                    cx.notify();
                    return;
                }
                if self.live_client(profile, true, cx).is_none() {
                    self.pending_db = Some(event.clone());
                    return;
                }
                let title: SharedString = format!("{schema} · diagram").into();
                let (db, profile, schema) = (self.db.clone(), profile.clone(), schema.clone());
                let view =
                    cx.new(|cx| DiagramView::new(title.clone(), db, profile, schema, window, cx));
                self.open_item(view.into(), key, title, Lucide::Waypoints, cx);
            }
        }
    }

    fn flags(&self, cx: &App) -> Vec<TabFlags> {
        self.tabs
            .iter()
            .map(|t| TabFlags {
                pinned: t.pinned,
                dirty: matches!(&t.item, PaneItem::File(e) if e.read(cx).dirty),
            })
            .collect()
    }

    /// Closes tabs per `op` relative to `target`. Closing a single tab with
    /// unsaved changes asks first; bulk closes leave such tabs open.
    fn close(&mut self, op: CloseOp, target: usize, window: &mut Window, cx: &mut Context<Self>) {
        let closing = to_close(op, target, &self.flags(cx));
        if let [ix] = closing[..]
            && let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone())
            && editor.read(cx).dirty
        {
            self.confirm_close_dirty(editor, window, cx);
            return;
        }
        self.remove_tabs(&closing, cx);
    }

    fn remove_tabs(&mut self, closing: &[usize], cx: &mut Context<Self>) {
        if closing.is_empty() {
            return;
        }
        let len = self.tabs.len();
        self.active = active_after(self.active, closing, len).unwrap_or(0);
        let mut ix = 0;
        self.tabs.retain(|_| {
            let keep = !closing.contains(&ix);
            ix += 1;
            keep
        });
        cx.notify();
    }

    fn confirm_close_dirty(
        &mut self,
        editor: Entity<FileEditor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = editor.read(cx).title();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Save changes to {name}?"),
            Some("Your changes will be lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            let close = match answer.await {
                Ok(0) => editor.update(cx, |e, cx| e.save_task(cx)).await,
                Ok(1) => true,
                _ => false,
            };
            if close {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this
                        .tabs
                        .iter()
                        .position(|t| matches!(&t.item, PaneItem::File(e) if *e == editor))
                    {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    fn toggle_pin(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        self.tabs.remove(ix);
        // Pinned tabs sit together at the start, like in Zed; a pinned tab
        // goes to the end of them, an unpinned one back into its group.
        let pinned = !tab.pinned;
        let at = insert_index(&self.slots(), pinned, tab.group.as_deref());
        self.tabs.insert(at, Tab { pinned, ..tab });
        self.active = active_after_move(self.active, ix, at);
        cx.notify();
    }

    fn toggle_read_only(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone()) {
            editor.update(cx, |e, cx| {
                let ro = !e.read_only;
                e.set_read_only(ro, cx)
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_menu(
        this: WeakEntity<Self>,
        ix: usize,
        is_file: bool,
        read_only: bool,
        pinned: bool,
        group: Option<PathBuf>,
        projects: Vec<PathBuf>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let muted = cx.theme().muted_foreground;
        let item = |label: &'static str, keys: &'static str| {
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .w_full()
                    .gap_6()
                    .justify_between()
                    .child(label)
                    .child(div().text_color(muted).child(keys))
            })
        };
        let close = |op: CloseOp| {
            let this = this.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |ws, cx| ws.close(op, ix, window, cx));
            }
        };
        let this_pin = this.clone();
        let this_ro = this.clone();
        let menu = Self::group_menu(this.clone(), ix, group, projects, menu, window, cx);
        menu.item(item("Close", "⌘W").on_click(close(CloseOp::This)))
            .item(item("Close Others", "⌥⌘T").on_click(close(CloseOp::Others)))
            .separator()
            .item(item("Close Left", "⌘K E").on_click(close(CloseOp::Left)))
            .item(item("Close Right", "⌘K T").on_click(close(CloseOp::Right)))
            .separator()
            .item(item("Close Clean", "⌘K U").on_click(close(CloseOp::Clean)))
            .item(item("Close All", "⌘K W").on_click(close(CloseOp::All)))
            .when(is_file, |menu| {
                menu.separator().item(
                    PopupMenuItem::new(if read_only {
                        "Make Tab Editable"
                    } else {
                        "Make Tab Read-Only"
                    })
                    .on_click(move |_, _, cx| {
                        let _ = this_ro.update(cx, |ws, cx| ws.toggle_read_only(ix, cx));
                    }),
                )
            })
            .separator()
            .item(
                item(if pinned { "Unpin Tab" } else { "Pin Tab" }, "⌘K ⇧↩").on_click(
                    move |_, _, cx| {
                        let _ = this_pin.update(cx, |ws, cx| ws.toggle_pin(ix, cx));
                    },
                ),
            )
    }

    /// "Move to Project" and "Remove from Group", for a tab's menu.
    fn group_menu(
        this: WeakEntity<Self>,
        ix: usize,
        group: Option<PathBuf>,
        projects: Vec<PathBuf>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let others: Vec<PathBuf> = projects
            .into_iter()
            .filter(|p| Some(p) != group.as_ref())
            .collect();
        if others.is_empty() && group.is_none() {
            return menu;
        }
        let this_move = this.clone();
        menu.when(!others.is_empty(), |menu| {
            menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
                for root in &others {
                    let this = this_move.clone();
                    let to = root.clone();
                    menu = menu.item(PopupMenuItem::new(project_name(root)).on_click(
                        move |_, _, cx| {
                            let _ = this
                                .update(cx, |ws, cx| ws.move_to_group(ix, Some(to.clone()), cx));
                        },
                    ));
                }
                menu
            })
        })
        .when(group.is_some(), |menu| {
            menu.item(
                PopupMenuItem::new("Remove from Group").on_click(move |_, _, cx| {
                    let _ = this.update(cx, |ws, cx| ws.move_to_group(ix, None, cx));
                }),
            )
        })
        .separator()
    }

    fn render_sidebar_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let tabs: [(&'static str, Icon, &'static str, SidebarTab, &dyn Action); 10] = [
            (
                "side-servers",
                Lucide::Server.into(),
                "PostgreSQL Instances",
                SidebarTab::Servers,
                &ShowInstances,
            ),
            (
                "side-files",
                IconName::Folder.into(),
                "Files",
                SidebarTab::Files,
                &ShowFiles,
            ),
            (
                "side-git",
                Lucide::GitBranch.into(),
                "Git",
                SidebarTab::Git,
                &ShowGit,
            ),
            (
                "side-db",
                Lucide::Database.into(),
                "Database",
                SidebarTab::Database,
                &ShowDatabase,
            ),
            (
                "side-run",
                Lucide::Play.into(),
                "Run Scripts",
                SidebarTab::Run,
                &ShowRun,
            ),
            (
                "side-ports",
                Lucide::Radio.into(),
                "Listening Ports",
                SidebarTab::Ports,
                &ShowPorts,
            ),
            (
                "side-network",
                IconName::Globe.into(),
                "Network Diagnosis",
                SidebarTab::Network,
                &ShowNetwork,
            ),
            (
                "side-ssh",
                Lucide::SquareTerminal.into(),
                "SSH Servers",
                SidebarTab::Ssh,
                &ShowSsh,
            ),
            (
                "side-shell-config",
                Lucide::FileTerminal.into(),
                "Shell Config",
                SidebarTab::ShellConfig,
                &ShowShellConfig,
            ),
            (
                "side-prompts",
                Lucide::MessageSquareText.into(),
                "Prompts",
                SidebarTab::Prompts,
                &ShowPrompts,
            ),
        ];
        let current = self.sidebar;
        h_flex()
            .h(px(30.))
            .px_1()
            .gap_0p5()
            .bg(cx.theme().sidebar)
            .border_b_1()
            .border_color(cx.theme().border)
            .children(tabs.into_iter().map(|(id, icon, tooltip, which, action)| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip_with_action(tooltip, action, Some("Workspace"))
                    .selected(current == which)
                    .on_click(cx.listener(move |this, _, _, cx| this.show_panel(which, cx)))
            }))
    }

    /// Shows `tab`'s panel in the sidebar, opening the sidebar if it's hidden.
    fn show_panel(&mut self, tab: SidebarTab, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        self.sidebar = tab;
        if !self.layout.sidebar {
            self.layout.toggle(Area::Sidebar);
        }
        if tab == SidebarTab::ShellConfig {
            self.shell_config_panel.update(cx, |p, cx| p.refresh(cx));
        }
        cx.notify();
    }

    /// Shows the tab `step` places after (or before) the active one.
    fn cycle_tab(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.tabs.len() as isize;
        if len > 1 {
            let ix = (self.active as isize + step).rem_euclid(len) as usize;
            self.show_center(cx);
            self.activate_tab(ix, cx);
        }
    }

    /// Switches to the project `step` places after (or before) the active one.
    fn cycle_project(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.projects.len() as isize;
        if len > 1 {
            let ix = (self.project as isize + step).rem_euclid(len) as usize;
            self.leave_welcome(PANEL_AND_TABS, cx);
            self.activate_project(ix, cx);
            cx.notify();
        }
    }

    /// A new shell in the active project's folder (else the home folder).
    fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.root(cx) {
            Some(root) => self.open_terminal_in(root, window, cx),
            None => {
                self.leave_welcome(TERMINAL_ONLY, cx);
                if !self.layout.terminal {
                    self.layout.toggle(Area::Terminal);
                }
                self.terminal
                    .update(cx, |t, cx| t.open_shell(Some(window), cx));
                cx.notify();
            }
        }
    }

    /// ⇧⌘↩: the canvas fills the window while it is the tab on screen and
    /// the terminal doesn't have the keyboard; otherwise the terminal does.
    fn toggle_maximize_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let terminal_focused = self.terminal.read(cx).has_focus(window, cx);
        let canvas_shown =
            self.welcome.is_none() && self.layout.center && self.active_canvas().is_some();
        if canvas_shown && !terminal_focused || self.canvas_maximized {
            self.toggle_canvas_maximized(cx);
        } else {
            self.toggle_terminal_maximized(window, cx);
        }
    }

    fn render_sidebar(&self) -> AnyElement {
        let view: AnyView = match self.sidebar {
            SidebarTab::Servers => self.server_list.clone().into(),
            SidebarTab::Files => self.current().files.clone().into(),
            SidebarTab::Git => self.current().git_panel.clone().into(),
            SidebarTab::Database => self.db_panel.clone().into(),
            SidebarTab::Run => self.current().run_panel.clone().into(),
            SidebarTab::Ports => self.ports_panel.clone().into(),
            SidebarTab::Network => self.network_panel.clone().into(),
            SidebarTab::Prompts => self.prompts_panel.clone().into(),
            SidebarTab::Ssh => self.ssh_panel.clone().into(),
            SidebarTab::ShellConfig => self.shell_config_panel.clone().into(),
        };
        cached(view)
    }

    fn render_tab_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let weak = cx.entity().downgrade();
        let projects = self.roots(cx);
        let mut items: Vec<AnyElement> = Vec::new();
        for (ix, tab) in self.tabs.iter().enumerate() {
            let group = tab.group.clone().filter(|_| !tab.pinned);
            let color = group.as_ref().map(|g| self.group_color(g, cx));
            // A header in front of each group's run of tabs.
            let starts_run =
                ix == 0 || self.tabs[ix - 1].pinned || self.tabs[ix - 1].group != tab.group;
            if let (Some(root), Some(color), true) = (&group, color, starts_run) {
                let collapsed = self.collapsed.contains(root);
                let count = self.tabs[ix..]
                    .iter()
                    .take_while(|t| !t.pinned && t.group == tab.group)
                    .count();
                let (toggle, close) = (root.clone(), root.clone());
                let weak = weak.clone();
                items.push(
                    group_header(
                        ("tab-group", ix),
                        project_name(root),
                        color,
                        collapsed,
                        count,
                        cx,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.toggle_group(toggle.clone(), cx)),
                    )
                    .context_menu(move |menu, _, _| {
                        let weak = weak.clone();
                        let close = close.clone();
                        menu.item(PopupMenuItem::new("Close Tabs in Group").on_click(
                            move |_, _, cx| {
                                let _ = weak.update(cx, |ws, cx| ws.close_group(&close, cx));
                            },
                        ))
                    })
                    .into_any_element(),
                );
            }
            let active = ix == self.active;
            // A folded group still shows its active tab.
            if !active && group.as_ref().is_some_and(|g| self.collapsed.contains(g)) {
                continue;
            }
            let projects = projects.clone();
            let tab_group = tab.group.clone();
            items.push({
                let pinned = tab.pinned;
                let glyph = |icon: Lucide| Icon::new(icon).xsmall().into_any_element();
                let (icon, title, dirty, read_only) = match &tab.item {
                    PaneItem::Server => {
                        (glyph(Lucide::Server), "Instance".to_string(), false, None)
                    }
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        (
                            crate::file_icons::file_icon(&e.path, cx),
                            e.title(),
                            e.dirty,
                            Some(e.read_only),
                        )
                    }
                    PaneItem::Other {
                        title, icon, file, ..
                    } => (
                        // The file's logo, else the tab's own icon (a canvas file has no logo).
                        match file {
                            Some(path) if crate::file_icons::logo_for(path).is_some() => {
                                crate::file_icons::file_icon(path, cx)
                            }
                            _ => glyph(*icon),
                        },
                        title.to_string(),
                        false,
                        None,
                    ),
                };
                let weak = weak.clone();
                h_flex()
                    .id(("pane-tab", ix))
                    .relative()
                    .flex_none()
                    .h_full()
                    .pl_3()
                    .pr_1()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| {
                        el.bg(cx.theme().tab).text_color(cx.theme().tab_foreground)
                    })
                    .child(icon)
                    .child(title)
                    .when(read_only == Some(true), |el| {
                        el.child(
                            Icon::new(Lucide::Lock)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .when(dirty, |el| {
                        el.child(div().size(px(6.)).rounded_full().bg(cx.theme().foreground))
                    })
                    .child(if pinned {
                        Button::new(("pin-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Pin)
                            .tooltip("Unpin")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_pin(ix, cx);
                            }))
                    } else {
                        Button::new(("close-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip("Close (⌘W)")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close(CloseOp::This, ix, window, cx);
                            }))
                    })
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener(move |this, _, _, cx| this.activate_tab(ix, cx)))
                    .context_menu(move |menu, window, cx| {
                        Self::tab_menu(
                            weak.clone(),
                            ix,
                            read_only.is_some(),
                            read_only == Some(true),
                            pinned,
                            tab_group.clone(),
                            projects.clone(),
                            menu,
                            window,
                            cx,
                        )
                    })
                    .into_any_element()
            });
        }
        if self.scrolled_to != Some(self.active) {
            // Bring a newly opened/activated tab into view.
            self.tab_scroll.scroll_to_item(self.active);
            self.scrolled_to = Some(self.active);
        }
        h_flex()
            .id("tab-bar")
            .h(px(32.))
            .flex_none()
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .children(items)
    }

    fn render_active_item(&self, cx: &mut Context<Self>) -> AnyElement {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::File(editor)) => cached(editor.clone().into()),
            Some(PaneItem::Other { view, .. }) => cached(view.clone()),
            Some(PaneItem::Server) => cached(self.server_detail.clone().into()),
            None => Placeholder::new(Lucide::PanelTop, "No tabs open")
                .detail("Pick an instance, file or table in the sidebar to open it here.")
                .when(!self.layout.sidebar, |state| {
                    state.action(
                        Button::new("empty-show-sidebar")
                            .small()
                            .icon(Lucide::PanelLeft)
                            .label("Show Sidebar")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx)),
                            ),
                    )
                })
                .action(
                    Button::new("empty-open-folder")
                        .small()
                        .icon(Lucide::FolderOpen)
                        .label("Open Folder…")
                        .on_click(|_, window, cx| {
                            crate::cli::prompt_open_folder(Some(window.window_handle()), cx)
                        }),
                )
                .into_any_element(),
        }
    }

    /// Puts the project path on the clipboard; the button shows a check
    /// for a moment as confirmation.
    fn copy_project_path(&mut self, path: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(path));
        self.path_copied = true;
        cx.notify();
        self._path_copied_reset = cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.path_copied = false;
                cx.notify();
            });
        });
    }

    fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let store = self.servers.read(cx);
        let running = store.running_count();
        let total = store.registry.instances.len();
        let project = self.root(cx).map(|r| r.display().to_string());
        let copied = self.path_copied;
        let git = self.current().git.read(cx);
        let branch = git.status.as_ref().map(|s| {
            let name = s
                .branch
                .clone()
                .or_else(|| s.head.clone())
                .unwrap_or_else(|| "main".into());
            let mut sync = String::new();
            if s.behind > 0 {
                sync.push_str(&format!(" ↓{}", s.behind));
            }
            if s.ahead > 0 {
                sync.push_str(&format!(" ↑{}", s.ahead));
            }
            (name, sync, s.files.len())
        });
        h_flex()
            .h(px(24.))
            .px_2()
            .gap_3()
            .justify_between()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .child(
                h_flex()
                    .gap_1()
                    .child(div().size(px(6.)).rounded_full().bg(if running > 0 {
                        cx.theme().success
                    } else {
                        cx.theme().muted_foreground
                    }))
                    .child(format!("{running}/{total} instances running")),
            )
            .when_some(branch, |el, (name, sync, changed)| {
                el.child(
                    Button::new("status-git-branch")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::GitBranch)
                        .label(format!(
                            "{name}{sync}{}",
                            if changed > 0 {
                                format!(" · {changed} changed")
                            } else {
                                String::new()
                            }
                        ))
                        .tooltip("Show Git")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.leave_welcome(PANEL_AND_TABS, cx);
                            if !this.layout.sidebar {
                                this.layout.toggle(Area::Sidebar);
                            }
                            this.sidebar = SidebarTab::Git;
                            cx.notify();
                        })),
                )
            })
            .when_some(project, |el, project| {
                el.child(
                    h_flex()
                        .min_w_0()
                        .gap_1()
                        .child(div().truncate().child(project.clone()))
                        .child(
                            Button::new("copy-project-path")
                                .ghost()
                                .xsmall()
                                .icon(if copied { Lucide::Check } else { Lucide::Copy })
                                .when(copied, |b| b.tooltip("Copied"))
                                .when(!copied, |b| {
                                    b.tooltip_with_action(
                                        "Copy Path",
                                        &CopyProjectPath,
                                        Some("Workspace"),
                                    )
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.copy_project_path(project.clone(), cx)
                                })),
                        ),
                )
            })
    }
}

const CENTER_ONLY: Layout = Layout {
    sidebar: false,
    center: true,
    terminal: false,
};
const TERMINAL_ONLY: Layout = Layout {
    sidebar: false,
    center: false,
    terminal: true,
};
/// A sidebar panel next to the tabs.
const PANEL_AND_TABS: Layout = Layout {
    sidebar: true,
    center: true,
    terminal: false,
};

/// Puts `dir` first in the recent folders the welcome screen lists.
fn remember_recent(dir: PathBuf) {
    let file = AppPaths::default_macos().recent_file();
    let mut recent = RecentProjects::load(&file).unwrap_or_default();
    recent.touch(dir);
    if let Err(e) = recent.save(&file) {
        eprintln!("recent: {e}");
    }
}

impl Workspace {
    /// Swaps the welcome screen for the panels in `layout`; once it is gone
    /// the layout is left alone.
    fn leave_welcome(&mut self, layout: Layout, cx: &mut Context<Self>) {
        if self.welcome.take().is_some() {
            self.layout = layout;
            cx.notify();
        }
    }

    fn on_welcome_event(
        &mut self,
        event: &WelcomeEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = window.window_handle();
        match event {
            WelcomeEvent::OpenFolder => crate::cli::prompt_open_folder(Some(handle), cx),
            WelcomeEvent::OpenRecent(dir) => {
                // Reads every workspace, this one included: after this update.
                let dir = dir.clone();
                cx.defer(move |cx| crate::cli::open_folder_in(dir, Some(handle), cx));
            }
            WelcomeEvent::NewInstance => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Servers;
                self.servers.update(cx, |s, cx| s.show_create(cx));
            }
            WelcomeEvent::OpenServer(name) => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Servers;
                self.servers.update(cx, |s, cx| s.select(name, cx));
            }
            WelcomeEvent::OpenDatabase(name) => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.sidebar = SidebarTab::Database;
                self.db.update(cx, |s, cx| s.ensure_connecting(name, cx));
            }
            WelcomeEvent::OpenTerminal => {
                self.leave_welcome(TERMINAL_ONLY, cx);
                self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
            }
            WelcomeEvent::Dismiss => self.leave_welcome(PANEL_AND_TABS, cx),
        }
        cx.notify();
    }

    /// Starts or shows scripts from the Run panel, showing the terminal.
    fn on_run_event(&mut self, event: &RunEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        match event {
            RunEvent::Start(launches) => {
                let last = launches.len().saturating_sub(1);
                self.terminal.update(cx, |t, cx| {
                    for (i, l) in launches.iter().enumerate() {
                        let window = (i == last).then_some(&mut *window);
                        t.start_job(l.tag.clone(), l.dir.clone(), &l.command, window, cx);
                    }
                });
            }
            RunEvent::Show(key) => {
                self.terminal
                    .update(cx, |t, cx| t.show_job(key, window, cx));
            }
        }
        cx.notify();
    }

    /// Types `commands` into this window's terminal, showing it if hidden.
    pub fn run_in_terminal(&mut self, commands: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(TERMINAL_ONLY, cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        self.terminal
            .update(cx, |t, cx| t.run(commands, window, cx));
        cx.notify();
    }

    fn toggle_area(&mut self, area: Area, cx: &mut Context<Self>) {
        // From the welcome screen, toggles build up from just the tabs.
        self.leave_welcome(CENTER_ONLY, cx);
        self.layout.toggle(area);
        cx.notify();
    }

    /// Fills the window with the terminal, or puts back the layout it
    /// replaced (everything, if that was terminal-only too).
    fn toggle_terminal_maximized(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.canvas_maximized = false;
        if self.layout == TERMINAL_ONLY {
            self.layout = self.unmaximized.take().unwrap_or_default();
        } else {
            self.unmaximized = Some(self.layout);
            self.layout = TERMINAL_ONLY;
        }
        self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
        cx.notify();
    }

    /// Opens a saved prompt in its own tab (one per prompt).
    fn open_prompt(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        let key: SharedString = format!("prompt\u{0}{id}").into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let store = self.prompt_store.clone();
            let view = cx.new(|cx| PromptView::new(id, store, window, cx));
            let title = view.read(cx).title(cx).into();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title,
                    icon: Lucide::MessageSquareText,
                    file: None,
                },
                None,
                cx,
            );
        }
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        cx.notify();
    }

    /// Shows a session to the server with `key` in the terminal dock: the
    /// open one, unless `new` or there is none.
    fn connect_ssh(&mut self, key: String, new: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        if !new
            && self
                .terminal
                .update(cx, |t, cx| t.focus_ssh(&key, window, cx))
        {
            cx.notify();
            return;
        }
        match self.ssh_store.update(cx, |s, cx| s.remote(&key, cx)) {
            Ok(remote) => self
                .terminal
                .update(cx, |t, cx| t.open_ssh(remote, Some(window), cx)),
            // Settings are incomplete: show them.
            Err(_) => {
                if let Some(id) = key.strip_prefix("ssh\u{0}") {
                    self.open_ssh_host(id.to_string(), window, cx);
                }
            }
        }
        cx.notify();
    }

    /// Opens a saved server's settings in its own tab (one per server).
    fn open_ssh_host(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        let key: SharedString = format!("ssh-host\u{0}{id}").into();
        if !self.activate_key(&key) {
            let store = self.ssh_store.clone();
            let view = cx.new(|cx| SshHostView::new(id, store, window, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, SshHostEvent::Connect(key), window, cx| {
                    this.connect_ssh(key.clone(), false, window, cx)
                },
            )
            .detach();
            let title = view.read(cx).title(cx).into();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title,
                    icon: Lucide::Server,
                    file: None,
                },
                None,
                cx,
            );
        }
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        cx.notify();
    }

    /// `key` for the active project: tabs like the commit graph are one
    /// per project, not one per window.
    fn project_key(&self, key: &str, cx: &App) -> String {
        match self.root(cx) {
            Some(root) => format!("{key}\u{0}{}", root.display()),
            None => key.to_string(),
        }
    }

    /// Activates the tab with `key`, if one is open.
    fn activate_key(&mut self, key: &str) -> bool {
        match self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if k.as_ref() == key))
        {
            Some(ix) => {
                self.active = ix;
                true
            }
            None => false,
        }
    }

    fn show_center(&mut self, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
    }

    /// Open tabs of a renamed or moved file (or of files in a moved folder)
    /// follow it.
    fn follow_move(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        for tab in &self.tabs {
            if let PaneItem::File(editor) = &tab.item {
                editor.update(cx, |e, cx| {
                    if let Ok(rest) = e.path.strip_prefix(from) {
                        e.path = if rest.as_os_str().is_empty() {
                            to.to_path_buf()
                        } else {
                            to.join(rest)
                        };
                        cx.notify();
                    }
                });
            }
        }
        cx.notify();
    }

    /// Find in Folder, one tab per folder.
    fn open_folder_search(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = FolderSearch::key(&dir);
        if self.activate_key(&key) {
            if let Some(PaneItem::Other { view, .. }) = self.tabs.get(self.active).map(|t| &t.item)
                && let Ok(search) = view.clone().downcast::<FolderSearch>()
            {
                let query = search.read(cx).query.clone();
                query.update(cx, |s, cx| s.focus(window, cx));
            }
        } else {
            let name = dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string());
            let dir_for_group = dir.clone();
            let view = cx.new(|cx| FolderSearch::new(dir, window, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &SearchEvent, window, cx| match e {
                    SearchEvent::Open { path, line } => {
                        this.open_file_at(path.clone(), Some((*line, 1)), window, cx)
                    }
                },
            )
            .detach();
            let group = self.group_of(&dir_for_group, cx);
            self.open_item_in(
                view.into(),
                key.into(),
                format!("Search: {name}").into(),
                Lucide::Search,
                group,
                cx,
            );
        }
        cx.notify();
    }

    /// The commits touching one file or folder (one tab each).
    fn open_git_history(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = format!("git-history\u{0}{}", path.display());
        if !self.activate_key(&key) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GraphView::history(git, path, cx));
            cx.subscribe(&view, |this, _, GraphEvent::OpenCommit(commit), cx| {
                this.open_commit(commit.clone(), cx)
            })
            .detach();
            self.open_item_in(
                view.into(),
                key.into(),
                format!("History: {name}").into(),
                Lucide::GitCommitHorizontal,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// The commit graph (one tab).
    fn open_git_graph(&mut self, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = self.project_key("git-graph", cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GraphView::new(git, cx));
            cx.subscribe(&view, |this, _, GraphEvent::OpenCommit(commit), cx| {
                this.open_commit(commit.clone(), cx)
            })
            .detach();
            self.open_item_in(
                view.into(),
                key.into(),
                "Commit Graph".into(),
                Lucide::GitGraph,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// One commit, one tab per commit.
    fn open_commit(&mut self, commit: sawit_core::git::Commit, cx: &mut Context<Self>) {
        let key = CommitView::key(&commit.hash);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| CommitView::new(git, commit, cx));
            let title = view.read(cx).title();
            self.open_item_in(
                view.into(),
                key,
                title,
                Lucide::GitCommitHorizontal,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// The GitHub tab (one tab).
    fn open_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_center(cx);
        let key = self.project_key("github", cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| GitHubView::new(git, window, cx));
            self.open_item_in(
                view.into(),
                key.into(),
                "GitHub".into(),
                Lucide::Github,
                group.clone(),
                cx,
            );
        }
        cx.notify();
    }

    /// A changed file's diff, one tab per file and side.
    fn open_git_diff(
        &mut self,
        file: sawit_core::git::FileChange,
        staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_center(cx);
        let key = self.project_key(&DiffView::key(&file, staged), cx);
        if !self.activate_key(&key) {
            let git = self.current().git.clone();
            let group = self.root(cx);
            let view = cx.new(|cx| DiffView::new(git, file, staged, cx));
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &DiffViewEvent, window, cx| match e {
                    DiffViewEvent::OpenFile(path) => this.open_file(path.clone(), window, cx),
                },
            )
            .detach();
            let title = view.read(cx).title();
            self.open_item_in(
                view.into(),
                key.into(),
                title,
                Lucide::GitCommitHorizontal,
                group,
                cx,
            );
        }
        cx.notify();
    }

    /// Shows the version & changelog page (one tab).
    fn open_changelog_page(&mut self, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        let key: SharedString = "changelog".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(ChangelogPage::new);
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Changelog".into(),
                    icon: Lucide::ScrollText,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Opens the active project's brainstorming canvas (one tab per
    /// project, saved in its `.nyawit/`), or the app-wide one without a
    /// project. With `native_canvas` it is the native canvas: its file is
    /// `.nyawit/canvas.figcraft` (else `canvas.figcraft` in Sawit's own
    /// folder) instead of the Excalidraw board.
    fn open_canvas(&mut self, cx: &mut Context<Self>) -> Option<CanvasTab> {
        let root = self.root(cx);
        self.open_canvas_of(root, cx)
    }

    /// Opens the canvas of the project at `root`, or the app-wide one for `None`, as
    /// [`Self::open_canvas`] opens the active project's. A native canvas whose file is open in a
    /// canvas file's tab already (opened before its project was in this window) is that tab: two
    /// canvases of one file would each save over the other's edits.
    fn open_canvas_of(
        &mut self,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<CanvasTab> {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        let key: SharedString = match &root {
            Some(root) => format!("canvas\u{0}{}", root.display()).into(),
            None => "canvas".into(),
        };
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            cx.notify();
            // The canvas is the tab's own view.
            let PaneItem::Other { view, .. } = &self.tabs[ix].item else {
                return None;
            };
            return match view.clone().downcast::<BoardView>() {
                Ok(board) => Some(CanvasTab::Native(board)),
                Err(view) => view.downcast::<CanvasView>().ok().map(CanvasTab::Web),
            };
        }
        let tab = if self.native_canvas {
            let file = board_file(root.as_deref(), &AppPaths::default_macos().root);
            if let Some((ix, board)) = self.board_tab(&file, cx) {
                self.activate_tab(ix, cx);
                return Some(CanvasTab::Native(board));
            }
            let view = cx.new(|cx| BoardView::open(file, cx));
            cx.subscribe(&view, |this, _, _: &ToggleCanvasMaximize, cx| {
                this.toggle_canvas_maximized(cx)
            })
            .detach();
            self.boards.push(view.downgrade());
            CanvasTab::Native(view)
        } else {
            let view = cx.new(|cx| CanvasView::new(root.clone(), cx));
            cx.subscribe(&view, |this, _, _: &ToggleCanvasMaximize, cx| {
                this.toggle_canvas_maximized(cx)
            })
            .detach();
            self.canvases.push(view.downgrade());
            CanvasTab::Web(view)
        };
        self.push_tab(
            PaneItem::Other {
                view: tab.view(),
                key,
                title: crate::canvas::TITLE.into(),
                icon: crate::canvas::ICON,
                file: None,
            },
            root,
            cx,
        );
        cx.notify();
        Some(tab)
    }

    /// The canvas whose tab is showing in the center pane: a project's canvas, or a canvas file's.
    fn active_canvas(&self) -> Option<EntityId> {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::Other { key, view, .. })
                if key.starts_with("canvas") || view.clone().downcast::<BoardView>().is_ok() =>
            {
                Some(view.entity_id())
            }
            _ => None,
        }
    }

    /// Fills the window with the canvas (hiding the tab bar too), or puts
    /// back the layout it replaced, like the terminal's maximize.
    fn toggle_canvas_maximized(&mut self, cx: &mut Context<Self>) {
        if self.canvas_maximized && self.layout == CENTER_ONLY {
            self.canvas_maximized = false;
            self.layout = self.unmaximized.take().unwrap_or_default();
        } else {
            if !self.canvas_maximized && self.layout != TERMINAL_ONLY {
                self.unmaximized = Some(self.layout);
            }
            self.canvas_maximized = true;
            self.layout = CENTER_ONLY;
        }
        cx.notify();
    }

    /// Keeps the canvases' native webviews in step with what is on screen:
    /// they draw above GPUI, so each is hidden unless its tab is showing.
    fn sync_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvases.retain(|c| c.upgrade().is_some());
        self.boards.retain(|b| b.upgrade().is_some());
        let shown = self
            .active_canvas()
            .filter(|_| self.welcome.is_none() && self.layout.center);
        // Any other layout change (⌘B, ⌘J, another tab) ends "fill window".
        if self.canvas_maximized && !(shown.is_some() && self.layout == CENTER_ONLY) {
            self.canvas_maximized = false;
            self.unmaximized = None;
        }
        let maximized = self.canvas_maximized;
        for canvas in self.canvases.iter().filter_map(|c| c.upgrade()) {
            let visible = shown == Some(canvas.entity_id());
            canvas.update(cx, |c, _| {
                c.set_visible(visible);
                c.set_maximized(visible && maximized);
            });
        }
    }

    /// Opens the browser (one tab), at `url` when given; an open browser
    /// becomes the active tab and keeps its page.
    pub(crate) fn open_browser(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        let key: SharedString = "browser".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let project = self.root(cx);
            let view = cx.new(|cx| BrowserView::new(project, url, window, cx));
            self.browser = Some(view.downgrade());
            cx.subscribe_in(&view, window, |this, browser, event, window, cx| {
                let crate::browser::BrowserEvent::SendToTerminal(md) = event;
                this.send_to_terminal(browser, md, window, cx);
            })
            .detach();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: crate::browser::TITLE.into(),
                    icon: crate::browser::ICON,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Pastes annotations into the terminal running an agent; copies them
    /// when no tab has bracketed paste on.
    fn send_to_terminal(
        &mut self,
        browser: &Entity<BrowserView>,
        markdown: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = match self.terminal.read(cx).paste_target(cx) {
            Some(ix) => {
                if !self.layout.terminal {
                    self.layout.toggle(Area::Terminal);
                }
                let sent = self
                    .terminal
                    .update(cx, |t, cx| t.paste(ix, markdown, window, cx));
                match sent {
                    Ok(()) => format!("Sent to Terminal {}", ix + 1),
                    Err(e) => e,
                }
            }
            None => {
                cx.write_to_clipboard(ClipboardItem::new_string(markdown.to_string()));
                "No terminal is ready for a paste, so it was copied.".to_string()
            }
        };
        browser.update(cx, |b, cx| b.set_annotate_note(&note, cx));
        cx.notify();
    }

    /// The browser is the tab showing in the center pane.
    fn browser_active(&self) -> bool {
        matches!(
            self.tabs.get(self.active).map(|t| &t.item),
            Some(PaneItem::Other { key, .. }) if key.as_ref() == "browser"
        )
    }

    /// Keeps the browser's native webview in step with what is on screen,
    /// as [`Self::sync_canvas`] does for the canvas.
    fn sync_browser(&mut self, cx: &mut Context<Self>) {
        let Some(browser) = self.browser.as_ref().and_then(|b| b.upgrade()) else {
            self.browser = None;
            return;
        };
        let visible = self.welcome.is_none() && self.layout.center && self.browser_active();
        browser.update(cx, |b, _| b.set_visible(visible));
    }

    /// Shows the MCP install page (one tab).
    fn open_mcp_page(&mut self, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        let key: SharedString = "mcp".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(McpPage::new);
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "MCP".into(),
                    icon: Lucide::Plug,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Shows the Dev Tools install page (one tab).
    fn open_tools_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        let key: SharedString = "tools".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
        } else {
            let view = cx.new(ToolsPage::new);
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &ToolsEvent, window, cx| match e {
                    ToolsEvent::RunInTerminal(command) => this.run_in_terminal(command, window, cx),
                },
            )
            .detach();
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Dev Tools".into(),
                    icon: Lucide::Package,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// One chip per open project (click to switch, × to close) and a +
    /// that adds another; just the tagline while no project is open.
    fn render_project_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        let roots: Vec<Option<PathBuf>> = self.projects.iter().map(|p| p.root(cx)).collect();
        if roots.iter().all(Option::is_none) {
            return div()
                .text_color(cx.theme().muted_foreground)
                .child("Your local dev workspace")
                .into_any_element();
        }
        let only = roots.len() == 1;
        let mut colors = 0;
        let chips = roots.into_iter().enumerate().filter_map(|(ix, root)| {
            let root = root?;
            let color = crate::tab_groups::group_color(colors, cx);
            colors += 1;
            let active = ix == self.project;
            Some(
                h_flex()
                    .id(("project", ix))
                    .flex_none()
                    .h(px(24.))
                    .pl_2()
                    .pr_0p5()
                    .gap_1p5()
                    .rounded(cx.theme().radius)
                    .cursor_pointer()
                    .when(active, |el| el.bg(cx.theme().list_active))
                    .when(!active, |el| {
                        el.text_color(cx.theme().muted_foreground)
                            .hover(|el| el.bg(cx.theme().list_hover))
                    })
                    .child(div().size(px(8.)).rounded_full().bg(color))
                    .child(div().max_w(px(160.)).truncate().child(project_name(&root)))
                    .child(
                        Button::new(("close-project", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .tooltip(if only {
                                "Close Project"
                            } else {
                                "Close Project and Its Tabs"
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_project(ix, window, cx);
                            })),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.leave_welcome(PANEL_AND_TABS, cx);
                        this.activate_project(ix, cx);
                    })),
            )
        });
        h_flex()
            .id("project-switcher")
            .min_w_0()
            .gap_1()
            .overflow_x_scroll()
            .children(chips)
            .child(
                Button::new("add-project")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Plus)
                    .tooltip_with_action(
                        "Open Another Project in This Window",
                        &AddProject,
                        Some("Workspace"),
                    )
                    .on_click(|_, window, cx| {
                        crate::cli::prompt_open_folder(Some(window.window_handle()), cx)
                    }),
            )
            .into_any_element()
    }

    fn render_layout_toggles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let l = self.layout;
        let toggle = |id: &'static str,
                      icon: Lucide,
                      tooltip: &'static str,
                      action: &dyn Action,
                      on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip_with_action(tooltip, action, Some("Workspace"))
                .selected(on)
        };
        let pending = self.updater.read(cx).pending().map(|r| r.version);
        h_flex()
            .gap_0p5()
            .children(pending.map(|v| {
                Button::new("restart-update")
                    .primary()
                    .xsmall()
                    .icon(Lucide::Download)
                    .label("Restart to Update")
                    .tooltip_with_action(
                        format!("Install Sawit {v} and Relaunch"),
                        &InstallUpdate,
                        Some("Workspace"),
                    )
                    .on_click(
                        cx.listener(|this, _, _, cx| {
                            this.updater.update(cx, |u, cx| u.restart(cx))
                        }),
                    )
            }))
            .child(
                Button::new("open-changelog")
                    .ghost()
                    .xsmall()
                    .label(format!("v{}", Version::current()))
                    .tooltip_with_action("Version & Changelog", &OpenChangelog, Some("Workspace"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_changelog_page(cx))),
            )
            .when(!self.cli_status.read(cx).installed, |el| {
                el.child(
                    Button::new("install-cli")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::SquareTerminal)
                        .label("Install CLI")
                        .tooltip_with_action(
                            "Install the ‘sawit’ Command to Open Folders from a Terminal",
                            &crate::cli::InstallCli,
                            Some("Workspace"),
                        )
                        .on_click(|_, _, cx| crate::cli::install_cli(cx)),
                )
            })
            .child(
                Button::new("open-canvas")
                    .ghost()
                    .xsmall()
                    .icon(crate::canvas::ICON)
                    .label("Canvas")
                    .tooltip_with_action(
                        "Brainstorm on a Whiteboard",
                        &OpenCanvas,
                        Some("Workspace"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.open_canvas(cx);
                    })),
            )
            .child(
                Button::new("open-browser")
                    .ghost()
                    .xsmall()
                    .icon(crate::browser::ICON)
                    .label("Browser")
                    .tooltip("Preview Your App in a Browser an Agent Can Drive")
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_browser(None, window, cx)),
                    ),
            )
            .child(
                Button::new("open-tools")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Package)
                    .label("Tools")
                    .tooltip_with_action(
                        "Install Dev Tools: Claude Code, Node.js, Bun, uv and More",
                        &OpenDevTools,
                        Some("Workspace"),
                    )
                    .on_click(cx.listener(|this, _, window, cx| this.open_tools_page(window, cx))),
            )
            .child(
                Button::new("open-mcp")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Plug)
                    .label("MCP")
                    .tooltip_with_action(
                        "Set Up the MCP Server for Claude, Codex and More",
                        &OpenMcp,
                        Some("Workspace"),
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.open_mcp_page(cx))),
            )
            .child(div().w(px(6.)))
            .child(
                toggle(
                    "toggle-sidebar",
                    Lucide::PanelLeft,
                    "Sidebar",
                    &ToggleSidebar,
                    l.sidebar,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx))),
            )
            .child(
                toggle(
                    "toggle-center",
                    Lucide::PanelTop,
                    "Tabs & Tables",
                    &ToggleCenter,
                    l.center,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Center, cx))),
            )
            .child(
                toggle(
                    "toggle-terminal",
                    Lucide::PanelBottom,
                    "Terminal / herdr",
                    &ToggleTerminal,
                    l.terminal,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_area(Area::Terminal, cx))),
            )
    }

    fn render_center(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let show_tabs = !self.canvas_maximized;
        v_flex()
            .size_full()
            .when(show_tabs, |el| el.child(self.render_tab_bar(cx)))
            .child(div().flex_1().min_h_0().child(self.render_active_item(cx)))
    }

    fn render_body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if let Some(welcome) = &self.welcome {
            return welcome.clone().into_any_element();
        }
        let l = self.layout;
        let maximized = l == TERMINAL_ONLY;
        self.terminal.update(cx, |t, cx| {
            if t.maximized != maximized {
                t.maximized = maximized;
                cx.notify();
            }
        });
        // The terminal stays alive while hidden; only its view is dropped.
        let main = match (l.center, l.terminal) {
            (true, true) => v_resizable("workspace-v")
                .child(resizable_panel().child(self.render_center(cx)))
                .child(
                    resizable_panel()
                        .size(px(240.))
                        .size_range(px(80.)..Pixels::MAX)
                        .child(cached(self.terminal.clone().into())),
                )
                .into_any_element(),
            (true, false) => self.render_center(cx).into_any_element(),
            _ => cached(self.terminal.clone().into()),
        };
        if !l.sidebar {
            return main;
        }
        h_resizable("workspace-h")
            .child(
                resizable_panel()
                    .size(px(240.))
                    .size_range(px(160.)..px(480.))
                    .child(
                        v_flex()
                            .size_full()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .child(self.render_sidebar_tabs(cx))
                            .child(div().flex_1().min_h_0().child(self.render_sidebar())),
                    ),
            )
            .child(resizable_panel().child(main))
            .into_any_element()
    }

    fn on_close_action(&mut self, op: CloseOp, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.active;
        self.close(op, target, window, cx);
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            // Keep workspace shortcuts working when nothing inside has focus.
            self.focus.focus(window, cx);
        }
        self.sync_canvas(cx);
        self.sync_browser(cx);
        v_flex()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &CloseActiveTab, w, cx| {
                this.on_close_action(CloseOp::This, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseOtherTabs, w, cx| {
                this.on_close_action(CloseOp::Others, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsLeft, w, cx| {
                this.on_close_action(CloseOp::Left, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseTabsRight, w, cx| {
                this.on_close_action(CloseOp::Right, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseCleanTabs, w, cx| {
                this.on_close_action(CloseOp::Clean, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseAllTabs, w, cx| {
                this.on_close_action(CloseOp::All, w, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleSidebar, _, cx| this.toggle_area(Area::Sidebar, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, _, cx| this.toggle_area(Area::Terminal, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ToggleCenter, _, cx| this.toggle_area(Area::Center, cx)),
            )
            .on_action(cx.listener(|this, _: &TogglePinTab, _, cx| {
                let ix = this.active;
                this.toggle_pin(ix, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ToggleMaximizePane, w, cx| this.toggle_maximize_pane(w, cx)),
            )
            .on_action(cx.listener(|this, _: &NextTab, _, cx| this.cycle_tab(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousTab, _, cx| this.cycle_tab(-1, cx)))
            .on_action(cx.listener(|this, _: &NextProject, _, cx| this.cycle_project(1, cx)))
            .on_action(cx.listener(|this, _: &PreviousProject, _, cx| this.cycle_project(-1, cx)))
            .on_action(cx.listener(|_, _: &AddProject, w, cx| {
                crate::cli::prompt_open_folder(Some(w.window_handle()), cx)
            }))
            .on_action(cx.listener(|this, _: &CloseProject, w, cx| {
                let ix = this.project;
                this.close_project(ix, w, cx)
            }))
            .on_action(cx.listener(|this, _: &CopyProjectPath, _, cx| {
                if let Some(root) = this.root(cx) {
                    this.copy_project_path(root.display().to_string(), cx)
                }
            }))
            .on_action(cx.listener(|this, _: &NewTerminal, w, cx| this.new_terminal(w, cx)))
            .on_action(cx.listener(|this, _: &ShowInstances, _, cx| {
                this.show_panel(SidebarTab::Servers, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowFiles, _, cx| this.show_panel(SidebarTab::Files, cx)),
            )
            .on_action(cx.listener(|this, _: &ShowGit, _, cx| this.show_panel(SidebarTab::Git, cx)))
            .on_action(cx.listener(|this, _: &ShowDatabase, _, cx| {
                this.show_panel(SidebarTab::Database, cx)
            }))
            .on_action(cx.listener(|this, _: &ShowRun, _, cx| this.show_panel(SidebarTab::Run, cx)))
            .on_action(
                cx.listener(|this, _: &ShowPorts, _, cx| this.show_panel(SidebarTab::Ports, cx)),
            )
            .on_action(
                cx.listener(|this, _: &ShowNetwork, _, cx| {
                    this.show_panel(SidebarTab::Network, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ShowSsh, _, cx| this.show_panel(SidebarTab::Ssh, cx)))
            .on_action(cx.listener(|this, _: &ShowShellConfig, _, cx| {
                this.show_panel(SidebarTab::ShellConfig, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowPrompts, _, cx| {
                    this.show_panel(SidebarTab::Prompts, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenCanvas, _, cx| {
                this.open_canvas(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenDevTools, w, cx| this.open_tools_page(w, cx)))
            .on_action(cx.listener(|this, _: &OpenMcp, _, cx| this.open_mcp_page(cx)))
            .on_action(cx.listener(|this, _: &OpenChangelog, _, cx| {
                this.show_center(cx);
                this.open_changelog_page(cx)
            }))
            .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| {
                // Installs a pending update, else checks for one and shows
                // the changelog page, where the result appears.
                if this.updater.read(cx).pending().is_some() {
                    this.updater.update(cx, |u, cx| u.restart(cx));
                } else {
                    this.updater.update(cx, |u, cx| u.check(cx));
                    this.show_center(cx);
                    this.open_changelog_page(cx);
                }
            }))
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    // Fullscreen hides the traffic lights: drop the space kept for them.
                    .when(window.is_fullscreen(), |this| this.pl_0())
                    .child(
                        h_flex()
                            .size_full()
                            .pr_2()
                            .gap_2()
                            .text_sm()
                            .child(img(crate::assets::LOGO_MARK).size(px(16.)))
                            .child(div().font_semibold().child("Sawit"))
                            .child(self.render_project_switcher(cx))
                            .child(div().flex_1())
                            .child(self.render_layout_toggles(cx)),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .child(self.render_status_bar(cx))
    }
}

/// Mounts a pane so it re-renders only when it notifies, not every time the
/// workspace does (terminal output, a cursor blink or a poll elsewhere
/// would otherwise rebuild the whole window).
fn cached(view: AnyView) -> AnyElement {
    view.cached(StyleRefinement::default().size_full())
        .into_any_element()
}

/// The project that `file` is the canvas file of (spec D5), among `roots`, the folders of the
/// window's projects: `Some` of its folder as `roots` has it, which is `None` for a project with no
/// folder (its canvas is the app's own, in `app_root`). `None` when it is no project's.
fn canvas_project(
    file: &Path,
    roots: impl IntoIterator<Item = Option<PathBuf>>,
    app_root: &Path,
) -> Option<Option<PathBuf>> {
    roots
        .into_iter()
        .find(|root| board_file(root.as_deref(), app_root) == file)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use super::{bindings, canvas_passthrough_keys, canvas_project, page_key};

    #[test]
    fn a_projects_canvas_file_is_that_projects_canvas() {
        let app = Path::new("/support/sawit");
        let (a, b) = (PathBuf::from("/work/a"), PathBuf::from("/work/b"));
        let roots = vec![Some(a.clone()), Some(b.clone()), None];
        let project = |file: &str| canvas_project(Path::new(file), roots.clone(), app);
        // Each project's canvas file, and for the project with no folder the app's own.
        assert_eq!(project("/work/b/.nyawit/canvas.figcraft"), Some(Some(b)));
        assert_eq!(project("/work/a/.nyawit/canvas.figcraft"), Some(Some(a)));
        assert_eq!(project("/support/sawit/canvas.figcraft"), Some(None));
        // Any other file is one of its own: another file in `.nyawit`, a canvas file elsewhere,
        // the canvas file of a project the window doesn't have.
        assert_eq!(project("/work/a/.nyawit/sketch.figcraft"), None);
        assert_eq!(project("/work/a/canvas.figcraft"), None);
        assert_eq!(project("/work/c/.nyawit/canvas.figcraft"), None);
        // With every project in a folder, the app's canvas file is one of its own too.
        let app_canvas = Path::new("/support/sawit/canvas.figcraft");
        let folders = [Some(PathBuf::from("/work/a"))];
        assert_eq!(canvas_project(app_canvas, folders, app), None);
    }

    #[test]
    fn no_two_shortcuts_share_a_keystroke() {
        let mut seen = HashSet::new();
        let global = ["cmd-q", "shift-cmd-n", "cmd-o"];
        for binding in bindings() {
            let keys: Vec<String> = binding
                .keystrokes()
                .iter()
                .map(|k| page_key(k.modifiers(), k.key()))
                .collect();
            let keys = keys.join(" ");
            assert!(
                !global.contains(&keys.as_str()),
                "{keys} is taken by the app"
            );
            assert!(seen.insert(keys.clone()), "{keys} is bound twice");
        }
        // A chord's first keystroke can't be a shortcut of its own.
        for chord in seen.iter().filter(|k| k.contains(' ')) {
            let first = chord.split(' ').next().unwrap();
            assert!(!seen.contains(first), "{first} shadows {chord}");
        }
    }

    #[test]
    fn the_canvas_passes_on_app_shortcuts_but_not_its_own() {
        let keys = canvas_passthrough_keys();
        for k in [
            "cmd-j",
            "shift-cmd-j",
            "cmd-1",
            "ctrl-cmd-c",
            "ctrl-tab",
            "cmd-o",
        ] {
            assert!(keys.iter().any(|x| x == k), "{k} missing");
        }
        // Excalidraw's own: reset zoom, add link; ⇧⌘↩ is handled by the page.
        for k in ["cmd-0", "cmd-k", "shift-cmd-enter"] {
            assert!(!keys.iter().any(|x| x == k), "{k} passed on");
        }
    }
}
