//! The main window, laid out like Zed: title bar, left panel with tabs
//! (Instances / Files / Git / Database / Run / Ports / Network / Prompts / SSH), tabbed center
//! pane, bottom terminal dock and status bar. A new, empty window starts on the welcome screen instead.
//! One window holds several projects (see `project`): the title bar switches
//! between them and tabs are grouped by the project they belong to.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu, PopupMenuItem};
use gpui_kit::component::resizable::{h_resizable, resizable_panel, v_resizable};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::board::{BoardView, ToggleCanvasMaximize};
use crate::browser::BrowserView;
use crate::changelog_page::ChangelogPage;
use crate::cli::{CliStatus, cli_status};
use crate::db::{DatabasePanel, DbEvent, DbStore, DiagramView, SqlView, TableView};
use crate::editor::{FileEditor, FileEditorEvent};
use crate::files::search::{FolderSearch, SearchEvent};
use crate::files::{FilesEvent, FilesPanel};
use crate::git::{
    CommitView, DiffView, DiffViewEvent, GitHubAccount, GitHubView, GitPanel, GitPanelEvent,
    GitStore, GraphEvent, GraphView, OpenGitHub, SignInToGitHub,
};
use crate::keymap::editor::KeymapEditor;
use crate::keymap::{OpenKeymapEditor, OpenKeymapFile};
use crate::layout::{Area, Layout};
use crate::mcp_page::McpPage;
use crate::network::NetworkPanel;
use crate::ports::{PortsEvent, PortsPanel};
use crate::prompts::{PromptStore, PromptStoreEvent, PromptView, PromptsEvent, PromptsPanel};
use crate::run::{Launch, RunEvent, RunPanel, RunStore};
use crate::servers::{ServerDetail, ServerList, ServerStore, ShowServerTab};
use crate::shell_config::{ShellConfigEvent, ShellConfigPanel};
use crate::skills_page::SkillsPage;
use crate::ssh::{SshEvent, SshHostEvent, SshHostView, SshPanel, SshStore, SshStoreEvent};
use crate::states::Placeholder;
use crate::tab_groups::{group_header, group_stripe, project_name};
use crate::tabs::{
    CloseOp, Drawn, Slot, TabFlags, active_after, active_after_move, insert_index, strip, to_close,
};
use crate::terminal::{Origin, TerminalPanel, ToggleMaximize};
use crate::tools_page::{ToolsEvent, ToolsPage};
use crate::update::{Updater, updater};
use crate::viewers::{ImageView, PdfView, VideoView};
use crate::welcome::{Welcome, WelcomeEvent};
use sawit_board::store::{self, board_file};
use sawit_core::files::{FileKind, classify};
use sawit_core::instances::AppPaths;
use sawit_core::recent::RecentProjects;
use sawit_core::terminal::PaneId;
use sawit_core::update::Version;

mod control;
mod explorer;
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
        ShowConfigFiles,
        ShowPrompts,
        OpenCanvas,
        OpenDevTools,
        OpenMcp,
        OpenChangelog,
        InstallUpdate,
        OpenLocalExplorer
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
        KeyBinding::new("cmd-9", ShowConfigFiles, ctx),
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
        KeyBinding::new("ctrl-cmd-e", OpenLocalExplorer, ctx),
        KeyBinding::new("ctrl-cmd-i", crate::cli::InstallCli, ctx),
        KeyBinding::new("ctrl-cmd-a", crate::agents::ToggleAgentCursors, ctx),
        // Zed's.
        KeyBinding::new("cmd-k cmd-s", OpenKeymapEditor, ctx),
    ]
}

pub fn init(cx: &mut App) {
    cx.bind_keys(bindings());
    crate::browser::init(cx);
    crate::board::init(cx);
}

/// Keeps GPUI's keyboard on screen. Whatever had it and is no longer drawn gives it to `home`
/// (the workspace): the browser's page after a click in it, a file editor, a terminal pane or a
/// sidebar panel's field, once their tab, the tab area, the dock or the panel is off screen
/// (⌃Tab, ⇧⌘J, ⌘J, ⌘1 to ⌘0, a page an agent puts in front, an agent's `app_activate_tab`).
/// Otherwise the keyboard stays on what isn't drawn: GPUI then sends the keys from the window's
/// root, gpui-kit's `Root`, past every workspace shortcut (⇧⌘J, ⌘W, ⌘T, ⌘1), and typing goes
/// nowhere, until a click. Zed's workspace does the same.
///
/// What it took is given back ([`Self::give_back`]) once what the workspace shows changes while
/// the workspace still has the keyboard: a terminal pane hidden and shown again (⌘J twice, the
/// title bar's button twice) or a file's text whose tab is left and come back to has it again,
/// and typing goes on. It is forgotten as soon as anything else has the keyboard. Given back
/// while still off screen (⌃Tab on to a third tab, ⌘J then ⌃Tab), it is parked again and the
/// keyboard comes back here once the frame is drawn.
pub(crate) struct KeyboardHome<S> {
    home: FocusHandle,
    /// What the keyboard was last taken from.
    parked: Rc<RefCell<Option<WeakFocusHandle>>>,
    /// What the workspace showed at its last render.
    shown: Option<S>,
    _lost: Subscription,
}

impl<S: PartialEq> KeyboardHome<S> {
    pub(crate) fn new<V: 'static>(
        home: FocusHandle,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let parked = Rc::new(RefCell::new(None));
        let _lost = cx.on_focus_lost(window, {
            let (home, parked) = (home.clone(), parked.clone());
            move |_, window, cx| {
                // Still the handle that is no longer drawn.
                *parked.borrow_mut() = window.focused(cx).map(|h| h.downgrade());
                window.focus(&home, cx);
            }
        });
        Self {
            home,
            parked,
            shown: None,
            _lost,
        }
    }

    /// From the workspace's render, once the canvases have taken or given their keyboard:
    /// `shown` is what the workspace shows now. `keep` says what may have the keyboard given
    /// back: not the browser's page, which gave AppKit's keyboard back when hidden, so that
    /// GPUI's alone would type nowhere, nor a keystroke recorder, which takes every key; they
    /// wait for a click, as before.
    pub(crate) fn give_back(
        &mut self,
        shown: S,
        keep: impl Fn(&FocusHandle, &App) -> bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let changed = self.shown.as_ref() != Some(&shown);
        self.shown = Some(shown);
        if !self.home.is_focused(window) {
            // Something else has the keyboard: the user went on from what was hidden.
            self.parked.borrow_mut().take();
            return;
        }
        if !changed {
            // Nothing changed on screen (a store's redraw, a chord's first key): it waits.
            return;
        }
        let back = self.parked.borrow_mut().take().and_then(|h| h.upgrade());
        let Some(back) = back.filter(|back| keep(back, cx)) else {
            return;
        };
        window.focus(&back, cx);
        // GPUI's focus-lost fallback runs only for a frame after one with a keyboard path, and
        // the frame that hid `back` had none: if `back` isn't drawn in this one either, nothing
        // would take the keyboard from it. Check once the frame is drawn (review I5).
        let (home, parked) = (self.home.clone(), self.parked.clone());
        let handle = window.window_handle();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                if back.is_focused(window) && !home.contains_focused(window, cx) {
                    *parked.borrow_mut() = Some(back.downgrade());
                    window.focus(&home, cx);
                }
            });
        });
    }
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
    /// The projects' canvas tabs (owned by their tabs), one per project at most.
    boards: Vec<WeakEntity<BoardView>>,
    /// The canvas fills the window: no sidebar, terminal or tab bar.
    canvas_maximized: bool,
    /// The browser tabs' views (owned by their tabs): one per project at most, and one for
    /// no project.
    browsers: Vec<WeakEntity<BrowserView>>,
    /// Shown instead of the panels until something is opened.
    welcome: Option<Entity<Welcome>>,
    tab_scroll: ScrollHandle,
    /// The active tab last scrolled into view: the view it shows, and its index. Another tab, or
    /// the same one moved, is scrolled to; a fold or a scroll by hand leaves the bar alone.
    scrolled_to: Option<(EntityId, usize)>,
    /// A table/SQL tab asked for while its connection was being re-made.
    pending_db: Option<DbEvent>,
    cli_status: Entity<CliStatus>,
    updater: Entity<Updater>,
    /// The app's one GitHub sign-in, which the title bar shows.
    github: Entity<GitHubAccount>,
    focus: FocusHandle,
    /// Takes the keyboard from what goes off screen, and gives it back when it is shown again.
    keyboard: KeyboardHome<Shown>,
    _subscriptions: Vec<Subscription>,
}

