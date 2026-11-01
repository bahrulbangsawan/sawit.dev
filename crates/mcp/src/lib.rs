//! MCP server exposing Sawit to AI agents. Everything here works without the
//! app running: local PostgreSQL instances, databases and saved connections
//! (with exports, backups and CSV imports), diagrams, ports, network checks,
//! SSH hosts, run commands, git, prompts, file search, developer tools and
//! the MCP servers installed in AI clients. Tools are grouped by area, one module each, and every
//! module contributes its own router (see [`Sawit::router`]).

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, ContentBlock as Content, Implementation, InitializeRequestParams,
    InitializeResult, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, tool_handler};
use sawit_core::app_control::{self, Store};
use sawit_core::instances::{AppPaths, Instance};

pub mod agent;
/// Every diagram type the diagram tools draw, by the keyword a source starts with
/// (`sawit_core::mermaid::TYPES`, which a test holds this to), for their descriptions.
macro_rules! diagram_types {
    () => {
        "flowchart, graph, flowchart-elk, swimlane-beta, sequenceDiagram, classDiagram, \
         stateDiagram, stateDiagram-v2, erDiagram, gantt, gitGraph, pie, mindmap, kanban, \
         timeline, journey, quadrantChart, xychart, xychart-beta, requirementDiagram, sankey, \
         sankey-beta, architecture-beta, C4Context, C4Container, C4Component, C4Dynamic, \
         C4Deployment, block, block-beta, agentflow-beta, railroad-beta, railroad-ebnf-beta, \
         railroad-abnf-beta, railroad-peg-beta, treeView-beta, cynefin-beta, wardley-beta, \
         eventmodeling, usecase-beta, venn-beta, ishikawa, ishikawa-beta, packet, packet-beta, \
         radar-beta, treemap, treemap-beta and info"
    };
}

pub mod app;
pub mod browser;
pub mod browser_design;
pub mod config_files;
pub mod db;
pub mod db_admin;
#[cfg(test)]
mod db_d1_tests;
pub mod dev_tools;
pub mod diagrams;
pub mod files;
pub mod git;
mod hints;
pub mod instances;
pub mod keymap;
pub mod local_explorer;
pub mod mcp_servers;
pub mod network;
pub mod new_project;
mod paths;
pub mod ports;
pub mod prompts;
pub mod scripts;
pub mod skills;
pub mod ssh;

use hints::hints;
pub(crate) use paths::{
    agent_path, agent_project, existing_dir, expand_home, output_path, project_dir,
    refuse_canvas_file, tilde,
};
pub use ssh::askpass;

