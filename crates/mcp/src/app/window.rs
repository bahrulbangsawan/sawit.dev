//! The tools for Sawit's windows: what they show, opening a file or a folder, the window in front,
//! the sidebar's panel, the layout, pages and center tabs.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::{Command, FillPanel, Page, Panel};

use super::{FILL_PANELS, PAGES, PANELS, variant};
use crate::{Sawit, fail};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct WindowArgs {
    #[schemars(
        description = "Project folder (or a path inside it) whose window to use; default the window with your working folder open, else the frontmost"
    )]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    #[schemars(description = "Absolute path of a file or folder to open")]
    pub path: String,
    #[schemars(description = "1-based line to put the cursor on (files)")]
    pub line: Option<u32>,
    #[schemars(description = "1-based column (files)")]
    pub column: Option<u32>,
    #[schemars(description = "Always open a new window")]
    #[serde(default)]
    pub new_window: bool,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PanelArgs {
    #[schemars(
        description = "instances, files, git, database, run, ports, network, prompts, ssh or shell_config (the Config Files panel)"
    )]
    pub panel: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LayoutArgs {
    #[schemars(description = "Show (true) or hide (false) the left sidebar")]
    pub sidebar: Option<bool>,
    #[schemars(description = "Show or hide the bottom terminal dock")]
    pub terminal: Option<bool>,
    #[schemars(description = "Show or hide the tabs (center); tabs or terminal stay visible")]
    pub center: Option<bool>,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct MaximizeArgs {
    #[schemars(
        description = "terminal, browser or canvas: the panel to fill the window with (the browser or canvas opens if it isn't); the panel that fills it already puts the layout back"
    )]
    pub panel: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    #[schemars(
        description = "canvas, browser, mcp, tools, skills, changelog, git_graph, github, instance, keymap, new_project, permissions or accounts"
    )]
    pub page: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TabArgs {
    #[schemars(description = "Tab index (from app_state) or title")]
    pub tab: String,
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CloseAllTabsArgs {
    #[schemars(
        description = "false (default): Safe Close All, the tabs that lose nothing and aren't pinned. true: Force Close All, every tab"
    )]
    #[serde(default)]
    pub force: bool,
    #[schemars(
        description = "With force: close even though unsaved changes, browser notes not yet sent or an install going on would be lost. Without it such a close is refused, naming what"
    )]
    #[serde(default)]
    pub confirm: bool,
    pub project: Option<String>,
}