/// What the workspace shows, as far as the keyboard goes: the welcome screen, the layout, the
/// filled canvas, the project, the sidebar's panel and the view of the tab in front.
type Shown = (bool, Layout, bool, usize, SidebarTab, Option<EntityId>);

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
        // Closing the window ends its terminals: ask while one runs something.
        crate::terminal::ask_before_closing(window, terminal.downgrade(), cx);
        // SSH sessions reconnect with their server's settings as they are now.
        let saved_servers = ssh_store.downgrade();
        terminal.update(cx, |t, _| {
            t.set_remote_source(move |key, cx| {
                let store = saved_servers.upgrade().ok_or("Sawit is closing.")?;
                store.update(cx, |s, cx| s.remote(key, cx))
            })
        });
        let shell_config_panel = cx.new(|cx| ShellConfigPanel::new(window, cx));
        let cli_status = cli_status(cx);
        let updater = updater(cx);
        let github = crate::git::account::global(cx);
        let welcome = cx.new(|cx| Welcome::new(servers.clone(), db.clone(), cx));
        let focus = cx.focus_handle();
        let keyboard = KeyboardHome::new(focus.clone(), window, cx);
        let subscriptions = vec![
            cx.observe(&servers, |_, _, cx| cx.notify()),
            cx.observe(&cli_status, |_, _, cx| cx.notify()),
            cx.observe(&updater, |_, _, cx| cx.notify()),
            cx.observe(&github, |_, _, cx| cx.notify()),
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
                        this.open_browser(Some(url.clone()), window, cx);
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
                    this.toggle_terminal_maximized(Origin::User, window, cx)
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
                        let _ = this.connect_ssh(key.clone(), *new, Origin::User, window, cx);
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
            boards: Vec::new(),
            canvas_maximized: false,
            browsers: Vec::new(),
            welcome: Some(welcome),
            tab_scroll: ScrollHandle::new(),
            scrolled_to: None,
            pending_db: None,
            focus,
            keyboard,
            cli_status,
            updater,
            github,
            _subscriptions: subscriptions,
        }
    }

    /// The active project's folder, if it has one.
    pub fn root(&self, cx: &App) -> Option<PathBuf> {
        self.current().root(cx)
    }

    /// The window's terminal dock.
    pub fn terminal(&self) -> &Entity<TerminalPanel> {
        &self.terminal
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

    /// Opens `kept.path` with the unsaved changes the app kept for it when it last quit without
    /// asking ([`crate::editor::kept`]): they're back in its buffer, unsaved, and `from`, the file
    /// that kept them, goes once they're in.
    pub fn restore_kept(
        &mut self,
        kept: crate::editor::kept::Kept,
        from: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_at(kept.path.clone(), None, window, cx);
        let editor = self.tabs.iter().find_map(|t| match &t.item {
            PaneItem::File(e) if e.read(cx).path == kept.path => Some(e.clone()),
            _ => None,
        });
        if let Some(editor) = editor {
            editor.update(cx, |e, cx| e.restore(kept, from, window, cx));
        }
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let kind = classify(&path);
        // Canvas files open on a canvas (spec D14), which takes the keyboard, as a file editor
        // does once its file has loaded (CANVAS-UI-2).
        if matches!(kind, FileKind::Canvas | FileKind::Excalidraw) {
            if let Some(board) = self.open_board_file(path, kind, cx) {
                board.update(cx, |board, cx| board.take_keyboard(cx));
            }
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
                // As a text file's tab: shown with its project.
                self.activate_tab(ix, cx);
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

    /// Opens a canvas file in a canvas tab (spec D14): a `.sawit` file bound to itself, or an
    /// `.excalidraw` file imported and bound to the `<stem>.sawit` beside it
    /// ([`BoardView::import`]).
    ///
    /// A file has one canvas in the window, whichever of its two names opens it, because two
    /// canvases of one file would each save over the other's edits. A project's canvas file is
    /// that project's canvas tab ([`Self::open_canvas_of`]); any other file's tab is keyed by the
    /// canvas file it is bound to ([`store::bound_file`]), and shown again when the file is opened
    /// again. The canvas shown.
    fn open_board_file(
        &mut self,
        path: PathBuf,
        kind: FileKind,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
        let file = store::bound_file(&path);
        let roots: Vec<Option<PathBuf>> = self.projects.iter().map(|p| p.root(cx)).collect();
        if let Some(root) = canvas_project(&file, roots, &AppPaths::default_macos().root) {
            return self.open_canvas_of(root, cx);
        }
        if let Some((ix, board)) = self.board_tab(&file, cx) {
            self.activate_tab(ix, cx);
            return Some(board);
        }
        let view = cx.new(|cx| match kind {
            FileKind::Excalidraw => BoardView::import(path, cx),
            _ => BoardView::open(file.clone(), cx),
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
                view: view.clone().into(),
                key: format!("file\u{0}{}", file.display()).into(),
                title,
                icon: crate::board::ICON,
                file: Some(file),
            },
            group,
            cx,
        );
        cx.notify();
        Some(view)
    }

    /// Whether a canvas tab of this window shows the canvas of the canvas file at `path`
    /// ([`Self::board_tab`]): an `.excalidraw` file's is the one bound to the `.sawit` file
    /// beside it. An agent's op on that file goes to this window ([`crate::cli`]).
    pub(crate) fn shows_canvas_file(&self, path: &Path, cx: &App) -> bool {
        self.board_tab(&store::bound_file(path), cx).is_some()
    }

    /// The tab of the canvas bound to the canvas file `file` ([`BoardView::path`]), and the
    /// canvas, whatever opened it: a project's canvas tab, or a canvas file's. Named through a
    /// link or as written, it is one file ([`same_canvas_file`]).
    fn board_tab(&self, file: &Path, cx: &App) -> Option<(usize, Entity<BoardView>)> {
        self.tabs.iter().enumerate().find_map(|(ix, tab)| {
            let PaneItem::Other { view, .. } = &tab.item else {
                return None;
            };
            let board = view.clone().downcast::<BoardView>().ok()?;
            same_canvas_file(board.read(cx).path(), file).then_some((ix, board))
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

    /// Shows the tab at `ix` as the user's click on it or ⌃Tab does ([`Self::activate_tab`]): a
    /// canvas's takes the keyboard too, as the canvas the user went to ([`BoardView::take_keyboard`],
    /// CANVAS-UI-2).
    fn show_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.activate_tab(ix, cx);
        if let Some(board) = self.board_at(ix) {
            board.update(cx, |board, cx| board.take_keyboard(cx));
        }
    }

    /// The canvas of the tab at `ix`, if it is a canvas's.
    fn board_at(&self, ix: usize) -> Option<Entity<BoardView>> {
        let PaneItem::Other { view, .. } = &self.tabs.get(ix)?.item else {
            return None;
        };
        view.clone().downcast::<BoardView>().ok()
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
                dirty: match &t.item {
                    PaneItem::File(e) => e.read(cx).dirty,
                    // Annotation notes not yet copied or sent; a Dev Tools tab that
                    // installs something counts too: closing it stops that.
                    PaneItem::Other { view, .. } => {
                        view.clone()
                            .downcast::<BrowserView>()
                            .is_ok_and(|b| b.read(cx).unsent_notes(cx) > 0)
                            || tools_work(&t.item, cx).is_some()
                            || skills_work(&t.item, cx).is_some()
                    }
                    PaneItem::Server => false,
                },
            })
            .collect()
    }

    /// Closes tabs per `op` relative to `target`. Closing a single tab with
    /// unsaved changes (or a browser with notes not yet sent), or the Dev Tools
    /// tab while it installs, asks first; bulk closes leave such tabs open.
    /// Returns what the question that went up is about, if one did: the tab
    /// then closes only once the user agrees.
    fn close(
        &mut self,
        op: CloseOp,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<&'static str> {
        let closing = to_close(op, target, &self.flags(cx));
        if let [ix] = closing[..]
            && let Some(PaneItem::File(editor)) = self.tabs.get(ix).map(|t| t.item.clone())
            && editor.read(cx).dirty
        {
            self.confirm_close_dirty(editor, window, cx);
            return Some("the file has unsaved changes");
        }
        if let [ix] = closing[..]
            && let Some(PaneItem::Other { view, .. }) = self.tabs.get(ix).map(|t| t.item.clone())
            && let Ok(browser) = view.downcast::<BrowserView>()
            && browser.read(cx).unsent_notes(cx) > 0
        {
            self.confirm_close_browser(browser, window, cx);
            return Some("the browser has annotations not yet copied or sent");
        }
        if let [ix] = closing[..]
            && let Some(work) = self.tabs.get(ix).and_then(|t| tools_work(&t.item, cx))
        {
            self.confirm_close_tools(work, window, cx);
            return Some("Dev Tools is installing, and closing the tab stops it");
        }
        if let [ix] = closing[..]
            && let Some(work) = self.tabs.get(ix).and_then(|t| skills_work(&t.item, cx))
        {
            self.confirm_close_skills(work, window, cx);
            return Some("Skills is installing, and closing the tab stops it");
        }
        self.remove_tabs(&closing, cx);
        None
    }

    /// Asks before closing a browser whose annotation notes weren't copied or sent: they
    /// live only in the tab.
    fn confirm_close_browser(
        &mut self,
        browser: Entity<BrowserView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let notes = browser.read(cx).unsent_notes(cx);
        let answer = window.prompt(
            PromptLevel::Warning,
            "Close the browser with its annotations?",
            Some(&crate::browser::notes_lost(notes)),
            &["Close Anyway", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if let Ok(0) = answer.await {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(|t| {
                        matches!(&t.item, PaneItem::Other { view, .. }
                            if view.clone().downcast::<BrowserView>().ok().as_ref() == Some(&browser))
                    }) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Asks before closing the Dev Tools tab while it installs: closing it
    /// stops the installer, and what's queued doesn't run.
    fn confirm_close_tools(&mut self, work: String, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Stop the Dev Tools installs?",
            Some(&format!(
                "{work}. Closing the tab stops it, and what's queued doesn't run."
            )),
            &["Stop and Close", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(
                        |t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == "tools"),
                    ) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Asks before closing the Skills tab while it installs: closing it stops that.
    fn confirm_close_skills(&mut self, work: String, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            "Stop the skill installs?",
            Some(&format!(
                "{work}. Closing the tab stops it, and what's queued doesn't run."
            )),
            &["Stop and Close", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    if let Some(ix) = this.tabs.iter().position(
                        |t| matches!(&t.item, PaneItem::Other { key, .. } if key.as_ref() == "skills"),
                    ) {
                        this.remove_tabs(&[ix], cx);
                    }
                });
            }
        })
        .detach();
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
        let keys = |action: &dyn Action| crate::keymap::keys_for(action, "Workspace", window);
        let [
            close_keys,
            others_keys,
            left_keys,
            right_keys,
            clean_keys,
            all_keys,
            pin_keys,
        ] = [
            &CloseActiveTab as &dyn Action,
            &CloseOtherTabs,
            &CloseTabsLeft,
            &CloseTabsRight,
            &CloseCleanTabs,
            &CloseAllTabs,
            &TogglePinTab,
        ]
        .map(keys);
        let item = |label: &'static str, keys: SharedString| {
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .w_full()
                    .gap_6()
                    .justify_between()
                    .child(label)
                    .child(div().text_color(muted).child(keys.clone()))
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
        menu.item(item("Close", close_keys).on_click(close(CloseOp::This)))
            .item(item("Close Others", others_keys).on_click(close(CloseOp::Others)))
            .separator()
            .item(item("Close Left", left_keys).on_click(close(CloseOp::Left)))
            .item(item("Close Right", right_keys).on_click(close(CloseOp::Right)))
            .separator()
            .item(item("Close Clean", clean_keys).on_click(close(CloseOp::Clean)))
            .item(item("Close All", all_keys).on_click(close(CloseOp::All)))
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
                item(if pinned { "Unpin Tab" } else { "Pin Tab" }, pin_keys).on_click(
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
                "Config Files",
                SidebarTab::ShellConfig,
                &ShowConfigFiles,
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
                    .role(Role::Tab)
                    .accessibility_label(tooltip)
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
            let roots = self.roots(cx);
            self.shell_config_panel
                .update(cx, |p, cx| p.set_projects(roots, cx));
        }
        cx.notify();
    }

    /// Shows the tab `step` places after (or before) the active one.
    fn cycle_tab(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.tabs.len() as isize;
        if len > 1 {
            let ix = (self.active as isize + step).rem_euclid(len) as usize;
            self.show_center(cx);
            self.show_tab(ix, cx);
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
        let canvas_shown = self.welcome.is_none() && self.layout.center && self.canvas_active();
        if canvas_shown && !terminal_focused || self.canvas_maximized {
            self.toggle_canvas_maximized(cx);
        } else {
            self.toggle_terminal_maximized(Origin::User, window, cx);
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

    /// The view the tab at `ix` shows: what tells it apart from a tab that takes its place. The
    /// tab bar's scroll and the keyboard's give-back ([`Shown`]) both go by it.
    fn tab_view(&self, ix: usize) -> Option<EntityId> {
        Some(match &self.tabs.get(ix)?.item {
            PaneItem::Server => self.server_detail.entity_id(),
            PaneItem::File(editor) => editor.entity_id(),
            PaneItem::Other { view, .. } => view.entity_id(),
        })
    }

    /// The header in front of the run of `root`'s tabs that starts at tab `first`, `count` long:
    /// a click folds the group, and its menu closes the group's tabs.
    fn render_group_header(
        &self,
        first: usize,
        count: usize,
        root: PathBuf,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let color = self.group_color(&root, cx);
        let collapsed = self.collapsed.contains(&root);
        let (toggle, close) = (root.clone(), root.clone());
        group_header(
            ("tab-group", first),
            project_name(&root),
            color,
            collapsed,
            count,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(toggle.clone(), cx)))
        .context_menu(move |menu, _, _| {
            let weak = weak.clone();
            let close = close.clone();
            menu.item(
                PopupMenuItem::new("Close Tabs in Group").on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |ws, cx| ws.close_group(&close, cx));
                }),
            )
        })
        .into_any_element()
    }

    fn render_tab_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let weak = cx.entity().downgrade();
        let projects = self.roots(cx);
        // A header in front of each group's run of tabs; a folded group shows its active tab only.
        let drawn = strip(&self.slots(), self.active, |g| self.collapsed.contains(g));
        let mut items: Vec<AnyElement> = Vec::with_capacity(drawn.len());
        // The active tab's place among the bar's children, which a scroll goes by.
        let mut active_child = None;
        for item in drawn {
            let ix = match item {
                Drawn::Tab(ix) => ix,
                Drawn::Header { first, count } => {
                    if let Some(root) = self.tabs[first].group.clone() {
                        items.push(self.render_group_header(first, count, root, cx));
                    }
                    continue;
                }
            };
            let tab = &self.tabs[ix];
            let group = tab.group.clone().filter(|_| !tab.pinned);
            let color = group.as_ref().map(|g| self.group_color(g, cx));
            let active = ix == self.active;
            if active {
                active_child = Some(items.len());
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
                    .role(Role::Tab)
                    .aria_label(title.clone())
                    .aria_selected(active)
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
                            .accessibility_label("Unpin")
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
                            .accessibility_label("Close Tab")
                            .tooltip_with_action("Close", &CloseActiveTab, Some("Workspace"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close(CloseOp::This, ix, window, cx);
                            }))
                    })
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_tab(ix, cx)))
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
        // Bring a newly opened or activated tab into view, by its place among the bar's children:
        // the group headers and the folded tabs make that differ from its index (SMOKE-1).
        let key = self.tab_view(self.active).map(|view| (view, self.active));
        if let (Some(key), Some(child)) = (key, active_child)
            && self.scrolled_to != Some(key)
        {
            self.tab_scroll.scroll_to_item(child);
            self.scrolled_to = Some(key);
        }
        h_flex()
            .id("tab-bar")
            .role(Role::TabList)
            .aria_label("Tabs")
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

    /// The status bar: instances and the branch on the left; the project's
    /// folder, the CLI and the version on the right.
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
        let muted = cx.theme().muted_foreground;
        let instances = match (running, total) {
            (_, 0) => "No instances".to_string(),
            (0, n) => format!("{n} stopped"),
            (r, n) => format!("{r} of {n} running"),
        };
        h_flex()
            .h(px(26.))
            .px_1p5()
            .gap_1()
            .text_xs()
            .text_color(muted)
            .bg(cx.theme().status_bar)
            .border_t_1()
            .border_color(cx.theme().status_bar_border)
            .child(
                Button::new("status-instances")
                    .ghost()
                    .xsmall()
                    .icon(Icon::new(Lucide::Database).text_color(if running > 0 {
                        cx.theme().success
                    } else {
                        muted
                    }))
                    .label(instances)
                    .tooltip_with_action("Show Instances", &ShowInstances, Some("Workspace"))
                    .on_click(
                        cx.listener(|this, _, _, cx| this.show_panel(SidebarTab::Servers, cx)),
                    ),
            )
            .when_some(branch, |el, (name, sync, changed)| {
                el.child(crate::catalog::divider(cx)).child(
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
                        .tooltip_with_action("Show Git", &ShowGit, Some("Workspace"))
                        .on_click(
                            cx.listener(|this, _, _, cx| this.show_panel(SidebarTab::Git, cx)),
                        ),
                )
            })
            .child(div().flex_1())
            .when_some(project, |el, project| {
                // The full path lives in the tooltip; the bar only offers the copy.
                el.child(
                    Button::new("copy-project-path")
                        .ghost()
                        .xsmall()
                        .icon(if copied { Lucide::Check } else { Lucide::Copy })
                        .label(if copied { "Copied" } else { "Copy Path" })
                        .tooltip_with_action(project.clone(), &CopyProjectPath, Some("Workspace"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy_project_path(project.clone(), cx)
                        })),
                )
                .child(crate::catalog::divider(cx))
            })
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
                Button::new("open-changelog")
                    .ghost()
                    .xsmall()
                    .label(format!("v{}", Version::current()))
                    .tooltip_with_action("Version & Changelog", &OpenChangelog, Some("Workspace"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_changelog_page(cx))),
            )
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