/// The `sawit-mcp` program: a stdio MCP server, or (run by `ssh` as
/// SSH_ASKPASS from `ssh_exec`) the answer to a password prompt. Release
/// builds ship it inside the `sawit` binary, reached through a `sawit-mcp`
/// symlink next to it (see `invoked_as_mcp` in the app's main.rs).
pub fn main() -> std::process::ExitCode {
    if let Some(account) = std::env::var_os(sawit_core::ssh::ASKPASS_ENV) {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        return (askpass(&account.to_string_lossy(), &prompt) as u8).into();
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let served = runtime.block_on(async {
        use rmcp::ServiceExt as _;
        let service = Sawit::new(AppPaths::default_macos())
            .serve(rmcp::transport::stdio())
            .await?;
        service.waiting().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    });
    match served {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

/// Text returned by one tool call at most; longer output is cut with a note.
const MAX_OUTPUT: usize = 100_000;

/// What the server tells agents when they connect.
pub const INSTRUCTIONS: &str = "\
Sawit is a macOS developer workbench. These tools work without the Sawit app running:
- Instances: local PostgreSQL servers managed by Sawit (list/create/start/stop/restart/remove, logs, port checks, installed binaries).
- Databases: detect every reachable database, list/create/drop databases, describe schemas and tables, `query` (always read-only) and `execute_sql` (read-write: can modify data), saved connections, writing DATABASE_URL to a project's .env; export tables or queries to CSV/Excel, pg_dump backups, CSV imports, drop/truncate (with confirm) and refresh relations. Cloudflare D1 (a Workers project's local SQLite files and remote databases) and SQLite files are targets too, by the ids detect_databases gives with project_dir.
- Diagrams: render_diagram draws a diagram written in Mermaid syntax to SVG with Pele (https://pele.run, built in and offline; mermaid.js isn't used), any type Pele draws: flowchart, swimlane, sequence, class, state, ER, gantt, git graph, pie, mindmap, kanban, timeline, journey, quadrant, XY, requirement, sankey, architecture, C4, block, agentflow, railroad, tree view, Cynefin, Wardley, event modeling, use case, Venn, Ishikawa, packet, radar, treemap, info (not ZenUML). schema_diagram draws a database schema as Mermaid, DBML or SVG. render_mermaid and canvas_add_mermaid are the old names of render_diagram and canvas_add_diagram.
- Ports: list listening dev servers and stop them.
- Cloudflare Local Explorer: local_explorer finds the explorers Workers dev servers (wrangler dev, vite with the Cloudflare plugin, cf dev) serve at http://localhost:<port>/cdn-cgi/local/explorer, the project's Workers, their bindings and whether the installed Wrangler or Vite plugin has it; local_explorer_request calls its API on loopback (GET reads; POST, PUT, PATCH and DELETE write local data and need confirm: true).
- Network: diagnose the internet connection, ping a host.
- SSH: saved servers and ~/.ssh/config hosts; add/update/remove, probe, run a non-interactive command.
- Run: a project's scripts (package.json, Makefile, …) and saved run commands.
- Git: status, log, diff, show, branches, stage/unstage, commit, switch.
- Prompts: the prompt library (with {{variables}}).
- Files: text search in a folder.
- Config files: the Config Files panel's list (shell startup files, and every coding agent's config files on this Mac and in the project), as paths and what each is, never contents: list_config_files.
- Dev tools: the Tools page catalog (Homebrew, Bun, uv, Claude Code, …), what is installed (and where) and what has a newer release, installing with prerequisites, upgrading and uninstalling (confirm=true).
- MCP servers: the MCP page catalog and the AI clients on this Mac; install or remove a server in a client's config (local servers and the remote bridge run with npx, pnpm dlx, bunx or yarn dlx: package_manager).
- New project: the New Project page's stack builder (Better-T-Stack, create-better-t-stack 3.49.0): stack_options lists every category, option and preset and the default stack; stack_check resolves a stack by the CLI's flag names (what the compatibility rules change, every disabled option and why, the command); stack_create creates the project (confirm=true), in a Sawit terminal that opens it as a project when the app runs, else here.
- Skills: the Skills page catalog of agent skill packs (Superpowers, Cloudflare, MCP Apps, …) and which agents on this Mac have each; install, update or remove one for chosen agents, with the skills CLI (package_manager), a Claude Code plugin or a clone and copy.
These control the running Sawit app (start it with launch_app; app_state shows windows, tabs and terminals):
- app_*: open files/folders, show panels, layout, tabs, pages (canvas, MCP, tools, git graph, …), Run panel scripts, SSH sessions, table/SQL/diagram tabs, prompts.
- GitHub: Sawit's GitHub features run on the user's one GitHub sign-in in the app, which no tool can do for them. app_state's `github` says whether they have signed in (signed_in, login); while they haven't, app_open_page github shows them the sign-in card and says to sign in to GitHub in Sawit first.
- terminal_*: run commands in Sawit's terminal and read their output, split a pane to run something beside it (terminal_split), type keys (ctrl-c), close tabs or panes. terminal_run, terminal_split, app_run_script and app_ssh_connect return the terminal (tab) and `pane` they used: pass both to terminal_read, terminal_input and terminal_close, since the user may be typing in another terminal, which stays in front (app_state lists tabs and panes, and the coding agent, such as Claude Code, running in each). terminal_input and terminal_split need the tab, and a split tab needs its pane; terminal_read without a tab reads the pane the user has in front, and terminal_close without a pane closes the whole tab. terminal_close ends what runs there: while a program runs or the user types there it needs confirm: true.
- canvas_*: the Sawit canvas: list, clear and export SVG/PNG the page on show; add shapes/stickies/arrows, and diagrams in Mermaid syntax as editable shapes (canvas_add_diagram; charts as one picture); update and delete layers by id, on any page; load another canvas in place of every page. They use your project's canvas (.nyawit/canvas.sawit), or the .sawit file named by `file` (a design file, say), in a canvas tab of its own. They also work on the saved file when the app is closed, and keep what they clear, load over or delete in backups beside it (.bak, .del.bak), which canvas_load reads back.
- canvas_command: run any command of the canvas engine by id with its parameters (board.sticky, node.set, align.left, edit.undo, …); canvas_commands lists the ids, and with a filter what each takes. With the app closed each call starts from the saved file, so nothing carries over between calls (no undo, no selection, no view): pass ids, or run the steps in one command.batch.
- browser_*: a browser tab (WebKit) to check the app being built: navigate (default the project's dev server), snapshot the page as an outline with element refs, click/hover/type/press keys/select/scroll by ref or CSS selector, wait for text, screenshot, read console errors and network requests, evaluate JS, resize to a device, read the user's annotations, inspect an element's design, read the page's design system, export it as DESIGN.md + tokens + assets. Take a new browser_snapshot after the page changes; refs reset.
- keymap_*: the key bindings, as the Keymap Editor shows them: keymap_list says what a key does or which key runs an action, keymap_bind and keymap_unbind change the user's bindings (they write keymap.json again without its comments, so a file with comments needs confirm: true). They need the app: the default keys live in it.
Most app tools take `project` to pick the window that has that folder open; without it, the window that has the folder sawit-mcp runs in open is used, else the frontmost window. The browser_* and canvas_* tools (and app_open_page browser or canvas) use that project's own browser tab and canvas, even while the user looks at another project of the window.
Targets: database tools take a Sawit instance name or a saved connection name (see list_connections).";

#[derive(Debug, Clone)]
pub struct Sawit {
    paths: AppPaths,
    tool_router: ToolRouter<Sawit>,
    /// The agent session this process serves, whose identity goes with every op to the app.
    agent: std::sync::Arc<agent::Session>,
}

pub(crate) fn ok(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![Content::text(clip(text.into(), MAX_OUTPUT))])
}

pub(crate) fn fail(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![Content::text(clip(text.into(), MAX_OUTPUT))])
}

/// `text` cut to at most `max` bytes (on a character boundary), with a note
/// saying how much was left out.
pub(crate) fn clip(text: String, max: usize) -> String {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n… (output truncated: {} of {} bytes shown)",
        &text[..end],
        end,
        text.len()
    )
}

