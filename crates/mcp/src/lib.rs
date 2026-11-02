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
pub mod sawit_skills;
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

/// What the server tells agents when they connect: an overview, the details being each tool's
/// own. Claude Code keeps only the first 4,096 characters of a server's instructions, so they
/// stay at 4,000 at most, and the step an agent designing UI takes first comes first.
pub const INSTRUCTIONS: &str = "\
Sawit is a macOS developer workbench. Before you design, build or review UI on the Sawit canvas, read canvas_design_guide and follow it.
These tools work without the Sawit app running:
- Instances: local PostgreSQL servers Sawit manages (create, start, stop, logs, …).
- Databases: detect; PostgreSQL: list, describe, create, drop, truncate, refresh, export, back up, import CSV; D1 and SQLite: list, describe, export; `query` (always read-only), `execute_sql` (read-write); saved connections, DATABASE_URL in .env. `target`: an instance or connection name (list_connections), or an id from detect_databases.
- Diagrams: render_diagram draws Mermaid syntax to SVG with Pele (built in, offline); schema_diagram draws a database schema.
- Ports: listening dev servers; stop them.
- Cloudflare Local Explorer: local_explorer finds Workers dev servers' explorers; local_explorer_request calls their API.
- Network: diagnose the connection, ping a host.
- SSH: saved and ~/.ssh/config hosts: add, update, remove, probe, run a command.
- Run: a project's scripts (package.json, Makefile, …) and saved commands.
- Git: status, log, diff, show, branches, stage/unstage, commit, switch.
- Prompts: the prompt library (with {{variables}}).
- Files: text search in a folder.
- Config files: list_config_files lists shell and agent config files (paths, never contents).
- Dev tools: the Tools page catalog (Homebrew, Bun, uv, Claude Code, …): install, upgrade, uninstall.
- MCP servers: the MCP page catalog; install or remove a server in an AI client's config.
- New project: stack_options, stack_check and stack_create build a Better-T-Stack project.
- Skills: the Skills page's packs per agent: install, update, remove; install_sawit_skills (Sawit's own).
These need the running app (launch_app starts it; app_state shows windows, tabs and terminals):
- app_*: open files and folders, panels, layout, tabs, pages (canvas, MCP, git graph, …), Run panel scripts, SSH sessions, table/SQL/diagram tabs, prompts.
- GitHub: only the user can sign in to GitHub in Sawit; app_state's `github` says if they have.
- terminal_*: run commands in Sawit's terminal (terminal_split: beside one), read output, type keys, close tabs or panes. terminal_run, terminal_split, app_run_script and app_ssh_connect return the tab and `pane` they used: pass both to terminal_read, terminal_input and terminal_close, since the user may be typing in another terminal (app_state lists them).
- canvas_*: the Sawit canvas: list, clear and export SVG/PNG the page on show; add shapes, stickies, arrows and Mermaid diagrams (canvas_add_diagram); update and delete layers by id, on any page; load another canvas in place of every page. They use your project's canvas (.nyawit/canvas.sawit), or the .sawit file named by `file`. They also work on the saved file when the app is closed, and keep what they clear, load over or delete in backups beside it (.bak, .del.bak).
- canvas_command: run any canvas engine command by id (board.sticky, align.left, edit.undo, …); canvas_commands lists the ids, and with a filter what each takes. With the app closed each call starts from the saved file (no undo, no selection, no view): pass ids, or run the steps in one command.batch.
- browser_*: a browser tab (WebKit) to check the app being built: navigate, snapshot (an outline with element refs), act by ref or selector, screenshot, console, network, JS, devices, the user's annotations, the page's design. Snapshot again after the page changes: refs reset.
- keymap_*: read and change the key bindings (keymap_list, keymap_bind, keymap_unbind). They need the app: the default keys live in it.
Most app tools take `project` to pick the window that has that folder open; without it, the window that has the folder sawit-mcp runs in open is used, else the frontmost. The browser_* and canvas_* tools (and app_open_page browser or canvas) use that project's own browser tab and canvas, even while the user looks at another project of the window.";

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
            + Self::sawit_skills_router()
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