/// ⇧⌘↩ on the canvas: fills the window with it (`maximized`: the tabs alone, without their tab
/// bar), or puts back the layout it replaced. `unmaximized` is the layout to go back to, which
/// the terminal's maximize shares.
fn toggle_canvas_fill(maximized: &mut bool, layout: &mut Layout, unmaximized: &mut Option<Layout>) {
    if *maximized && *layout == CENTER_ONLY {
        *maximized = false;
        *layout = unmaximized.take().unwrap_or_default();
    } else {
        if !*maximized && *layout != TERMINAL_ONLY {
            *unmaximized = Some(*layout);
        }
        *maximized = true;
        *layout = CENTER_ONLY;
    }
}

/// Ends the canvas's fill-window (`maximized`) once the window shows something else: `layout` is
/// no longer the tabs alone (⌘B, ⌘J), or the canvas isn't the tab on screen (`canvas_shown`;
/// another tab, the welcome screen). The tab bar comes back with it, and the layout stays as it
/// was changed to.
fn settle_canvas_fill(
    maximized: &mut bool,
    unmaximized: &mut Option<Layout>,
    layout: Layout,
    canvas_shown: bool,
) {
    if *maximized && !(canvas_shown && layout == CENTER_ONLY) {
        *maximized = false;
        *unmaximized = None;
    }
}

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
            WelcomeEvent::SignInToGitHub => self.sign_in_to_github(window, cx),
            WelcomeEvent::OpenGitHub => {
                self.leave_welcome(PANEL_AND_TABS, cx);
                self.open_github(window, cx);
            }
        }
        cx.notify();
    }

    /// Starts or shows scripts from the Run panel, showing the terminal.
    fn on_run_event(&mut self, event: &RunEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let RunEvent::Browse(url) = event {
            self.open_browser(Some(url.clone()), window, cx);
            return;
        }
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        match event {
            // A terminal that can't start says why in the dock.
            RunEvent::Start(launches, origin) => {
                let _ = self.start_scripts(launches, *origin, window, cx);
            }
            RunEvent::Show(key) => {
                self.terminal
                    .update(cx, |t, cx| t.show_job(key, window, cx));
            }
            RunEvent::Browse(_) => {}
        }
        cx.notify();
    }

    /// Starts the terminals of a Run panel row (a running one is only
    /// shown), showing the dock if hidden; the last one shows as `origin`
    /// allows. Returns why the first that couldn't start didn't, once the
    /// others have started.
    fn start_scripts(
        &mut self,
        launches: &[Launch],
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let last = launches.len().saturating_sub(1);
        let started: Vec<_> = self.terminal.update(cx, |t, cx| {
            launches
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let show = (i == last).then_some(origin);
                    t.start_job(l.tag.clone(), l.dir.clone(), &l.command, show, window, cx)
                })
                .collect()
        });
        cx.notify();
        started.into_iter().try_for_each(|s| s.map(drop))
    }

    /// Types `commands` into this window's terminal, showing the dock if
    /// hidden; the terminal shows as `origin` allows. Returns the tab and
    /// pane it typed into, or why no shell could start.
    pub fn run_in_terminal(
        &mut self,
        commands: &str,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        self.leave_welcome(TERMINAL_ONLY, cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        let ran = self
            .terminal
            .update(cx, |t, cx| t.run(commands, origin, window, cx));
        cx.notify();
        ran
    }

    fn toggle_area(&mut self, area: Area, cx: &mut Context<Self>) {
        // From the welcome screen, toggles build up from just the tabs.
        self.leave_welcome(CENTER_ONLY, cx);
        self.layout.toggle(area);
        cx.notify();
    }

    /// Fills the window with the terminal, or puts back the layout it
    /// replaced (everything, if that was terminal-only too). For the user
    /// the terminal takes the keyboard; an agent leaves it where it is.
    fn toggle_terminal_maximized(
        &mut self,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.canvas_maximized = false;
        if self.layout == TERMINAL_ONLY {
            self.layout = self.unmaximized.take().unwrap_or_default();
        } else {
            self.unmaximized = Some(self.layout);
            self.layout = TERMINAL_ONLY;
        }
        if origin == Origin::User {
            self.terminal.update(cx, |t, cx| t.focus_active(window, cx));
        }
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
    /// open one, unless `new` or there is none. It shows as `origin` allows.
    /// Returns the session's tab and pane, or why there is none: when the
    /// server's settings are incomplete, the user gets them in a tab.
    fn connect_ssh(
        &mut self,
        key: String,
        new: bool,
        origin: Origin,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(usize, PaneId), String> {
        self.leave_welcome(Layout::default(), cx);
        if !self.layout.terminal {
            self.layout.toggle(Area::Terminal);
        }
        if !new
            && let Some(found) = self
                .terminal
                .update(cx, |t, cx| t.focus_ssh(&key, origin, window, cx))
        {
            cx.notify();
            return Ok(found);
        }
        let opened = match self.ssh_store.update(cx, |s, cx| s.remote(&key, cx)) {
            Ok(remote) => self
                .terminal
                .update(cx, |t, cx| t.open_ssh(remote, origin, window, cx)),
            Err(problem) => {
                if origin == Origin::User
                    && let Some(id) = key.strip_prefix("ssh\u{0}")
                {
                    self.open_ssh_host(id.to_string(), window, cx);
                }
                Err(format!(
                    "{problem} Fix the server's settings (update_ssh_host), then connect again."
                ))
            }
        };
        cx.notify();
        opened
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
                    let _ = this.connect_ssh(key.clone(), false, Origin::User, window, cx);
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
    /// follow it: a file editor's, a canvas file's, whose canvas saves
    /// there from now on ([`BoardView::rebind`], CANVAS-UI-3), an image's, a
    /// PDF's and a video's, and a Find in Folder tab its folder and the files it
    /// found. A project's canvas stays the project's, `.nyawit/canvas.sawit`
    /// by definition. A History tab keeps the path it lists the commits of: git
    /// knows them by that path until the move is committed.
    fn follow_move(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        for tab in &mut self.tabs {
            follow_tab(&mut tab.item, from, to, cx);
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
            let name = folder_name(&dir);
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

    /// Starts the one GitHub sign-in (unless one is under way) and shows the GitHub page, whose
    /// card shows the code to enter: from the title bar, the Welcome screen, or a "Sign in"
    /// beside a GitHub control. Only ever from the user's click: nothing signs in by itself.
    fn sign_in_to_github(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(PANEL_AND_TABS, cx);
        self.github.update(cx, |a, cx| a.sign_in(cx));
        self.open_github(window, cx);
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
    /// project. Its file is `.nyawit/canvas.sawit` (else `canvas.sawit`
    /// in Sawit's own folder).
    fn open_canvas(&mut self, cx: &mut Context<Self>) -> Option<Entity<BoardView>> {
        let root = self.root(cx);
        self.open_canvas_of(root, cx)
    }

    /// Opens the active project's canvas as the user's ⌃⌘C and the title bar's Canvas button do:
    /// with the keyboard ([`BoardView::take_keyboard`], CANVAS-UI-2). An agent's canvas op and
    /// `app_open_page canvas` open the canvas of the agent's own project without
    /// ([`Self::open_canvas_of`]), leaving the keyboard where the user has it.
    fn show_canvas(&mut self, cx: &mut Context<Self>) {
        if let Some(board) = self.open_canvas(cx) {
            board.update(cx, |board, cx| board.take_keyboard(cx));
        }
    }

    /// Opens the canvas of the project at `root`, or the app-wide one for `None`, as
    /// [`Self::open_canvas`] opens the active project's. A canvas whose file is open in a canvas
    /// file's tab already (opened before its project was in this window) is that tab: two
    /// canvases of one file would each save over the other's edits.
    fn open_canvas_of(
        &mut self,
        root: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Option<Entity<BoardView>> {
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
            // With its project: another project's canvas file opened by its path, say.
            self.activate_tab(ix, cx);
            // The canvas is the tab's own view.
            let PaneItem::Other { view, .. } = &self.tabs[ix].item else {
                return None;
            };
            return view.clone().downcast::<BoardView>().ok();
        }
        let file = board_file(root.as_deref(), &AppPaths::default_macos().root);
        if let Some((ix, board)) = self.board_tab(&file, cx) {
            self.activate_tab(ix, cx);
            return Some(board);
        }
        let view = cx.new(|cx| BoardView::open(file.clone(), cx));
        cx.subscribe(&view, |this, _, _: &ToggleCanvasMaximize, cx| {
            this.toggle_canvas_maximized(cx)
        })
        .detach();
        self.boards.retain(|b| b.upgrade().is_some());
        self.boards.push(view.downgrade());
        self.push_tab(
            PaneItem::Other {
                view: view.clone().into(),
                key,
                title: crate::board::TITLE.into(),
                icon: crate::board::ICON,
                // The file it shows, for `sawit --wait` on it (CANVAS-UI-7) and app_state.
                file: Some(file),
            },
            root,
            cx,
        );
        cx.notify();
        Some(view)
    }

    /// A canvas's tab is showing in the center pane: a project's canvas, or a canvas file's.
    fn canvas_active(&self) -> bool {
        matches!(
            self.tabs.get(self.active).map(|t| &t.item),
            Some(PaneItem::Other { view, .. }) if view.clone().downcast::<BoardView>().is_ok()
        )
    }

    /// Fills the window with the canvas (hiding the tab bar too), or puts
    /// back the layout it replaced, like the terminal's maximize.
    fn toggle_canvas_maximized(&mut self, cx: &mut Context<Self>) {
        toggle_canvas_fill(
            &mut self.canvas_maximized,
            &mut self.layout,
            &mut self.unmaximized,
        );
        cx.notify();
    }

    /// Ends "fill window" once something else changed what is on screen (⌘B, ⌘J, another tab,
    /// the welcome screen): the canvas fills the window only while its tab is the one showing, on
    /// its own. Each render checks, because the layout and the active tab change in many places.
    fn sync_canvas_maximized(&mut self) {
        let shown = self.welcome.is_none() && self.layout.center && self.canvas_active();
        settle_canvas_fill(
            &mut self.canvas_maximized,
            &mut self.unmaximized,
            self.layout,
            shown,
        );
    }

    /// Opens the active project's browser (one tab per project, in its group, as its canvas;
    /// one for no project), at `url` when given; an open browser becomes the active tab, and
    /// goes to `url` when one is given. Each project's browser keeps its own cookies, and
    /// writes its screenshots and exports into that project.
    pub(crate) fn open_browser(
        &mut self,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        let root = self.root(cx);
        self.open_browser_of(root, url, window, cx)
    }

    /// Opens the browser of the project at `root` (`None`: the one for no project), as
    /// [`Self::open_browser`] opens the active project's, with its project, as a click on its
    /// tab shows it: an agent's browser is the browser of the project it works in, which may not
    /// be the one shown.
    pub(super) fn open_browser_of(
        &mut self,
        root: Option<PathBuf>,
        url: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<BrowserView> {
        self.leave_welcome(CENTER_ONLY, cx);
        if !self.layout.center {
            self.layout.toggle(Area::Center);
        }
        let browser = match self.browser_tab(root.as_deref()) {
            Some((ix, browser)) => {
                self.activate_tab(ix, cx);
                browser.update(cx, |b, cx| b.show(url.as_deref(), cx));
                browser
            }
            None => {
                let view = cx.new(|cx| BrowserView::new(root.clone(), url, window, cx));
                self.browsers.retain(|b| b.upgrade().is_some());
                self.browsers.push(view.downgrade());
                cx.subscribe_in(&view, window, |this, browser, event, window, cx| {
                    let crate::browser::BrowserEvent::SendToTerminal(md) = event;
                    this.send_to_terminal(browser, md, window, cx);
                })
                .detach();
                self.push_tab(
                    PaneItem::Other {
                        view: view.clone().into(),
                        key: browser_key(root.as_deref()),
                        title: crate::browser::TITLE.into(),
                        icon: crate::browser::ICON,
                        file: None,
                    },
                    root,
                    cx,
                );
                view
            }
        };
        cx.notify();
        browser
    }

    /// The browser tab of the project at `root` (`None`: the one for no project), and its view.
    pub(super) fn browser_tab(&self, root: Option<&Path>) -> Option<(usize, Entity<BrowserView>)> {
        let key = browser_key(root);
        self.tabs
            .iter()
            .enumerate()
            .find_map(|(ix, t)| match &t.item {
                PaneItem::Other { key: k, view, .. } if *k == key => {
                    view.clone().downcast::<BrowserView>().ok().map(|b| (ix, b))
                }
                _ => None,
            })
    }

    /// The browser whose tab is the active one.
    pub(super) fn active_browser(&self) -> Option<Entity<BrowserView>> {
        match self.tabs.get(self.active).map(|t| &t.item) {
            Some(PaneItem::Other { view, .. }) => view.clone().downcast::<BrowserView>().ok(),
            _ => None,
        }
    }

    /// Pastes annotations into the terminal running an agent, the browser's project's first;
    /// copies them when no tab has bracketed paste on.
    fn send_to_terminal(
        &mut self,
        browser: &Entity<BrowserView>,
        markdown: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let project = browser.read(cx).project().map(Path::to_path_buf);
        let target = self.terminal.read(cx).paste_target(project.as_deref(), cx);
        let sent = target.and_then(|ix| {
            if !self.layout.terminal {
                self.layout.toggle(Area::Terminal);
            }
            match self
                .terminal
                .update(cx, |t, cx| t.paste(ix, markdown, window, cx))
            {
                Ok(()) => Some(ix),
                Err(e) => {
                    eprintln!("annotate: paste failed: {e}");
                    None
                }
            }
        });
        let note = match sent {
            Some(ix) => format!("Sent to Terminal {}", ix + 1),
            None => {
                cx.write_to_clipboard(ClipboardItem::new_string(markdown.to_string()));
                "No terminal is ready for a paste, so it was copied.".to_string()
            }
        };
        browser.update(cx, |b, cx| b.set_annotate_note(&note, cx));
        cx.notify();
    }

    /// Tells each canvas whether its tab is on screen ([`BoardView::set_shown`]), as
    /// [`Self::sync_browser`] tells the browser: the active tab, while the tab area shows. Each
    /// render checks, because the layout and the active tab change in many places. A canvas taken
    /// off screen ends what it held of the pointer, which nothing would hear the release of, and
    /// gives the keyboard, if it had it, to the workspace, which a canvas shown again takes it
    /// back from. Those going off screen are told first, so that the one coming on screen finds
    /// the keyboard where they left it, whatever their order.
    fn sync_boards(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.welcome.is_none() && self.layout.center;
        let home = self.focus.clone();
        let mut boards: Vec<(Entity<BoardView>, bool)> = (0..self.tabs.len())
            .filter_map(|ix| Some((self.board_at(ix)?, center && ix == self.active)))
            .collect();
        boards.sort_by_key(|(_, shown)| *shown);
        for (board, shown) in boards {
            board.update(cx, |board, cx| board.set_shown(shown, &home, window, cx));
        }
    }

    /// Keeps the browsers' native webviews in step with what is on screen:
    /// they draw above GPUI, so each is hidden unless its tab is showing.
    fn sync_browser(&mut self, cx: &mut Context<Self>) {
        self.browsers.retain(|b| b.upgrade().is_some());
        let shown = (self.welcome.is_none() && self.layout.center)
            .then(|| self.active_browser())
            .flatten();
        let browsers: Vec<Entity<BrowserView>> =
            self.browsers.iter().filter_map(|b| b.upgrade()).collect();
        // Hidden first: a webview that hides hands the keyboard back.
        for browser in browsers.iter().filter(|b| shown.as_ref() != Some(*b)) {
            browser.update(cx, |b, _| b.set_visible(false));
        }
        if let Some(browser) = shown {
            browser.update(cx, |b, _| b.set_visible(true));
        }
    }

    /// What the workspace shows now ([`Shown`]).
    fn shown(&self) -> Shown {
        (
            self.welcome.is_some(),
            self.layout,
            self.canvas_maximized,
            self.project,
            self.sidebar,
            self.tab_view(self.active),
        )
    }

    /// Shows the MCP install page (one tab), with the tab area, which may be hidden, and the
    /// keyboard in its search.
    fn open_mcp_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_mcp_page(true, window, cx);
    }

    /// Shows the MCP page; `focus` gives its search the keyboard (not when an
    /// agent opens it: the keyboard stays where the user has it).
    fn show_mcp_page(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "mcp".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<McpPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| McpPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
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

    /// Shows the keymap editor (one tab), with the tab area, which may be hidden, and the
    /// keyboard.
    fn open_keymap_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_keymap_editor(true, window, cx);
    }

    /// Shows the keymap editor; `focus` gives it the keyboard. Not when an agent opens it: the
    /// keyboard stays where the user has it, as for the Dev Tools and MCP pages. There, a ⌫
    /// meant for the terminal would remove the selected binding from keymap.json.
    fn show_keymap_editor(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "keymap".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(editor) = view.clone().downcast::<KeymapEditor>()
            {
                window.focus(&editor.focus_handle(cx), cx);
            }
        } else {
            let view = cx.new(|cx| KeymapEditor::new(window, cx));
            if focus {
                window.focus(&view.focus_handle(cx), cx);
            }
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Keymap Editor".into(),
                    icon: Lucide::Keyboard,
                    file: None,
                },
                None,
                cx,
            );
        }
        cx.notify();
    }

    /// Opens the keymap file in an editor tab, writing a new one with a few lines on how to fill
    /// it if there is none.
    fn open_keymap_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let store = crate::keymap::store(cx);
        let path = store.read(cx).path().to_path_buf();
        if let Err(error) = store.read(cx).ensure_file() {
            log::warn!("sawit: can't create {}: {error}", path.display());
            return;
        }
        self.show_center(cx);
        self.open_file(path, window, cx);
    }

    /// Shows the Dev Tools install page (one tab), with the tab area, which may be hidden, and
    /// the keyboard in its search.
    fn open_tools_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_tools_page(true, window, cx);
    }

    /// Shows the Dev Tools page; `focus` gives its search the keyboard (not
    /// when an agent opens it: the keyboard stays where the user has it).
    fn show_tools_page(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "tools".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            // The keyboard goes to its search, as for a new one.
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<ToolsPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| ToolsPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
            cx.subscribe_in(
                &view,
                window,
                |this, _, e: &ToolsEvent, window, cx| match e {
                    ToolsEvent::RunInTerminal(command) => {
                        let _ = this.run_in_terminal(command, Origin::User, window, cx);
                    }
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

    /// Shows the Skills page (one tab), with the tab area, which may be hidden, and the keyboard
    /// in its search.
    fn open_skills_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show_skills_page(true, window, cx);
    }

    /// Shows the Skills page; `focus` gives its search the keyboard (not when an agent opens it:
    /// the keyboard stays where the user has it).
    fn show_skills_page(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.leave_welcome(CENTER_ONLY, cx);
        self.show_center(cx);
        let key: SharedString = "skills".into();
        if let Some(ix) = self
            .tabs
            .iter()
            .position(|t| matches!(&t.item, PaneItem::Other { key: k, .. } if *k == key))
        {
            self.active = ix;
            if focus
                && let PaneItem::Other { view, .. } = &self.tabs[ix].item
                && let Ok(page) = view.clone().downcast::<SkillsPage>()
            {
                page.update(cx, |page, cx| page.focus_search(window, cx));
            }
        } else {
            let view = cx.new(|cx| SkillsPage::new(window, cx));
            if focus {
                view.update(cx, |page, cx| page.focus_search(window, cx));
            }
            self.push_tab(
                PaneItem::Other {
                    view: view.into(),
                    key,
                    title: "Skills".into(),
                    icon: Lucide::Sparkles,
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
                    .role(Role::Tab)
                    .aria_label(project_name(&root))
                    .aria_selected(active)
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
                            .accessibility_label(format!("Close {}", project_name(&root)))
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
            .role(Role::TabList)
            .aria_label("Projects")
            .min_w_0()
            .gap_1()
            .overflow_x_scroll()
            .children(chips)
            .child(
                Button::new("add-project")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Plus)
                    .accessibility_label("Open Another Project")
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

    /// The title bar's right side: an update waiting, the pages a click
    /// opens, then the layout toggles, each set in its own group.
    fn render_layout_toggles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let l = self.layout;
        let group = || {
            h_flex()
                .flex_none()
                .p(px(2.))
                .gap(px(2.))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
        };
        let toggle = |id: &'static str,
                      icon: Lucide,
                      tooltip: &'static str,
                      action: &dyn Action,
                      on: bool| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .accessibility_label(tooltip)
                .tooltip_with_action(tooltip, action, Some("Workspace"))
                .selected(on)
        };
        let pending = self.updater.read(cx).pending().map(|r| r.version);
        h_flex()
            .flex_none()
            .gap_2()
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
                    .on_click(|_, _, cx| crate::update::restart(cx))
            }))
            .child(
                group()
                    .child(
                        Button::new("open-canvas")
                            .ghost()
                            .xsmall()
                            .icon(crate::board::ICON)
                            .label("Canvas")
                            .tooltip_with_action(
                                "Brainstorm on a Whiteboard",
                                &OpenCanvas,
                                Some("Workspace"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.show_canvas(cx))),
                    )
                    .child(
                        Button::new("open-browser")
                            .ghost()
                            .xsmall()
                            .icon(crate::browser::ICON)
                            .label("Browser")
                            .tooltip("Preview Your App in a Browser an Agent Can Drive")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_browser(None, window, cx);
                            })),
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
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_tools_page(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("open-mcp")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Plug)
                            .label("MCP")
                            .tooltip_with_action(
                                "Install MCP Servers into Claude, Codex and More",
                                &OpenMcp,
                                Some("Workspace"),
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_mcp_page(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("open-skills")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Sparkles)
                            .label("Skills")
                            .tooltip(
                                "Install Agent Skills: Superpowers, Cloudflare, MCP Apps and More",
                            )
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_skills_page(window, cx)
                                }),
                            ),
                    )
                    .child(
                        Button::new("open-keymap")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Keyboard)
                            .accessibility_label("Keymap Editor")
                            .tooltip_with_action(
                                "Keymap Editor",
                                &OpenKeymapEditor,
                                Some("Workspace"),
                            )
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_keymap_editor(window, cx)
                            })),
                    ),
            )
            .child(
                group()
                    .child(
                        toggle(
                            "toggle-sidebar",
                            Lucide::PanelLeft,
                            "Sidebar",
                            &ToggleSidebar,
                            l.sidebar,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_area(Area::Sidebar, cx)),
                        ),
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
                            "Terminal",
                            &ToggleTerminal,
                            l.terminal,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_area(Area::Terminal, cx)),
                        ),
                    ),
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
        A11Y_ACTIVE.set(window.is_a11y_active() && needs_whole_tree());
        if window.focused(cx).is_none() {
            // Keep workspace shortcuts working when nothing inside has focus.
            self.focus.focus(window, cx);
        }
        self.sync_canvas_maximized();
        self.sync_boards(window, cx);
        self.sync_browser(cx);
        // After the canvases: one shown again takes its keyboard back itself, later in the frame.
        let pages: Vec<FocusHandle> = self
            .browsers
            .iter()
            .filter_map(|b| b.upgrade())
            .map(|b| b.read(cx).focus_handle(cx))
            .collect();
        let shown = self.shown();
        self.keyboard.give_back(
            shown,
            |back, cx| !pages.contains(back) && !crate::keymap::is_recorder(back, cx),
            window,
            cx,
        );
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
            .on_action(cx.listener(|this, _: &ShowConfigFiles, _, cx| {
                this.show_panel(SidebarTab::ShellConfig, cx)
            }))
            .on_action(
                cx.listener(|this, _: &ShowPrompts, _, cx| {
                    this.show_panel(SidebarTab::Prompts, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenCanvas, _, cx| this.show_canvas(cx)))
            .on_action(cx.listener(|this, _: &OpenDevTools, w, cx| this.open_tools_page(w, cx)))
            .on_action(cx.listener(|this, _: &OpenMcp, w, cx| this.open_mcp_page(w, cx)))
            .on_action(
                cx.listener(|this, _: &OpenLocalExplorer, w, cx| this.open_local_explorer(w, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenKeymapEditor, w, cx| {
                this.show_center(cx);
                this.open_keymap_editor(w, cx)
            }))
            .on_action(cx.listener(|this, _: &OpenKeymapFile, w, cx| this.open_keymap_file(w, cx)))
            .on_action(cx.listener(|this, _: &OpenChangelog, _, cx| {
                this.show_center(cx);
                this.open_changelog_page(cx)
            }))
            // Every "Sign in" in the window runs the one sign-in, whose code the GitHub page
            // shows.
            .on_action(cx.listener(|this, _: &SignInToGitHub, w, cx| this.sign_in_to_github(w, cx)))
            .on_action(cx.listener(|this, _: &OpenGitHub, w, cx| {
                this.leave_welcome(PANEL_AND_TABS, cx);
                this.open_github(w, cx)
            }))
            .on_action(cx.listener(|this, _: &InstallUpdate, _, cx| {
                // Installs a pending update, else checks for one and shows
                // the changelog page, where the result appears.
                if this.updater.read(cx).pending().is_some() {
                    crate::update::restart(cx);
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
                            .child(self.render_layout_toggles(cx))
                            .child(crate::git::account::account_control(&self.github, cx)),
                    ),
            )
            .child(div().flex_1().min_h_0().child(self.render_body(cx)))
            .child(self.render_status_bar(cx))
    }
}

thread_local! {
    /// Whether the window being drawn has assistive technology attached that walks the whole
    /// accessibility tree (VoiceOver, Switch Control, or an end-to-end run's driver), as the
    /// workspace's render saw it: see [`cached`].
    static A11Y_ACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The last answer of [`screen_reader_on`] and when it was read, so a render asks macOS at
    /// most once every [`SCREEN_READER_RECHECK`].
    static SCREEN_READER: std::cell::Cell<Option<(std::time::Instant, bool)>> =
        const { std::cell::Cell::new(None) };
}

/// How long a read of whether VoiceOver or Switch Control is on stays good.
const SCREEN_READER_RECHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// Whether the panes must render uncached for the accessibility tree to stay whole. Any app
/// that reads the window's accessibility (a text expander, a grammar checker, a clipboard or
/// launcher utility reading the focused field) turns GPUI's accessibility on, and stays on, but
/// only a screen reader, Switch Control or an end-to-end run walks the whole tree. Rendering
/// uncached for the rest would rebuild every pane on each line of terminal output.
fn needs_whole_tree() -> bool {
    static E2E_RUN: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("SAWIT_E2E_RUN").is_some());
    if *E2E_RUN {
        return true;
    }
    SCREEN_READER.with(|cell| match cell.get() {
        Some((at, on)) if at.elapsed() < SCREEN_READER_RECHECK => on,
        _ => {
            let on = screen_reader_on();
            cell.set(Some((std::time::Instant::now(), on)));
            on
        }
    })
}

/// Whether VoiceOver or Switch Control is on.
#[cfg(target_os = "macos")]
fn screen_reader_on() -> bool {
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    workspace.isVoiceOverEnabled() || workspace.isSwitchControlEnabled()
}

/// Elsewhere, any attached assistive technology counts: there is no cheap way to tell a screen
/// reader from other clients.
#[cfg(not(target_os = "macos"))]
fn screen_reader_on() -> bool {
    true
}

/// Mounts a pane so it re-renders only when it notifies, not every time the
/// workspace does (terminal output, a cursor blink or a poll elsewhere
/// would otherwise rebuild the whole window).
///
/// Not while a screen reader, Switch Control or an end-to-end run reads the accessibility tree
/// ([`needs_whole_tree`]): GPUI leaves a cached view's accessibility nodes out of every frame
/// that reuses the view's last render, so the panes' buttons, fields and lists would vanish from
/// the accessibility tree as soon as they stopped changing.
fn cached(view: AnyView) -> AnyElement {
    if A11Y_ACTIVE.get() {
        return view.into_any_element();
    }
    view.cached(StyleRefinement::default().size_full())
        .into_any_element()
}

/// [`Workspace::follow_move`] for the tab showing `item`.
fn follow_tab(item: &mut PaneItem, from: &Path, to: &Path, cx: &mut App) {
    match item {
        PaneItem::File(editor) => editor.update(cx, |e, cx| {
            if let Some(path) = moved_to(&e.path, from, to) {
                e.path = path;
                cx.notify();
            }
        }),
        PaneItem::Other {
            view,
            key,
            title,
            file,
            ..
        } if key.starts_with("file\u{0}") => {
            let board = view.clone().downcast::<BoardView>().ok();
            let shown = match &board {
                Some(board) => board.read(cx).path().to_path_buf(),
                None => file.clone().unwrap_or_default(),
            };
            let Some(path) = moved_to(&shown, from, to) else {
                return;
            };
            if let Some(board) = board {
                board.update(cx, |board, cx| board.rebind(path.clone(), cx));
            } else if let Ok(image) = view.clone().downcast::<ImageView>() {
                image.update(cx, |image, cx| image.follow(path.clone(), cx));
            } else if let Ok(pdf) = view.clone().downcast::<PdfView>() {
                pdf.update(cx, |pdf, cx| {
                    pdf.path = path.clone();
                    cx.notify();
                });
            } else if let Ok(video) = view.clone().downcast::<VideoView>() {
                // Its player has the file open, wherever it goes.
                video.update(cx, |video, cx| {
                    video.path = path.clone();
                    cx.notify();
                });
            }
            *key = format!("file\u{0}{}", path.display()).into();
            *title = (path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
                .into();
            *file = Some(path);
        }
        PaneItem::Other {
            view, key, title, ..
        } if key.starts_with("search\u{0}") => {
            let Ok(search) = view.clone().downcast::<FolderSearch>() else {
                return;
            };
            if let Some(dir) = search.update(cx, |search, cx| search.follow_move(from, to, cx)) {
                *key = FolderSearch::key(&dir).into();
                *title = format!("Search: {}", folder_name(&dir)).into();
            }
        }
        PaneItem::Other { .. } | PaneItem::Server => {}
    }
}

/// The name a folder's tabs show: its own, or its path for `/`.
fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

/// Where `path` is once `from` has been renamed or moved to `to`: `to` itself, or for a path in a
/// moved folder its place in `to`. `None` when the move left it where it was.
fn moved_to(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

/// The project that `file` is the canvas file of (spec D5), among `roots`, the folders of the
/// window's projects: `Some` of its folder as `roots` has it, which is `None` for a project with no
/// folder (its canvas is the app's own, in `app_root`). `None` when it is no project's. Named
/// through a link or as written, it is one file ([`same_canvas_file`]).
fn canvas_project(
    file: &Path,
    roots: impl IntoIterator<Item = Option<PathBuf>>,
    app_root: &Path,
) -> Option<Option<PathBuf>> {
    roots
        .into_iter()
        .find(|root| same_canvas_file(&board_file(root.as_deref(), app_root), file))
}

/// Whether `a` and `b` name one canvas file: as written, or through the links on their way, the
/// file there yet or not (CANVAS-UI-4). An agent's `app_open` passes its path as given, where the
/// CLI's paths and the projects' folders are resolved: `/tmp/p/x.sawit` is
/// `/private/tmp/p/x.sawit` on macOS. Two canvases of one file would each save over the
/// other's edits.
fn same_canvas_file(a: &Path, b: &Path) -> bool {
    use sawit_core::files::resolve;
    a == b || resolve(a) == resolve(b)
}

/// The key of the Browser tab of the project at `root` (`None`: the one for no project).
fn browser_key(root: Option<&Path>) -> SharedString {
    match root {
        Some(root) => format!("browser\u{0}{}", root.display()).into(),
        None => "browser".into(),
    }
}

/// What the Dev Tools page in `item` runs and has queued, when it is that page
/// and it runs something.
fn tools_work(item: &PaneItem, cx: &App) -> Option<String> {
    let PaneItem::Other { view, key, .. } = item else {
        return None;
    };
    if key.as_ref() != "tools" {
        return None;
    }
    view.clone().downcast::<ToolsPage>().ok()?.read(cx).work()
}

/// What the Skills page in `item` runs and has queued, when it is that page and it runs
/// something.
fn skills_work(item: &PaneItem, cx: &App) -> Option<String> {
    let PaneItem::Other { view, key, .. } = item else {
        return None;
    };
    if key.as_ref() != "skills" {
        return None;
    }
    view.clone().downcast::<SkillsPage>().ok()?.read(cx).work()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};

    use gpui_kit::Modifiers;

    use super::{
        CENTER_ONLY, bindings, browser_key, canvas_project, moved_to, settle_canvas_fill,
        toggle_canvas_fill,
    };
    use crate::layout::{Area, Layout};

    /// Each project has its own Browser tab, as it has its own canvas: its cookies, its
    /// annotation screenshots and its design exports are the project's ("a browser tab per
    /// project"). A window used to keep one, tied to the project it was opened in.
    #[test]
    fn each_project_has_its_own_browser() {
        let (a, b) = (Path::new("/work/a"), Path::new("/work/b"));
        assert_ne!(browser_key(Some(a)), browser_key(Some(b)));
        assert_ne!(browser_key(Some(a)), browser_key(None));
        assert_eq!(
            browser_key(Some(a)),
            browser_key(Some(&PathBuf::from("/work/a")))
        );
        // `app_state` names a tab's kind by its key's first part.
        assert_eq!(browser_key(Some(a)).split('\u{0}').next(), Some("browser"));
        assert_eq!(browser_key(None).as_ref(), "browser");
    }

    /// A keystroke written as `ctrl-alt-shift-cmd-key`.
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

    /// The Dev Tools tab counts as unsaved while it installs: closing it alone asks first
    /// (closing stops the installer), and closing others or all leaves it open.
    #[gpui_kit::test]
    fn a_dev_tools_tab_that_installs_counts_as_unsaved(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::AppContext as _;
        let busy = super::PaneItem::Other {
            view: cx
                .new(|cx| crate::tools_page::ToolsPage::installing("bun", cx))
                .into(),
            key: "tools".into(),
            title: "Dev Tools".into(),
            icon: gpui_kit::assets::IconName::Package,
            file: None,
        };
        cx.update(|cx| {
            assert_eq!(
                super::tools_work(&busy, cx).as_deref(),
                Some("Installing Bun")
            );
            assert_eq!(super::tools_work(&super::PaneItem::Server, cx), None);
        });
    }

    #[test]
    fn a_projects_canvas_file_is_that_projects_canvas() {
        let app = Path::new("/support/sawit");
        let (a, b) = (PathBuf::from("/work/a"), PathBuf::from("/work/b"));
        let roots = vec![Some(a.clone()), Some(b.clone()), None];
        let project = |file: &str| canvas_project(Path::new(file), roots.clone(), app);
        // Each project's canvas file, and for the project with no folder the app's own.
        assert_eq!(project("/work/b/.nyawit/canvas.sawit"), Some(Some(b)));
        assert_eq!(project("/work/a/.nyawit/canvas.sawit"), Some(Some(a)));
        assert_eq!(project("/support/sawit/canvas.sawit"), Some(None));
        // Any other file is one of its own: another file in `.nyawit`, a canvas file elsewhere,
        // the canvas file of a project the window doesn't have.
        assert_eq!(project("/work/a/.nyawit/sketch.sawit"), None);
        assert_eq!(project("/work/a/canvas.sawit"), None);
        assert_eq!(project("/work/c/.nyawit/canvas.sawit"), None);
        // With every project in a folder, the app's canvas file is one of its own too.
        let app_canvas = Path::new("/support/sawit/canvas.sawit");
        let folders = [Some(PathBuf::from("/work/a"))];
        assert_eq!(canvas_project(app_canvas, folders, app), None);
    }

    /// A renamed or moved file, and the files of a moved folder, are where the move put them:
    /// the tabs of file editors and of canvas files follow them there (CANVAS-UI-3).
    #[test]
    fn a_moved_path_is_found_where_the_move_put_it() {
        let (from, to) = (Path::new("/p/sub"), Path::new("/p/moved"));
        let moved = |path: &str| moved_to(Path::new(path), from, to);
        assert_eq!(moved("/p/sub"), Some(PathBuf::from("/p/moved")));
        assert_eq!(
            moved("/p/sub/a.sawit"),
            Some(PathBuf::from("/p/moved/a.sawit"))
        );
        assert_eq!(
            moved("/p/sub/deeper/b.sawit"),
            Some(PathBuf::from("/p/moved/deeper/b.sawit"))
        );
        // Beside it, or with a name that only starts the same: where it was.
        assert_eq!(moved("/p/other.sawit"), None);
        assert_eq!(moved("/p/sub2/a.sawit"), None);
        assert_eq!(moved("/p"), None);
    }

    /// CANVAS-UI-4: a project's canvas file named through a link (an agent's `app_open` of
    /// `/tmp/p/.nyawit/canvas.sawit` for the project at `/private/tmp/p`, `/tmp` being a link
    /// on macOS) is that project's canvas still, whether `.nyawit/` is there yet or not: one
    /// canvas per file. Compared as written, it was a file of its own, and a second canvas of it
    /// opened in a tab, whose saves the first one's refused.
    #[test]
    fn a_projects_canvas_file_named_through_a_link_is_that_projects_canvas() {
        let scratch = std::env::temp_dir().join(format!(
            "sawit-workspace-{}-canvas-link",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&scratch);
        let project = scratch.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let link = scratch.join("link");
        std::os::unix::fs::symlink(&project, &link).unwrap();
        let app = Path::new("/support/sawit");
        let roots = || vec![Some(project.clone()), None];
        let aliased = link.join(".nyawit").join("canvas.sawit");
        assert_eq!(
            canvas_project(&aliased, roots(), app),
            Some(Some(project.clone()))
        );
        std::fs::create_dir_all(project.join(".nyawit")).unwrap();
        std::fs::write(project.join(".nyawit/canvas.sawit"), b"").unwrap();
        assert_eq!(
            canvas_project(&aliased, roots(), app),
            Some(Some(project.clone()))
        );
        // Another file through the link is still one of its own.
        let other = link.join(".nyawit").join("sketch.sawit");
        assert_eq!(canvas_project(&other, roots(), app), None);
        let _ = std::fs::remove_dir_all(&scratch);
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

    /// ⇧⌘↩ fills the window with the canvas and a second ⇧⌘↩ puts the layout back; anything else
    /// that changes what the window shows ends it too, and the tab bar comes back (ruling P8-C3).
    /// The tab bar shows unless the canvas fills the window (`render_center`), and each render
    /// settles the fill against what is on screen (`sync_canvas_maximized`).
    #[test]
    fn maximize_still_works_without_webview() {
        let before = Layout::default();
        let (mut maximized, mut layout, mut unmaximized) = (false, before, None);
        toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
        assert!(maximized, "the tab bar is hidden");
        assert_eq!(layout, CENTER_ONLY);
        // While the canvas stays the tab on screen, renders leave it filling the window.
        settle_canvas_fill(&mut maximized, &mut unmaximized, layout, true);
        assert!(maximized);
        toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
        assert!(!maximized, "the tab bar is back");
        assert_eq!(layout, before);

        // ⌘B shows the sidebar: the next render ends the fill, and the sidebar stays.
        toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
        layout.toggle(Area::Sidebar);
        settle_canvas_fill(&mut maximized, &mut unmaximized, layout, true);
        assert!(!maximized, "the tab bar is back");
        assert_eq!(unmaximized, None);
        assert!(layout.sidebar && layout.center && !layout.terminal);

        // Another tab is activated: the canvas is no longer on screen, which ends the fill too.
        let (mut maximized, mut layout, mut unmaximized) = (false, before, None);
        toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
        settle_canvas_fill(&mut maximized, &mut unmaximized, layout, false);
        assert!(!maximized, "the tab bar is back");
        assert_eq!(unmaximized, None);
        // ⇧⌘↩ on the canvas again fills the window from the layout as it is now.
        toggle_canvas_fill(&mut maximized, &mut layout, &mut unmaximized);
        assert!(maximized);
        assert_eq!(unmaximized, Some(CENTER_ONLY));
    }

    /// A stand-in for the workspace in a window as `cli::open_window` makes one (under gpui-kit's
    /// `Root`): the workspace's keys, key context and keyboard, `render`'s refocus when nothing
    /// has the keyboard, and its [`super::KeyboardHome`], given back to as `render` does. It shows
    /// a terminal pane while the dock shows (⌘J), which records what is typed into it, and a
    /// file's text while its tab is in front (⌃Tab goes round three tabs), drawn with a page,
    /// which, as the browser's, never has the keyboard given back.
    mod keyboard {
        use super::super::{KeyboardHome, NextTab, ToggleTerminal};
        use gpui_kit::component::input::{Input, InputState};
        use gpui_kit::prelude::*;
        use gpui_kit::{
            AnyWindowHandle, App, Entity, FocusHandle, KeyDownEvent, TestAppContext, Window, div,
        };

        pub(super) struct StandIn {
            pub focus: FocusHandle,
            /// A terminal pane's keyboard, drawn while the dock shows (⌘J).
            pub pane: FocusHandle,
            pub dock: bool,
            /// A file editor's text, drawn while its tab is the one in front (⌃Tab).
            pub text: Entity<InputState>,
            /// A web page's keyboard, drawn with the text.
            pub page: FocusHandle,
            pub tab: usize,
            pub presses: usize,
            /// What reached the terminal pane.
            pub typed: Vec<String>,
            renders: usize,
            keyboard: KeyboardHome<(bool, usize)>,
        }

        impl Render for StandIn {
            fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                // A give-back that asked for frames without end would hang the test instead.
                self.renders += 1;
                assert!(self.renders < 500, "the stand-in redraws without end");
                if window.focused(cx).is_none() {
                    self.focus.focus(window, cx);
                }
                let page = self.page.clone();
                self.keyboard
                    .give_back((self.dock, self.tab), |back, _| *back != page, window, cx);
                div()
                    .key_context("Workspace")
                    .track_focus(&self.focus)
                    .size_full()
                    .on_action(cx.listener(|this, _: &ToggleTerminal, _, cx| {
                        this.dock = !this.dock;
                        this.presses += 1;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                        this.tab = (this.tab + 1) % 3;
                        this.presses += 1;
                        cx.notify();
                    }))
                    .when(self.tab == 0, |el| {
                        el.child(Input::new(&self.text))
                            .child(div().track_focus(&self.page))
                    })
                    .when(self.dock, |el| {
                        el.child(
                            div()
                                .key_context("Terminal")
                                .track_focus(&self.pane)
                                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                                    if !e.keystroke.modifiers.modified() {
                                        this.typed.push(e.keystroke.key.clone());
                                        cx.stop_propagation();
                                    }
                                })),
                        )
                    })
            }
        }

        /// What has the keyboard to start with.
        pub(super) type Keyboard = fn(&StandIn, &App) -> FocusHandle;

        /// The stand-in, with the keyboard given to what `keyboard` picks.
        pub(super) fn open(
            keyboard: Keyboard,
            cx: &mut TestAppContext,
        ) -> (AnyWindowHandle, Entity<StandIn>) {
            let (window, workspace) = cx.update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |window, cx| {
                    let text = cx.new(|cx| InputState::new(window, cx));
                    cx.new(|cx| {
                        let focus = cx.focus_handle();
                        StandIn {
                            keyboard: KeyboardHome::new(focus.clone(), window, cx),
                            focus,
                            pane: cx.focus_handle(),
                            dock: true,
                            text,
                            page: cx.focus_handle(),
                            tab: 0,
                            presses: 0,
                            typed: Vec::new(),
                            renders: 0,
                        }
                    })
                })
                .unwrap()
            });
            let to = cx.update(|cx| keyboard(workspace.read(cx), cx));
            cx.update_window(window, |_, window, cx| {
                window.activate_window();
                window.focus(&to, cx);
            })
            .unwrap();
            cx.run_until_parked();
            let placed = cx
                .update_window(window, |_, window, _| to.is_focused(window))
                .unwrap();
            assert!(placed, "the keyboard isn't where the test put it");
            (window, workspace)
        }

        /// Changes what the stand-in shows as a click does: with no key.
        pub(super) fn show(
            workspace: &Entity<StandIn>,
            cx: &mut TestAppContext,
            change: impl FnOnce(&mut StandIn),
        ) {
            workspace.update(cx, |workspace, cx| {
                change(workspace);
                cx.notify();
            });
            cx.run_until_parked();
        }

        /// Whether `handle`, picked as `keyboard` picks, has the keyboard.
        pub(super) fn has(
            keyboard: Keyboard,
            window: AnyWindowHandle,
            workspace: &Entity<StandIn>,
            cx: &mut TestAppContext,
        ) -> bool {
            cx.update_window(window, |_, window, cx| {
                keyboard(workspace.read(cx), cx).is_focused(window)
            })
            .unwrap()
        }
    }

    /// What had the keyboard and goes off screen gives it to the workspace
    /// ([`super::KeyboardHome`]), whatever it is: a terminal pane once ⌘J hides the dock, a file
    /// editor's text once ⌃Tab shows another tab. Before, it kept the keyboard unseen, GPUI sent
    /// the keys from the window's root, and the second ⌘J or ⌃Tab did nothing. Each of three
    /// presses works, and the keyboard ends where it belongs: with the workspace once the dock is
    /// hidden, with the text once ⌃Tab has gone round to its tab again. (The browser's page and
    /// address field: `browser::tests`.)
    #[gpui_kit::test]
    fn what_goes_off_screen_gives_the_keyboard_to_the_workspace(cx: &mut gpui_kit::TestAppContext) {
        use self::keyboard::{Keyboard, has, open};
        use gpui_kit::Focusable as _;

        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let mut failed = Vec::new();
        let home: Keyboard = |this, _| this.focus.clone();
        let pane: Keyboard = |this, _| this.pane.clone();
        let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
        for (what, keyboard, keys, ends) in [
            ("a terminal pane", pane, "cmd-j", ("the workspace", home)),
            ("a file's text", text, "ctrl-tab", ("the text", text)),
        ] {
            let (window, workspace) = open(keyboard, cx);
            for _ in 0..3 {
                cx.simulate_keystrokes(window, keys);
                cx.run_until_parked();
            }
            let presses = cx.update(|cx| workspace.read(cx).presses);
            let has_it = has(ends.1, window, &workspace, cx);
            if presses != 3 || !has_it {
                failed.push(format!(
                    "with the keyboard in {what}, {keys} pressed 3 times worked {presses} times; \
                     {} has the keyboard = {has_it}",
                    ends.0
                ));
            }
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    /// What went off screen with the keyboard has it again once it is shown again
    /// ([`super::KeyboardHome::give_back`], review I4): a terminal pane hidden and shown by the
    /// title bar's button (clicks, no key) or by ⌘J twice, and a file's text whose tab is left and
    /// come back to by clicks, take the typing that follows, as they did before the workspace took
    /// the keyboard from what is hidden. What has been clicked meanwhile keeps the keyboard, and
    /// the browser's page (here the stand-in's page) waits for a click, as it always did.
    #[gpui_kit::test]
    fn what_is_shown_again_has_the_keyboard_again(cx: &mut gpui_kit::TestAppContext) {
        use self::keyboard::{Keyboard, has, open, show};
        use gpui_kit::{AppContext as _, Focusable as _};

        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let mut failed = Vec::new();
        let home: Keyboard = |this, _| this.focus.clone();
        let pane: Keyboard = |this, _| this.pane.clone();
        let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);
        let page: Keyboard = |this, _| this.page.clone();

        let (window, workspace) = open(pane, cx);
        for dock in [false, true] {
            show(&workspace, cx, |w| w.dock = dock);
        }
        cx.simulate_keystrokes(window, "l s");
        let typed = cx.update(|cx| workspace.read(cx).typed.clone());
        if typed != ["l", "s"] {
            failed.push(format!(
                "the terminal pane hidden and shown by clicks got {typed:?} of `l s`"
            ));
        }

        let (window, workspace) = open(pane, cx);
        for _ in 0..2 {
            cx.simulate_keystrokes(window, "cmd-j");
            cx.run_until_parked();
        }
        cx.simulate_keystrokes(window, "l s");
        let (dock, typed) = cx.update(|cx| {
            let workspace = workspace.read(cx);
            (workspace.dock, workspace.typed.clone())
        });
        if !dock || typed != ["l", "s"] {
            failed.push(format!(
                "after cmd-j twice the dock shows = {dock}, and the pane got {typed:?} of `l s`"
            ));
        }

        let (window, workspace) = open(text, cx);
        for tab in [1, 0] {
            show(&workspace, cx, |w| w.tab = tab);
        }
        cx.simulate_input(window, "hi");
        let value = cx.update(|cx| workspace.read(cx).text.read(cx).value().to_string());
        if value != "hi" {
            failed.push(format!(
                "the file's text, its tab left and come back to, got {value:?} of \"hi\""
            ));
        }

        let (window, workspace) = open(text, cx);
        show(&workspace, cx, |w| w.tab = 1);
        let to = cx.update(|cx| pane(workspace.read(cx), cx));
        cx.update_window(window, |_, window, cx| window.focus(&to, cx))
            .unwrap();
        cx.run_until_parked();
        show(&workspace, cx, |w| w.tab = 0);
        if !has(pane, window, &workspace, cx) {
            failed.push(
                "the terminal pane clicked while the text was hidden lost the keyboard when \
                 the text was shown again"
                    .into(),
            );
        }

        let (window, workspace) = open(page, cx);
        for tab in [1, 0] {
            show(&workspace, cx, |w| w.tab = tab);
        }
        if !has(home, window, &workspace, cx) {
            failed.push("the page, hidden and shown, took the keyboard back".into());
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }

    /// Given back while it is still off screen, the keyboard comes home ([`super::KeyboardHome`],
    /// review I5): GPUI takes it from what isn't drawn only in a frame after one with a keyboard
    /// path, and the frame that hid it had none. So two changes on screen in a row, the second
    /// showing something else again, left the keyboard on the hidden view, past every workspace
    /// shortcut, until a click: ⌘J, ⌃Tab, ⌘J from a terminal pane left the dock hidden; ⌃Tab,
    /// ⌘J, ⌃Tab from a file's text lost the last ⌃Tab. Also: what is clicked meanwhile makes the
    /// workspace forget what it took, and a redraw that changes nothing gives nothing back (one
    /// would, given back to what is hidden, come home and ask for the next frame, without end).
    #[gpui_kit::test]
    fn the_keyboard_never_stays_on_what_is_hidden(cx: &mut gpui_kit::TestAppContext) {
        use self::keyboard::{Keyboard, has, open, show};
        use gpui_kit::{AppContext as _, Focusable as _};

        cx.update(|cx| {
            gpui_kit::init(cx);
            super::init(cx);
        });
        let mut failed = Vec::new();
        let home: Keyboard = |this, _| this.focus.clone();
        let pane: Keyboard = |this, _| this.pane.clone();
        let text: Keyboard = |this, cx| this.text.read(cx).focus_handle(cx);

        for (what, from, keys, presses, ends) in [
            (
                "⌘J, ⌃Tab, ⌘J",
                pane,
                "cmd-j ctrl-tab cmd-j",
                3,
                ("the pane", pane),
            ),
            (
                "⌃Tab, ⌘J, ⌃Tab, ⌃Tab",
                text,
                "ctrl-tab cmd-j ctrl-tab ctrl-tab",
                4,
                ("the text", text),
            ),
        ] {
            let (window, workspace) = open(from, cx);
            for key in keys.split(' ') {
                cx.simulate_keystrokes(window, key);
                cx.run_until_parked();
            }
            let worked = cx.update(|cx| workspace.read(cx).presses);
            let has_it = has(ends.1, window, &workspace, cx);
            if worked != presses || !has_it {
                failed.push(format!(
                    "{what}: {worked} of {presses} presses worked; {} has the keyboard = {has_it}",
                    ends.0
                ));
            }
        }

        // The text's tab left, then the dock hidden, both by clicks: ⌘J still works.
        let (window, workspace) = open(text, cx);
        show(&workspace, cx, |w| w.tab = 1);
        show(&workspace, cx, |w| w.dock = false);
        cx.simulate_keystrokes(window, "cmd-j");
        cx.run_until_parked();
        let (worked, dock) = cx.update(|cx| (workspace.read(cx).presses, workspace.read(cx).dock));
        if worked != 1 || !dock || !has(home, window, &workspace, cx) {
            failed.push(format!(
                "after the text's tab and the dock were hidden by clicks, cmd-j worked {worked} \
                 times and the dock shows = {dock}"
            ));
        }

        // What is clicked meanwhile makes the workspace forget the text: the workspace gets the
        // keyboard back another way (as a canvas going off screen gives it), and the text's tab
        // shown again doesn't take it.
        let (window, workspace) = open(text, cx);
        show(&workspace, cx, |w| w.tab = 1);
        for to in [pane, home] {
            let to = cx.update(|cx| to(workspace.read(cx), cx));
            cx.update_window(window, |_, window, cx| window.focus(&to, cx))
                .unwrap();
            cx.run_until_parked();
        }
        show(&workspace, cx, |w| w.tab = 0);
        if !has(home, window, &workspace, cx) {
            failed.push("the text took the keyboard back after the pane had it meanwhile".into());
        }

        // A redraw that changes nothing gives nothing back; the text's tab shown again does.
        let (window, workspace) = open(text, cx);
        show(&workspace, cx, |w| w.tab = 1);
        show(&workspace, cx, |_| {});
        let home_had_it = has(home, window, &workspace, cx);
        show(&workspace, cx, |w| w.tab = 0);
        if !home_had_it || !has(text, window, &workspace, cx) {
            failed.push(format!(
                "after a redraw with the text's tab hidden the workspace had the keyboard = \
                 {home_had_it}, and the text didn't get it back when shown"
            ));
        }
        assert!(failed.is_empty(), "{}", failed.join("\n"));
    }
}
