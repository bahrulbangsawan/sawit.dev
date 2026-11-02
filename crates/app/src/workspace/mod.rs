//! The main window, laid out like Zed: title bar, left panel with tabs
//! (Instances / Files / Git / Database / Run / Ports / Network / Prompts / SSH), tabbed center
//! pane, bottom terminal dock and status bar. A new, empty window starts on the welcome screen instead.
//! One window holds several projects (see `project`): the title bar switches
//! between them and tabs are grouped by the project they belong to.

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

mod areas;
mod browser;
mod canvas;
mod close;
mod control;
mod dock;
mod explorer;
mod file_tabs;
mod keyboard;
mod new_project;
mod pages;
mod pages_menu;
mod panel_tabs;
mod project;
mod render;
mod setup;
mod sidebar;
mod status_bar;
mod tab_bar;
mod tab_list;
mod title_bar;

pub use control::Answer;
pub(crate) use keyboard::KeyboardHome;
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
        OpenBrowser,
        OpenDevTools,
        OpenMcp,
        OpenSkills,
        OpenNewProject,
        OpenChangelog,
        InstallUpdate,
        OpenLocalExplorer
    ]
);

/// Every workspace shortcut: Zed's defaults where Zed has the command; the
/// pages (listed in the account menu and the footer's Pages menu) are on ⌃⌘,
/// which nothing else here (inputs, the terminal, the canvas) uses. No two
/// share a keystroke (see the tests).
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
        KeyBinding::new("ctrl-cmd-n", OpenNewProject, ctx),
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
    /// The active project's folder, if it has one.
    pub fn root(&self, cx: &App) -> Option<PathBuf> {
        self.current().root(cx)
    }

    /// The window's terminal dock.
    pub fn terminal(&self) -> &Entity<TerminalPanel> {
        &self.terminal
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

#[cfg(test)]
mod tests;
