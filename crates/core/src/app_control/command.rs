//! The commands the app runs for `sawit-mcp`, and the names of the panels, pages and actions
//! they take.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Instances,
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

/// Single-tab pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Canvas,
    Browser,
    Mcp,
    Tools,
    Skills,
    /// Agent Accounts: Claude Code's accounts, their sign-in and usage.
    Accounts,
    Changelog,
    GitGraph,
    Github,
    Instance,
    Keymap,
    /// The New Project page: a TypeScript stack from Better-T-Stack's builder.
    NewProject,
    /// The Permissions page: what agents may do on this Mac.
    Permissions,
}

/// A panel that can fill the window (`maximize`): the terminal dock, or the browser's or the
/// canvas's tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FillPanel {
    Terminal,
    Browser,
    Canvas,
}

/// Data the app keeps in memory and saves itself; after another process
/// changes the file, the app reads it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    Instances,
    Connections,
    Prompts,
    Ssh,
    RunCommands,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstanceAction {
    Select,
    Start,
    Stop,
    Restart,
}

/// Where a terminal split puts the new pane, as the pane menu's Split right and Split down do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Side by side, the new pane on the right (⌘D).
    Right,
    /// Stacked, the new pane below (⇧⌘D).
    Down,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Windows, their project, layout, tabs and terminals.
    State,
    /// Brings the window forward.
    Focus,
    /// A file (at a 1-based line/column) or a folder, as `sawit <path>` does.
    Open {
        path: PathBuf,
        #[serde(default)]
        line: Option<u32>,
        #[serde(default)]
        column: Option<u32>,
        #[serde(default)]
        new_window: bool,
    },
    ShowPanel {
        panel: Panel,
    },
    /// Shows (`true`) or hides each area; `None` leaves it as is.
    Layout {
        #[serde(default)]
        sidebar: Option<bool>,
        #[serde(default)]
        terminal: Option<bool>,
        #[serde(default)]
        center: Option<bool>,
    },
    /// Fills the window with the terminal (`maximize` with `panel: terminal`, kept for older
    /// clients).
    MaximizeTerminal,
    /// Fills the window with `panel`, or puts the layout back when it already fills it.
    Maximize {
        panel: FillPanel,
    },
    OpenPage {
        page: Page,
    },
    /// Activates the tab with this index (from `state`) or title, and its project.
    ActivateTab {
        tab: String,
    },
    /// Closes the tab with this index or title; one that would lose something (unsaved changes,
    /// unsent annotations, a Dev Tools install) asks the user first, and the answer says so.
    CloseTab {
        tab: String,
    },
    /// Types `command` and Enter into a shell: a new one in `cwd` when given
    /// or `new_tab` (without `cwd`, in the agent's own project, as for its
    /// browser), else the active or newest idle shell, never the one the
    /// user is typing in.
    TerminalRun {
        command: String,
        #[serde(default)]
        cwd: Option<PathBuf>,
        #[serde(default)]
        new_tab: bool,
    },
    /// The text of a terminal pane, in the tab with this index (from
    /// `state`, default the active one): its scrollback tail and screen, up
    /// to `lines` lines.
    TerminalRead {
        #[serde(default)]
        tab: Option<usize>,
        /// The pane within the tab. Without one: the active tab's focused
        /// pane, or a named tab's only pane (a split tab needs it named).
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        lines: Option<usize>,
    },
    /// Sends raw input to a terminal: text as typed, or a key named in
    /// [`KEY_NAMES`](super::KEY_NAMES) (`ctrl-c`, `enter`, `up`). `sawit-mcp` always names the
    /// tab.
    TerminalInput {
        #[serde(default)]
        tab: Option<usize>,
        /// The pane within the tab, as for `TerminalRead`.
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        text: Option<String>,
        #[serde(default)]
        key: Option<String>,
    },
    /// Closes a terminal tab, or only one of its panes, ending what runs
    /// there. Without `confirm` it is refused while a program runs there (a
    /// dev server, an SSH session, an agent) or the user is typing in it.
    TerminalClose {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
        #[serde(default)]
        confirm: bool,
        /// Closes other tabs from `tab` instead, as the tab's menu does: `others`, `left`,
        /// `right`, `idle` (Safe Close All: tabs with nothing running) or `all` (Force Close
        /// All). Locked tabs always stay; without `confirm` it is refused while a tab it would
        /// close runs something.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scope: Option<String>,
    },
    /// Clears a terminal pane's scrollback and screen, as its menu's Clear (⌘K) does.
    TerminalClear {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
    },
    /// Closes the center tabs: safe (`force` false), the tabs that lose nothing and aren't
    /// pinned, as Safe Close All does; or every tab (Force Close All), refused without `confirm`
    /// while that would lose unsaved changes, notes not yet sent or an install going on.
    CloseAllTabs {
        #[serde(default)]
        force: bool,
        #[serde(default)]
        confirm: bool,
    },
    /// The Run panel's scripts and saved commands, with running state.
    RunList,
    /// Starts a script from the Run panel (`dev`, or `web · dev` in a
    /// workspace package, as `RunList` names it) or a saved command, in its
    /// own terminal.
    RunScript {
        name: String,
    },
    StopScript {
        name: String,
    },
    /// Opens an SSH session to a saved server (id) or `~/.ssh/config` alias.
    SshConnect {
        host: String,
        #[serde(default)]
        new_session: bool,
    },
    OpenTable {
        connection: String,
        schema: String,
        table: String,
    },
    OpenSql {
        connection: String,
        #[serde(default)]
        sql: Option<String>,
        /// Runs it right away (once connected), as ⌘↩ does.
        #[serde(default)]
        run: bool,
    },
    /// Selects an instance in the Instances tab and starts, stops or
    /// restarts it there. A start whose port is taken is held, and the
    /// instance tab explains the conflict, as when clicking Start.
    Instance {
        name: String,
        action: InstanceAction,
    },
    /// Runs the Network tab's diagnosis, and pings `ping` when given.
    NetworkDiagnose {
        #[serde(default)]
        ping: Option<String>,
    },
    OpenDiagram {
        connection: String,
        schema: String,
    },
    OpenPrompt {
        id: String,
    },
    /// Changes the canvas (opening it if needed): one op of `sawit_board::ops`, answered at once.
    /// The canvas is the one of the agent's project, or with `file` the canvas of that file (an
    /// absolute path to a `.sawit` or an `.excalidraw` file), in a canvas tab of its own as
    /// `sawit <file>` opens it.
    Canvas {
        op: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file: Option<PathBuf>,
    },
    /// Drives the project's browser tab; see `crate::browser`. Only the ops that show a page
    /// ([`BrowserOp::shows_tab`](crate::browser::BrowserOp::shows_tab): navigate, screenshot,
    /// resize) open it; the others answer that it isn't open ([`crate::browser::NOT_OPEN`]).
    ///
    /// With `tab` (an id, a title or an address, [`crate::browser::find_tab`]) the op goes to that
    /// tab of the browser, woken off screen if need be: the user's tab on show and the keyboard
    /// stay as they are.
    Browser {
        op: crate::browser::BrowserOp,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
    },
    /// The tabs of the project's browser: `list`, `open` (a new tab at `url`, behind the one on
    /// show unless `show`), `close` or `activate` the tab named `tab` (`show` brings the browser
    /// forward too).
    BrowserTabs {
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default)]
        show: bool,
    },
    /// The project's canvases, each a `.sawit` file in `.nyawit/` with a tab of its own: `list`,
    /// `open` (the one named `tab`, made new when there is none), `close` or `activate` it, or
    /// `rename` it to `name`. The project's `canvas.sawit` is the first.
    CanvasTabs {
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tab: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(default)]
        show: bool,
    },
    Reload {
        store: Store,
    },
    /// Splits pane `pane` of terminal tab `tab` (a split tab needs it
    /// named), as the pane menu's Split right and Split down do, and starts
    /// a shell in the new half: in `cwd`, else the split pane's folder. It
    /// types `command` and Enter there when given. The new pane never takes
    /// the keyboard, and a split that would leave a half under 20 columns by
    /// 4 rows is refused.
    TerminalSplit {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
        direction: SplitDirection,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        cwd: Option<PathBuf>,
    },
    /// Reads or changes the key bindings, which every window shares; see
    /// [`crate::keymap`].
    Keymap {
        op: crate::keymap::KeymapOp,
    },
    /// Names a terminal tab, or one pane of a split tab, as renaming it in the tab strip or the
    /// pane's header does: a tab of one pane is named itself, as its pane's rename does there. A
    /// blank name clears it, back to the title the shell gives.
    TerminalRename {
        tab: usize,
        #[serde(default)]
        pane: Option<usize>,
        name: String,
    },
    /// Locks a terminal tab so nothing closes it (the user's ×, a key, Close Others, an agent's
    /// TerminalClose) until it is unlocked, or unlocks it: `locked`, or the other way round
    /// when it is left out.
    TerminalLock {
        tab: usize,
        #[serde(default)]
        locked: Option<bool>,
    },
    /// Pins a terminal tab to the start of the strip (it comes back when its project opens
    /// again), or unpins it: `pinned`, or the other way round when left out.
    TerminalPin {
        tab: usize,
        #[serde(default)]
        pinned: Option<bool>,
    },
    /// Creates a project with the New Project page, as Create Project does: the page shows
    /// `stack` (resolved, named by its `project_name`), runs its create in a terminal tab of its
    /// own in `location`, and opens the project once it is made, or, with `open` false (Create
    /// Project Only), leaves it made, with the page's Open Project button.
    NewProject {
        location: PathBuf,
        stack: crate::stack_builder::Stack,
        /// An op without it, from an older `sawit-mcp`, opens the project as before.
        #[serde(default = "opens")]
        open: bool,
    },
}

/// [`Command::NewProject`]'s `open` when the op leaves it out: Create Project's, which opens.
fn opens() -> bool {
    true
}