#[tool_router(router = window_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "What the Sawit app shows: each window's project, sidebar panel, layout, open tabs (index, title, kind, path, unsaved) and terminal tabs (index, title, running job, script, ssh, and `agent`: the coding agent running there, such as claude-code or codex, with a tab's `agents` listing every one of its panes') and each tab's split panes (each with its `agent`), and `browser`: the browser tab of the window's active project (url, title, loading, viewport, panel, pick_mode), which may not be the browser your browser_* tools use (theirs is your project's; browser_snapshot shows its page). `github` is the app's one GitHub sign-in: signed_in, and login (never a token); only the user can sign in. `agents` lists the AI agents at work on a canvas or browser now (id, name, color, surface, action, seconds_ago), you among them"
    )]
    async fn app_state(&self) -> CallToolResult {
        self.app_tool(None, Command::State).await
    }

    #[tool(
        description = "Open a folder as a project or a file (at a line/column) in Sawit, like `sawit <path>`"
    )]
    async fn app_open(&self, Parameters(a): Parameters<OpenArgs>) -> CallToolResult {
        let command = Command::Open {
            path: PathBuf::from(a.path),
            line: a.line,
            column: a.column,
            new_window: a.new_window,
        };
        self.app_tool(None, command).await
    }

    #[tool(description = "Bring a Sawit window to the front")]
    async fn app_focus(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::Focus).await
    }

    #[tool(description = "Show a sidebar panel in Sawit")]
    async fn app_show_panel(&self, Parameters(a): Parameters<PanelArgs>) -> CallToolResult {
        match variant::<Panel>(&a.panel, PANELS) {
            Ok(panel) => self.app_tool(a.project, Command::ShowPanel { panel }).await,
            Err(e) => fail(e),
        }
    }

    #[tool(description = "Show or hide the sidebar, the tabs and the terminal dock")]
    async fn app_layout(&self, Parameters(a): Parameters<LayoutArgs>) -> CallToolResult {
        let command = Command::Layout {
            sidebar: a.sidebar,
            terminal: a.terminal,
            center: a.center,
        };
        self.app_tool(a.project, command).await
    }

    #[tool(
        description = "Toggle filling the window with a panel: terminal, browser or canvas (no sidebar, other areas, tab bar or status bar), and back to the layout as it was. Like ⇧⌘↩ (⌃⌘1/2/3), but the keyboard stays where the user has it. Answers the panel that fills the window now"
    )]
    async fn app_maximize(&self, Parameters(a): Parameters<MaximizeArgs>) -> CallToolResult {
        match variant::<FillPanel>(&a.panel, FILL_PANELS) {
            Ok(panel) => self.app_tool(a.project, Command::Maximize { panel }).await,
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Toggle filling the window with the terminal (and back): app_maximize with panel terminal"
    )]
    async fn app_maximize_terminal(&self, Parameters(a): Parameters<WindowArgs>) -> CallToolResult {
        let panel = FillPanel::Terminal;
        self.app_tool(a.project, Command::Maximize { panel }).await
    }

    #[tool(
        description = "Open a page as a tab: canvas (whiteboard and design canvas), browser (web browser agents can drive), mcp (install page), tools (dev tools installer), skills (agent skills installer), changelog, git_graph, github (pull requests, issues and your repositories; while the user isn't signed in to GitHub it shows them the sign-in card, and the reply says to sign in to GitHub in Sawit first), instance (selected PostgreSQL instance), keymap (keymap editor), new_project (stack builder that creates a TypeScript project; stack_create creates one), permissions (what agents may do: Claude Code's rules, Sawit's confirm tools, macOS permissions; list_permissions and edit_permission work without the app), accounts (Agent Accounts: the coding agents' sign-ins)"
    )]
    async fn app_open_page(&self, Parameters(a): Parameters<PageArgs>) -> CallToolResult {
        match variant::<Page>(&a.page, PAGES) {
            Ok(page) => self.app_tool(a.project, Command::OpenPage { page }).await,
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Switch to a center tab by index or title, as a click on it does: its project becomes the active one"
    )]
    async fn app_activate_tab(&self, Parameters(a): Parameters<TabArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::ActivateTab { tab: a.tab })
            .await
    }

    #[tool(
        description = "Close a center tab by index or title. A tab that would lose something asks the user first, as their own close does: a file with unsaved changes, the browser with annotations not yet copied or sent, the Dev Tools page while it installs. The reply then says \"asking\", not \"closed\": the tab closes only if the user agrees (see app_state)"
    )]
    async fn app_close_tab(&self, Parameters(a): Parameters<TabArgs>) -> CallToolResult {
        self.app_tool(a.project, Command::CloseTab { tab: a.tab })
            .await
    }

    #[tool(
        description = "Close the center tabs at once, as the tab menu's Safe Close All and Force Close All do. Safe (the default) closes the tabs that lose nothing and aren't pinned, and says how many it kept. DESTRUCTIVE with force: every tab closes, pinned ones too, unsaved changes are discarded; refused unless confirm is true while something would be lost, naming it"
    )]
    async fn app_close_all_tabs(
        &self,
        Parameters(a): Parameters<CloseAllTabsArgs>,
    ) -> CallToolResult {
        let command = Command::CloseAllTabs {
            force: a.force,
            confirm: a.confirm,
        };
        self.app_tool(a.project, command).await
    }
}