/// Unwraps a `Result<T, String>` inside a tool, returning `fail(e)` on error.
macro_rules! tri {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(e) => return $crate::fail(e),
        }
    };
}
pub(crate) use tri;

/// Runs blocking work (Keychain, Pele, big file reads, the app socket) off
/// the async workers.
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("internal error: {e}"))
}

pub fn instance_url(inst: &Instance) -> String {
    format!("postgres://postgres@localhost:{}/postgres", inst.port)
}

/// Unix seconds as `YYYY-MM-DD HH:MM` (UTC).
pub(crate) fn utc_time(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60
    )
}

impl Sawit {
    /// A server for the Sawit data folder `paths`.
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            tool_router: Self::router(),
            agent: Default::default(),
        }
    }

    /// Every tool, from each area's router, each with the hints that say what it does to the
    /// world ([`hints()`]).
    pub fn router() -> ToolRouter<Sawit> {
        let mut router = Self::area_routers();
        for (name, route) in &mut router.map {
            route.attr.annotations = Some(hints(name));
        }
        router
    }

    fn area_routers() -> ToolRouter<Sawit> {
        Self::instances_router()
            + Self::db_router()
            + Self::db_admin_router()
            + Self::diagrams_router()
            + Self::ports_router()
            + Self::local_explorer_router()
            + Self::network_router()
            + Self::ssh_router()
            + Self::scripts_router()
            + Self::git_router()
            + Self::prompts_router()
            + Self::files_router()
            + Self::dev_tools_router()
            + Self::mcp_servers_router()
            + Self::skills_router()
            + Self::new_project_router()
            + Self::config_files_router()
            // App-control tools (need the app running).
            + Self::app_router()
            + Self::browser_router()
            + Self::browser_design_router()
            + Self::keymap_router()
    }

    /// Tells a running app to re-read `store` after a tool changed its file.
    pub(crate) async fn notify(&self, store: Store) {
        let paths = self.paths.clone();
        let _ = blocking(move || app_control::notify_reload(&paths, store)).await;
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Sawit {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("sawit-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    /// The client says who it is here, and the agent session takes its name
    /// ([`agent::Session::met`]); the rest is rmcp's own handshake.
    async fn initialize(
        &self,
        request: InitializeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<InitializeResult, ErrorData> {
        self.agent.met(&request.client_info);
        context.peer.set_peer_info(request.clone());
        self.negotiate_initialize(&request)
    }
}

#[cfg(test)]
mod tests;
