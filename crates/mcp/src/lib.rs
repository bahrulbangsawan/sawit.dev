//! MCP server exposing Sawit to AI agents. Everything here works without the
//! app running: local PostgreSQL instances, databases and saved connections
//! (with exports, backups and CSV imports), diagrams, ports, network checks,
//! SSH hosts, run commands, git, prompts, file search, developer tools and
//! the MCP servers installed in AI clients. Tools are grouped by area, one module each, and every
//! module contributes its own router (see [`Sawit::router`]).

use std::path::{Path, PathBuf};

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{
    CallToolResult, ContentBlock as Content, Implementation, ServerCapabilities, ServerConfig,
};
use rmcp::{ServerHandler, tool_handler};
use sawit_core::app_control::{self, Store};
use sawit_core::instances::{AppPaths, Instance};

pub mod app;
pub mod browser;
pub mod browser_design;
pub mod db;
pub mod db_admin;
pub mod dev_tools;
pub mod diagrams;
pub mod files;
pub mod git;
pub mod instances;
pub mod keymap;
pub mod mcp_servers;
pub mod network;
pub mod ports;
pub mod prompts;
pub mod scripts;
pub mod ssh;

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
- Databases: detect every reachable database, list/create/drop databases, describe schemas and tables, `query` (always read-only) and `execute_sql` (read-write: can modify data), saved connections, writing DATABASE_URL to a project's .env; export tables or queries to CSV/Excel, pg_dump backups, CSV imports, drop/truncate (with confirm) and refresh relations.
- Diagrams: render Mermaid to SVG, and draw a database schema as Mermaid, DBML or SVG.
- Ports: list listening dev servers and stop them.
- Network: diagnose the internet connection, ping a host.
- SSH: saved servers and ~/.ssh/config hosts; add/update/remove, probe, run a non-interactive command.
- Run: a project's scripts (package.json, Makefile, …) and saved run commands.
- Git: status, log, diff, show, branches, stage/unstage, commit, switch.
- Prompts: the prompt library (with {{variables}}).
- Files: text search in a folder.
- Dev tools: the Tools page catalog (Homebrew, Bun, uv, Claude Code, …), what is installed and what has a newer release, installing with prerequisites and upgrading (confirm=true).
- MCP servers: the MCP page catalog and the AI clients on this Mac; install or remove a server in a client's config.
These control the running Sawit app (start it with launch_app; app_state shows windows, tabs and terminals):
- app_*: open files/folders, show panels, layout, tabs, pages (canvas, MCP, tools, git graph, …), Run panel scripts, SSH sessions, table/SQL/diagram tabs, prompts.
- terminal_*: run commands in Sawit's terminal and read their output, split a pane to run something beside it (terminal_split), type keys (ctrl-c), close tabs or panes. terminal_run, terminal_split, app_run_script and app_ssh_connect return the terminal (tab) and `pane` they used: pass both to terminal_read, terminal_input and terminal_close, since the user may be typing in another terminal, which stays in front (app_state lists tabs and panes). terminal_input and terminal_split need the tab, and a split tab needs its pane; terminal_read without a tab reads the pane the user has in front, and terminal_close without a pane closes the whole tab. terminal_close ends what runs there: while a program runs or the user types there it needs confirm: true.
- canvas_*: the canvas (FigCraft engine): list, clear and export SVG/PNG the page on show; add shapes/stickies/arrows and Mermaid as editable shapes; update and delete layers by id, on any page; load another canvas in place of every page. They use your project's canvas (.nyawit/canvas.figcraft), or the .figcraft file named by `file` (a design file, say), in a canvas tab of its own. They also work on the saved file when the app is closed, and keep what they clear, load over or delete in backups beside it (.bak, .del.bak), which canvas_load reads back.
- canvas_command: run any command of the canvas engine by id with its parameters (board.sticky, node.set, align.left, edit.undo, …); canvas_commands lists the ids, and with a filter what each takes. With the app closed each call starts from the saved file, so nothing carries over between calls (no undo, no selection, no view): pass ids, or run the steps in one command.batch.
- browser_*: a browser tab (WebKit) to check the app being built: navigate (default the project's dev server), snapshot the page as an outline with element refs, click/hover/type/press keys/select/scroll by ref or CSS selector, wait for text, screenshot, read console errors and network requests, evaluate JS, resize to a device, read the user's annotations, inspect an element's design, read the page's design system, export it as DESIGN.md + tokens + assets. Take a new browser_snapshot after the page changes; refs reset.
- keymap_*: the key bindings, as the Keymap Editor shows them: keymap_list says what a key does or which key runs an action, keymap_bind and keymap_unbind change the user's bindings (they write keymap.json again without its comments, so a file with comments needs confirm: true). They need the app: the default keys live in it.
Most app tools take `project` to pick the window that has that folder open; without it, the window that has the folder sawit-mcp runs in open is used, else the frontmost window. The browser_* and canvas_* tools (and app_open_page browser or canvas) use that project's own browser tab and canvas, even while the user looks at another project of the window.
Targets: database tools take a Sawit instance name or a saved connection name (see list_connections).";

#[derive(Debug, Clone)]
pub struct Sawit {
    paths: AppPaths,
    tool_router: ToolRouter<Sawit>,
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

/// `~/x` for paths in the home folder.
pub(crate) fn tilde(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

/// An existing folder given by an agent (absolute, `~` expanded).
pub(crate) fn existing_dir(dir: &str) -> Result<PathBuf, String> {
    let path = expand_home(dir.trim());
    if !path.is_absolute() {
        return Err(format!("“{dir}” must be an absolute path."));
    }
    if !path.is_dir() {
        return Err(format!("{} is not a folder.", path.display()));
    }
    Ok(path)
}

/// The file `output_path` names for `tool` to write, if it names one, read as every tool that
/// writes a file reads it: trimmed (a blank one names none, and the tool picks its own file), `~`
/// for the home folder, absolute, and never a canvas file ([`refuse_canvas_file`]). It is checked
/// before anything is made, and it is the path written.
pub(crate) fn output_path(
    tool: &str,
    output_path: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let Some(p) = output_path.map(str::trim).filter(|p| !p.is_empty()) else {
        return Ok(None);
    };
    let path = expand_home(p);
    if !path.is_absolute() {
        return Err(format!("output_path “{p}” must be absolute."));
    }
    refuse_canvas_file(tool, &path)?;
    Ok(Some(path))
}

/// Refuses `path`, a file an agent asked `tool` to write, when it is a canvas file: a `.figcraft`
/// or `.excalidraw` file, or a copy Sawit keeps of one ([`sawit_board::ops::is_canvas_file`]).
/// While the app is closed nothing else holds the board, so one mistaken path would replace it
/// with a screenshot, a table or a dump (ruling P7-X3, which the canvas's own export commands
/// keep too). Only the name counts: `canvas.figcraft.png` is an image, and is written.
pub(crate) fn refuse_canvas_file(tool: &str, path: &Path) -> Result<(), String> {
    if sawit_board::ops::is_canvas_file(path) {
        return Err(format!(
            "{tool} would overwrite a canvas file: {}",
            path.display()
        ));
    }
    Ok(())
}

/// `path` as an agent means it: trimmed, with `~` for the home folder, and a relative path taken
/// from the folder the agent runs in (sawit-mcp's), not from the app's, whose folder is another.
pub(crate) fn agent_path(path: &str) -> PathBuf {
    let path = expand_home(path.trim());
    match std::env::current_dir() {
        Ok(cwd) if path.is_relative() => cwd.join(path),
        _ => path,
    }
}

pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ if path == "~" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default(),
        _ => PathBuf::from(path),
    }
}

/// The project a tool's files belong to: the `project` argument, else the
/// folder the agent started `sawit-mcp` in (its project), unless that is
/// `/` or the home folder. The canvas, and the diagrams and exports written
/// with no path, go by the project that holds it ([`agent_project`]).
pub(crate) fn project_dir(project: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = project.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(expand_home(p));
    }
    let cwd = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    (cwd.parent().is_some() && Some(&cwd) != home.as_ref()).then_some(cwd)
}

/// The project whose canvas the canvas tools use: the `project` argument, else the project that
/// holds the folder the agent started `sawit-mcp` in ([`project_dir`], [`project_holding`]). The
/// app takes an agent's folder to mean the project of its window that holds it, so an agent
/// started in a subfolder (`web/` of a repository) reaches the canvas the app shows for that
/// project with the app closed too, rather than a canvas of the subfolder's own that the app never
/// shows (Q12). The diagrams and canvas exports an agent writes with no path go to its `.nyawit/`
/// too, so that sawit-mcp makes no `.nyawit/` of a subfolder's own.
pub(crate) fn agent_project(project: Option<&str>) -> Option<PathBuf> {
    let named = project.is_some_and(|p| !p.trim().is_empty());
    let folder = project_dir(project)?;
    if named {
        return Some(folder);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Some(project_holding(&folder, home.as_deref()))
}

/// The project that holds `folder`, from what is on disk, as the app would have it open: within
/// the git repository `folder` is in (the nearest folder from it up with `.git`), the nearest
/// folder whose `.nyawit/` holds a canvas (`canvas.figcraft`, or the old `canvas.excalidraw`),
/// which the app made for a project it opened (a package of the repository opened as a project of
/// its own, say); else the repository. Outside a repository, `folder` itself.
///
/// A `.nyawit/` with no canvas in it decides nothing: sawit-mcp made one for the diagrams an agent
/// rendered in `shop/web`, which then took the agent's canvas too, one the app never shows with
/// `shop` open. Nor does a canvas above the repository: an agent once run in `~/code` left one
/// there, which every repository under it with no canvas of its own then shared. The home folder
/// and `/` are never one, nor is a folder above them, as [`project_dir`] has it.
fn project_holding(folder: &Path, home: Option<&Path>) -> PathBuf {
    let below_home = |dir: &&Path| dir.parent().is_some() && Some(*dir) != home;
    let up: Vec<&Path> = folder.ancestors().take_while(below_home).collect();
    let Some(repository) = up.iter().position(|dir| dir.join(".git").exists()) else {
        return folder.to_path_buf();
    };
    let holds_a_canvas = |dir: &Path| {
        sawit_core::nyawit::board_file(dir).exists()
            || sawit_core::nyawit::legacy_canvas_file(dir).exists()
    };
    let mut within = up[..=repository].iter().copied();
    within
        .find(|dir| holds_a_canvas(dir))
        .unwrap_or(up[repository])
        .to_path_buf()
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

/// The tools that change nothing: they read files, databases, the app or the network, or change
/// only what the Sawit window shows (a panel, a tab, the layout, the browser page's scroll or
/// size). MCP's `readOnlyHint`, which a client may act on by letting them through ([`hints`]).
const READ_ONLY: &[&str] = &[
    "list_instances",
    "instance_logs",
    "check_port",
    "list_postgres_binaries",
    "list_connections",
    "detect_databases",
    "list_databases",
    "describe_schema",
    "describe_table",
    "query",
    "list_ports",
    "diagnose_network",
    "ping",
    "list_ssh_hosts",
    "probe_ssh_host",
    "list_scripts",
    "git_status",
    "git_log",
    "git_diff",
    "git_show",
    "git_branches",
    "list_prompts",
    "get_prompt",
    "search_files",
    "list_dev_tools",
    "list_mcp_servers",
    "app_state",
    "app_open",
    "app_focus",
    "app_show_panel",
    "app_layout",
    "app_maximize_terminal",
    "app_open_page",
    "app_activate_tab",
    "app_run_list",
    "app_open_table",
    "app_network_diagnose",
    "app_open_diagram",
    "app_open_prompt",
    "terminal_read",
    "canvas_get",
    "canvas_commands",
    "browser_snapshot",
    "browser_hover",
    "browser_scroll",
    "browser_wait_for",
    "browser_console",
    "browser_network",
    "browser_resize",
    "browser_inspect",
    "browser_design",
    "keymap_list",
];

/// The tools that can delete or replace what was there before the call (a database, rows, a file
/// at a path given, a setting, the canvas, a running process, the working tree's files and what
/// was staged, the page the browser shows), or run what they can't bound: SQL, a shell command,
/// a script, JavaScript, an install script. MCP's `destructiveHint`, which a client may act on by
/// asking first. Every other tool that isn't [`READ_ONLY`] only adds.
const DESTRUCTIVE: &[&str] = &[
    "stop_instance",
    "restart_instance",
    "remove_instance",
    "drop_database",
    "execute_sql",
    "save_connection",
    "remove_connection",
    "write_env",
    "export_table",
    "export_query",
    "backup_database",
    "drop_relation",
    "truncate_table",
    "render_mermaid",
    "schema_diagram",
    "stop_processes",
    "update_ssh_host",
    "remove_ssh_host",
    "ssh_exec",
    "save_command",
    "remove_command",
    "git_unstage",
    "git_commit",
    "git_switch",
    "update_prompt",
    "delete_prompt",
    "install_dev_tools",
    "upgrade_dev_tools",
    "install_mcp_server",
    "uninstall_mcp_server",
    "app_close_tab",
    "app_run_script",
    "app_stop_script",
    "app_open_sql",
    "app_instance",
    "terminal_run",
    "terminal_input",
    "terminal_close",
    "canvas_update",
    "canvas_delete",
    "canvas_clear",
    "canvas_load",
    "canvas_export",
    "canvas_command",
    "browser_navigate",
    "browser_click",
    "browser_type",
    "browser_press_key",
    "browser_select_option",
    "browser_screenshot",
    "browser_evaluate",
    "browser_annotations",
    "browser_export_design",
    "terminal_split",
    "keymap_bind",
    "keymap_unbind",
];

/// The tools that change things but only add: a database, an instance, a prompt, rows, shapes on
/// the canvas, a saved SSH host, the working tree's changes to what is staged; or start something
/// (the app, an instance, a session). Not read-only, and not destructive ([`hints`]). Every tool
/// is in one of [`READ_ONLY`], [`DESTRUCTIVE`] and this list, so a new one has to be placed.
const ADDS: &[&str] = &[
    "create_instance",
    "start_instance",
    "create_database",
    "import_csv",
    "refresh_materialized_view",
    "add_ssh_host",
    "git_stage",
    "create_prompt",
    "fill_prompt",
    "launch_app",
    "app_ssh_connect",
    "canvas_add_elements",
    "canvas_add_mermaid",
];

/// The tools that reach past this Mac's own state: the internet, a remote host, a database that
/// may be a server elsewhere (a saved connection), a web page, or whatever a command they run
/// reaches. MCP's `openWorldHint`; the others stay on this Mac.
const OPEN_WORLD: &[&str] = &[
    "create_instance",
    "detect_databases",
    "list_databases",
    "create_database",
    "drop_database",
    "describe_schema",
    "describe_table",
    "query",
    "execute_sql",
    "export_table",
    "export_query",
    "backup_database",
    "import_csv",
    "drop_relation",
    "truncate_table",
    "refresh_materialized_view",
    "schema_diagram",
    "diagnose_network",
    "ping",
    "probe_ssh_host",
    "ssh_exec",
    "list_dev_tools",
    "install_dev_tools",
    "upgrade_dev_tools",
    "app_run_script",
    "app_ssh_connect",
    "app_open_table",
    "app_open_sql",
    "app_network_diagnose",
    "app_open_diagram",
    "terminal_run",
    "terminal_input",
    "canvas_command",
    "browser_navigate",
    "browser_snapshot",
    "browser_click",
    "browser_hover",
    "browser_type",
    "browser_press_key",
    "browser_select_option",
    "browser_scroll",
    "browser_wait_for",
    "browser_screenshot",
    "browser_console",
    "browser_network",
    "browser_evaluate",
    "browser_resize",
    "browser_inspect",
    "browser_design",
    "browser_export_design",
    "terminal_split",
];

/// MCP's hints for the tool `name`, from [`READ_ONLY`], [`DESTRUCTIVE`], [`ADDS`] and
/// [`OPEN_WORLD`] (MCP-15). A tool that only reads says nothing of destroying, which MCP reads only
/// of one that doesn't; one in no list counts as destructive, as MCP's own default has it.
fn hints(name: &str) -> rmcp::model::ToolAnnotations {
    let read_only = READ_ONLY.contains(&name);
    let destructive = DESTRUCTIVE.contains(&name) || !ADDS.contains(&name);
    rmcp::model::ToolAnnotations::from_raw(
        None,
        Some(read_only),
        (!read_only).then_some(destructive),
        None,
        Some(OPEN_WORLD.contains(&name)),
    )
}

impl Sawit {
    /// A server for the Sawit data folder `paths`.
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            tool_router: Self::router(),
        }
    }

    /// Every tool, from each area's router, each with the hints that say what it does to the
    /// world ([`hints`]).
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
            + Self::network_router()
            + Self::ssh_router()
            + Self::scripts_router()
            + Self::git_router()
            + Self::prompts_router()
            + Self::files_router()
            + Self::dev_tools_router()
            + Self::mcp_servers_router()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_is_routed() {
        let mut names: Vec<String> = Sawit::router()
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect();
        names.sort();
        let mut expected = vec![
            // instances
            "list_instances",
            "create_instance",
            "start_instance",
            "stop_instance",
            "restart_instance",
            "remove_instance",
            "instance_logs",
            "check_port",
            "list_postgres_binaries",
            // databases
            "list_connections",
            "detect_databases",
            "list_databases",
            "create_database",
            "drop_database",
            "describe_schema",
            "describe_table",
            "query",
            "execute_sql",
            "save_connection",
            "remove_connection",
            "write_env",
            "export_table",
            "export_query",
            "backup_database",
            "import_csv",
            "drop_relation",
            "truncate_table",
            "refresh_materialized_view",
            // diagrams
            "render_mermaid",
            "schema_diagram",
            // ports
            "list_ports",
            "stop_processes",
            // network
            "diagnose_network",
            "ping",
            // ssh
            "list_ssh_hosts",
            "add_ssh_host",
            "update_ssh_host",
            "remove_ssh_host",
            "probe_ssh_host",
            "ssh_exec",
            // run
            "list_scripts",
            "save_command",
            "remove_command",
            // git
            "git_status",
            "git_log",
            "git_diff",
            "git_show",
            "git_branches",
            "git_stage",
            "git_unstage",
            "git_commit",
            "git_switch",
            // prompts
            "list_prompts",
            "get_prompt",
            "create_prompt",
            "update_prompt",
            "delete_prompt",
            "fill_prompt",
            // files
            "search_files",
            // dev tools
            "list_dev_tools",
            "install_dev_tools",
            "upgrade_dev_tools",
            // mcp servers
            "list_mcp_servers",
            "install_mcp_server",
            "uninstall_mcp_server",
            // app control
            "launch_app",
            "app_state",
            "app_open",
            "app_focus",
            "app_show_panel",
            "app_layout",
            "app_maximize_terminal",
            "app_open_page",
            "app_activate_tab",
            "app_close_tab",
            "app_run_list",
            "app_run_script",
            "app_stop_script",
            "app_ssh_connect",
            "app_open_table",
            "app_open_sql",
            "app_instance",
            "app_network_diagnose",
            "app_open_diagram",
            "app_open_prompt",
            "terminal_run",
            "terminal_read",
            "terminal_input",
            "terminal_close",
            "canvas_get",
            "canvas_add_elements",
            "canvas_add_mermaid",
            "canvas_update",
            "canvas_delete",
            "canvas_clear",
            "canvas_load",
            "canvas_export",
            "canvas_command",
            "canvas_commands",
            // browser
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_hover",
            "browser_type",
            "browser_press_key",
            "browser_select_option",
            "browser_scroll",
            "browser_wait_for",
            "browser_screenshot",
            "browser_console",
            "browser_network",
            "browser_evaluate",
            "browser_resize",
            "browser_annotations",
            "browser_inspect",
            "browser_design",
            "browser_export_design",
            // terminal splits
            "terminal_split",
            // keymap
            "keymap_list",
            "keymap_bind",
            "keymap_unbind",
        ];
        expected.sort();
        assert_eq!(names, expected);
        for tool in Sawit::router().list_all() {
            assert!(
                tool.description.as_deref().is_some_and(|d| !d.is_empty()),
                "{} has no description",
                tool.name
            );
        }
    }

    /// README and the sawit.dev site say how many tools agents get: every count they give is the
    /// router's, so adding or removing a tool shows each place that counts them. The site said 110
    /// while the server had 117 (SITE-5).
    #[test]
    fn the_readme_and_the_site_give_the_tool_count() {
        let tools = Sawit::router().list_all().len();
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        // "117 tools", and the site's stat: "Tools for agents</dt>", then "<dd>117</dd>".
        let counts = |text: &str| -> Vec<usize> {
            let mut found = Vec::new();
            for (at, _) in text.match_indices(" tools") {
                let digits: String = text[..at]
                    .chars()
                    .rev()
                    .take_while(char::is_ascii_digit)
                    .collect();
                if !digits.is_empty() {
                    found.push(digits.chars().rev().collect::<String>().parse().unwrap());
                }
            }
            for (at, _) in text.match_indices("Tools for agents</dt>") {
                let rest = &text[at..];
                let value = rest.find("<dd>").expect("the stat's value") + "<dd>".len();
                let digits: String = rest[value..]
                    .chars()
                    .take_while(char::is_ascii_digit)
                    .collect();
                found.push(digits.parse().expect("the stat is a number"));
            }
            found
        };
        for file in ["README.md", "site/src/routes/index.tsx"] {
            let text = std::fs::read_to_string(repo.join(file))
                .unwrap_or_else(|e| panic!("{file} can't be read: {e}"));
            let found = counts(&text);
            assert!(found.len() >= 2, "{file} gives the tool count {found:?}");
            assert!(
                found.iter().all(|&n| n == tools),
                "{file} counts {found:?} tools; sawit-mcp has {tools}"
            );
        }
    }

    /// MCP-15: every tool carries MCP's hints for the clients that act on them (letting a
    /// read-only tool through, asking before a destructive one): whether it only reads, whether
    /// it can destroy what was there, and whether it reaches past this Mac. No tool had any, so a
    /// client told `query` from `execute_sql`, or `canvas_get` from `canvas_clear`, only by prose.
    #[test]
    fn every_tool_says_what_it_does_to_the_world() {
        let tools = Sawit::router().list_all();
        let unsaid: Vec<&str> = tools
            .iter()
            .filter(|tool| {
                tool.annotations.as_ref().is_none_or(|hints| {
                    hints.read_only_hint.is_none()
                        || hints.open_world_hint.is_none()
                        || (hints.read_only_hint == Some(false) && hints.destructive_hint.is_none())
                })
            })
            .map(|tool| tool.name.as_ref())
            .collect();
        assert!(unsaid.is_empty(), "these tools say nothing: {unsaid:?}");

        let hints = |name: &str| {
            let tool = tools.iter().find(|tool| tool.name == name);
            let tool = tool.unwrap_or_else(|| panic!("no tool {name}"));
            let sent = serde_json::to_value(tool).unwrap()["annotations"].clone();
            (
                sent["readOnlyHint"].as_bool(),
                sent["destructiveHint"].as_bool(),
                sent["openWorldHint"].as_bool(),
            )
        };
        for name in [
            "query",
            "describe_table",
            "canvas_get",
            "git_status",
            "list_dev_tools",
            "app_state",
            "browser_snapshot",
        ] {
            assert_eq!(hints(name).0, Some(true), "{name} only reads");
        }
        for name in [
            "execute_sql",
            "drop_database",
            "truncate_table",
            "remove_instance",
            "canvas_clear",
            "canvas_delete",
            "stop_processes",
            "git_commit",
            "uninstall_mcp_server",
            "install_dev_tools",
            "terminal_run",
            "browser_evaluate",
            // They replace what was there: the working tree's files, what was staged, the page.
            "git_switch",
            "git_unstage",
            "browser_navigate",
        ] {
            assert_eq!(hints(name).0, Some(false), "{name} changes things");
            assert_eq!(hints(name).1, Some(true), "{name} can destroy");
        }
        for name in [
            "canvas_add_elements",
            "create_prompt",
            "create_database",
            "git_stage",
            "launch_app",
        ] {
            assert_eq!(hints(name).0, Some(false), "{name} changes things");
            assert_eq!(hints(name).1, Some(false), "{name} only adds");
        }
        for name in [
            "query",
            "ping",
            "ssh_exec",
            "install_dev_tools",
            "browser_navigate",
        ] {
            assert_eq!(hints(name).2, Some(true), "{name} reaches past this Mac");
        }
        for name in ["canvas_get", "git_status", "list_prompts", "search_files"] {
            assert_eq!(hints(name).2, Some(false), "{name} stays on this Mac");
        }
    }

    /// Every tool is placed in exactly one of [`READ_ONLY`], [`DESTRUCTIVE`] and [`ADDS`], so a
    /// tool added without its hints fails here; and every name in the lists, [`OPEN_WORLD`]'s
    /// too, is a tool there is.
    #[test]
    fn the_hint_lists_place_every_tool_once() {
        let names: Vec<String> = Sawit::router()
            .list_all()
            .iter()
            .map(|tool| tool.name.to_string())
            .collect();
        for name in &names {
            let placed = [READ_ONLY, DESTRUCTIVE, ADDS]
                .iter()
                .filter(|list| list.contains(&name.as_str()))
                .count();
            assert_eq!(placed, 1, "{name} is in {placed} of the lists");
        }
        for name in READ_ONLY
            .iter()
            .chain(DESTRUCTIVE)
            .chain(ADDS)
            .chain(OPEN_WORLD)
        {
            assert!(names.iter().any(|n| n == name), "no tool {name}");
        }
    }

    #[test]
    fn server_info_has_instructions() {
        let sawit = Sawit::new(AppPaths {
            root: std::env::temp_dir(),
        });
        let info = sawit.get_info();
        assert!(info.capabilities.tools.is_some());
        let instructions = info.instructions.as_deref().unwrap();
        assert!(instructions.contains("app_*"));
        // The engine's commands are reachable, and an agent is told where to find their ids.
        assert!(instructions.contains("canvas_command"), "{instructions}");
        assert!(instructions.contains("canvas_commands"), "{instructions}");
    }

    /// The canvas the tools draw on is the native one, and what an agent is sent says so. Nothing
    /// in it names the web canvas that this one replaced: not a tool's description, not the
    /// description of one of its arguments (those are in the input schema), and not the
    /// instructions. The canvas tools speak of that canvas's format only as the extension of the
    /// files they take: `canvas_load` imports one, and every canvas tool's `file` may name one.
    #[test]
    fn canvas_tools_describe_the_native_canvas() {
        const OLD_NAME: &str = "excalidraw";
        // Each tool as `tools/list` sends it: its name, description and input schema.
        let sent: Vec<(String, String)> = Sawit::router()
            .list_all()
            .iter()
            .map(|tool| (tool.name.to_string(), serde_json::to_string(tool).unwrap()))
            .collect();
        let tool = |name: &str| {
            let found = sent.iter().find(|(candidate, _)| candidate == name);
            found.map_or_else(|| panic!("no tool {name}"), |(_, text)| text.as_str())
        };

        let mut stale = Vec::new();
        for (name, text) in &sent {
            // The canvas tools take `.excalidraw` files: that extension is a file's, not a name.
            let named = match name.starts_with("canvas_") {
                true => text.replace(".excalidraw", ""),
                false => text.clone(),
            };
            if named.to_lowercase().contains(OLD_NAME) {
                stale.push(format!("{name}: {text}"));
            }
        }
        assert!(
            stale.is_empty(),
            "these still name the web canvas: {stale:#?}"
        );
        assert!(
            !INSTRUCTIONS.to_lowercase().contains(OLD_NAME),
            "the instructions still name the web canvas: {INSTRUCTIONS}"
        );

        // What the tools say instead.
        let page = tool("app_open_page");
        assert!(
            page.contains("canvas (whiteboard and design canvas)"),
            "{page}"
        );
        let elements = tool("canvas_add_elements");
        assert!(elements.contains("Element skeletons, e.g."), "{elements}");
        assert!(
            elements
                .contains("Types: rectangle, ellipse, diamond, sticky, text, arrow, line, frame"),
            "{elements}"
        );
        // It says what a skeleton with no colour comes in (Plan 8 final review P-2): the colours
        // a board draws it in, as `get` reads them back, and connectors that run straight
        // (sawit-board's skeleton tests pin the routing itself).
        let mut board = sawit_board::BoardDoc::new_board("Canvas").unwrap();
        let bare = serde_json::json!([
            {"type": "rectangle", "x": 0, "y": 0},
            {"type": "sticky", "x": 300, "y": 0},
            {"type": "line", "x": 0, "y": 300, "points": [[0, 0], [50, 40], [100, 0]]},
        ]);
        let added = sawit_board::ops::run(
            &mut board,
            &serde_json::json!({"op": "add", "elements": bare}),
        );
        assert_eq!(added["ok"], true, "{added}");
        let drawn = sawit_board::ops::run(&mut board, &serde_json::json!({"op": "get"}));
        let colour = |n: usize| drawn["elements"][n]["color"].as_str().unwrap().to_string();
        let (shape, sticky, ink) = (colour(0), colour(1), colour(2));
        assert_eq!(drawn["elements"][2]["type"], "ink", "{drawn}");
        assert_eq!(shape, sticky);
        for said in [
            "Shapes and stickies with no backgroundColor come in sticky yellow",
            &format!("shapes and stickies are sticky yellow ({shape})"),
            "pass backgroundColor for another",
            "for no fill",
            &format!(
                "A line through more than two points is drawn as ink, {ink} unless strokeColor"
            ),
            "Arrows and lines run straight",
        ] {
            assert!(elements.contains(said), "no {said:?} in {elements}");
        }
        // The canvas tab opens only in the app. With the app closed a clear is saved at once, so
        // nothing undoes it, and the save keeps the board it cleared beside the file (m1), and the
        // ones before it (CANVAS-CORE-3). So does every op that deletes layers, a deletion by id
        // and an engine command that deletes included, whatever is left on the canvas, in
        // backups of their own, which never push out a clear's.
        assert!(
            elements.contains("Opens the canvas tab when the app is running"),
            "{elements}"
        );
        let clear = tool("canvas_clear");
        assert!(
            clear.contains("The board it held is kept beside the canvas file as <file>.bak (canvas.figcraft.bak for the project's canvas, home.figcraft.bak for home.figcraft)")
                && clear.contains("the 4 kept before it moving to .bak.1 to .bak.4")
                && clear.contains("never a deletion")
                && clear.contains("canvas_load brings it back")
                && clear.contains("in the app the user can also undo it"),
            "{clear}"
        );
        let delete = tool("canvas_delete");
        assert!(
            delete.contains(
                "The board they were on is first kept beside the canvas file as <file>.del.bak (canvas.figcraft.del.bak for the project's canvas, home.figcraft.del.bak for home.figcraft)"
            ) && delete.contains("the 4 kept before it moving to .del.bak.1 to .del.bak.4")
                && delete.contains("so they never push those out")
                && delete.contains("canvas_load brings any of them back")
                && delete.contains("in the app the user can also undo it"),
            "{delete}"
        );
        let command = tool("canvas_command");
        assert!(
            command.contains("A command that deletes layers (edit.delete, edit.cut, page.delete, ...) keeps the board they were on beside the canvas file as <file>.del.bak first"),
            "{command}"
        );
        // A backup is named one way everywhere, arguments included: the canvas file's whole name,
        // then what Sawit adds (`<file>.bak`). `<name>` meant the file's stem in one place and its
        // whole name in another.
        for (name, text) in sent.iter().filter(|(name, _)| name.starts_with("canvas_")) {
            assert!(!text.contains("<name>"), "{name}: {text}");
        }
        let load = tool("canvas_load");
        assert!(
            load.contains(
                "A .figcraft file to load, or an .excalidraw file to import (replaces the board)"
            ),
            "{load}"
        );
        // The canvas tools work with the app closed, so launching it is not a step they need.
        let launch = tool("launch_app");
        assert!(
            launch.contains("canvas_* tools also work on the saved canvas file without it"),
            "{launch}"
        );

        // The canvas_* line is the native canvas's, and it works on the file with the app closed.
        let instruction = |start: &str| {
            let found = INSTRUCTIONS.lines().find(|line| line.starts_with(start));
            found.unwrap_or_else(|| panic!("no instruction line {start}"))
        };
        let canvas = instruction("- canvas_*:");
        assert!(canvas.contains("the canvas (FigCraft engine)"), "{canvas}");
        assert!(
            canvas.contains("saved file") && canvas.contains("when the app is closed"),
            "{canvas}"
        );

        // The list of commands is too long to return whole with their parameters (a tool's answer
        // is cut at 100 KB), and every place that names the list says the parameters come with a
        // filter.
        for (what, text) in [
            ("canvas_command", tool("canvas_command")),
            ("canvas_commands", tool("canvas_commands")),
            ("the canvas_command line", instruction("- canvas_command:")),
        ] {
            assert!(
                text.contains("filter"),
                "{what} doesn't mention the filter: {text}"
            );
        }
    }

    /// canvas_get, canvas_clear and canvas_export act on the page on show, and say so: they said
    /// "the canvas", or "the whole canvas" for a clear that leaves the other pages as they are.
    /// canvas_get says where the other pages and the nested layers are.
    #[test]
    fn canvas_tools_say_which_page_they_act_on() {
        let tools = Sawit::router().list_all();
        let said = |name: &str| {
            let tool = tools.iter().find(|tool| tool.name == name).expect(name);
            tool.description.as_deref().unwrap_or_default().to_string()
        };
        let get = said("canvas_get");
        for part in [
            "top-level layers of the canvas's page on show",
            "page.list",
            "node.get",
            "page.select",
        ] {
            assert!(get.contains(part), "no {part:?} in {get}");
        }
        let clear = said("canvas_clear");
        assert!(
            clear.contains("page on show; other pages keep theirs"),
            "{clear}"
        );
        let export = said("canvas_export");
        assert!(
            export.contains("page on show") && export.contains("file.export"),
            "{export}"
        );
        for name in ["canvas_get", "canvas_clear", "canvas_export"] {
            assert!(!said(name).contains("whole canvas"), "{name}");
        }
    }

    /// What an agent is told matches what the tools do:
    /// - MCP-12: with the app closed every canvas_command opens the saved file afresh, so nothing
    ///   carries over from one call to the next: edit.undo has nothing to undo, and a command that
    ///   acts on the selection has none unless it is given ids or runs in one command.batch.
    /// - MCP-13: the browser_* tools need the app, as the app_* and terminal_* tools do.
    /// - MCP-14: app_show_panel's `shell_config` is the panel the app calls Config Files.
    #[test]
    fn agents_are_told_what_needs_the_app_and_what_a_closed_one_keeps() {
        let sent: Vec<(String, String)> = Sawit::router()
            .list_all()
            .iter()
            .map(|tool| (tool.name.to_string(), serde_json::to_string(tool).unwrap()))
            .collect();
        let tool = |name: &str| {
            let found = sent.iter().find(|(candidate, _)| candidate == name);
            found.map_or_else(|| panic!("no tool {name}"), |(_, text)| text.clone())
        };
        let line = INSTRUCTIONS
            .lines()
            .find(|line| line.starts_with("- canvas_command:"))
            .expect("a canvas_command line");
        for (what, text) in [
            ("canvas_command", tool("canvas_command")),
            ("the canvas_command line", line.to_string()),
        ] {
            for said in [
                "With the app closed each call starts from the saved file",
                "no undo",
                "pass ids",
                "command.batch",
            ] {
                assert!(text.contains(said), "{what} doesn't say {said:?}: {text}");
            }
        }
        let launch = tool("launch_app");
        assert!(
            launch.contains("The app_*, terminal_* and browser_* tools need it running"),
            "{launch}"
        );
        // The keymap's defaults are bound in the app, and a split is the app's terminal's.
        assert!(
            launch.contains("The keymap_* tools and terminal_split need it running too"),
            "{launch}"
        );
        let keymap = INSTRUCTIONS
            .lines()
            .find(|line| line.starts_with("- keymap_*:"))
            .expect("a keymap_* line");
        assert!(keymap.contains("They need the app"), "{keymap}");
        for name in ["keymap_list", "keymap_bind", "keymap_unbind"] {
            assert!(keymap.contains(name), "no {name} in {keymap}");
            let said = tool(name).to_lowercase();
            assert!(said.contains("needs the app running"), "{said}");
        }
        let panel = tool("app_show_panel");
        assert!(
            panel.contains("shell_config (the Config Files panel)"),
            "{panel}"
        );
    }

    /// Q12: an agent that names no project gets its own project's canvas, as it gets its own
    /// project's browser, and the instructions say so: they said it of the browser alone, while
    /// the canvas tools used the project in front.
    #[test]
    fn agents_are_told_the_canvas_is_their_own_projects() {
        let line = INSTRUCTIONS
            .lines()
            .find(|line| line.starts_with("Most app tools take `project`"))
            .expect("the line on `project`");
        for said in [
            "the window that has the folder sawit-mcp runs in open",
            "The browser_* and canvas_* tools (and app_open_page browser or canvas) use that project's own browser tab and canvas",
        ] {
            assert!(line.contains(said), "no {said:?} in {line}");
        }
        let tools = Sawit::router().list_all();
        for tool in tools.iter().filter(|tool| tool.name.starts_with("canvas_")) {
            let properties = tool.input_schema.get("properties").cloned();
            let project = properties.unwrap_or_default()["project"]["description"].clone();
            let project = project.as_str().unwrap_or_default();
            assert!(
                project.contains("Default: the project your working folder is in"),
                "{}: {project}",
                tool.name
            );
        }
    }

    /// Every canvas tool takes `file`, a canvas file to work on instead of the project's board (a
    /// design file, say), and the canvas_* line says so: the tools reached the project's canvas
    /// alone, though the app opens any `.figcraft` file in a canvas tab of its own.
    #[test]
    fn every_canvas_tool_takes_a_file_and_the_instructions_say_so() {
        let tools = Sawit::router().list_all();
        let canvas_tools = tools.iter().filter(|tool| tool.name.starts_with("canvas_"));
        let without: Vec<&str> = canvas_tools
            .filter(|tool| {
                let properties = tool.input_schema.get("properties");
                properties.is_none_or(|p| p.get("file").is_none())
            })
            .map(|tool| tool.name.as_ref())
            .collect();
        assert!(without.is_empty(), "these take no file: {without:?}");
        let line = INSTRUCTIONS
            .lines()
            .find(|line| line.starts_with("- canvas_*:"))
            .expect("a canvas_* line");
        for said in [
            "your project's canvas (.nyawit/canvas.figcraft)",
            "the .figcraft file named by `file`",
            "list, clear and export SVG/PNG the page on show",
            "update and delete layers by id, on any page",
            "load another canvas in place of every page",
            "keep what they clear, load over or delete in backups beside it (.bak, .del.bak)",
        ] {
            assert!(line.contains(said), "no {said:?} in {line}");
        }
    }

    /// The terminal_* line says what the tools do now that a terminal tab can hold split panes
    /// (terminal-panes review m2): it names each terminal tool that takes `pane`, and where the
    /// pane's number comes from, instead of saying the tools close tabs.
    #[test]
    fn the_terminal_line_tells_tabs_from_panes() {
        let line = INSTRUCTIONS
            .lines()
            .find(|line| line.starts_with("- terminal_*:"))
            .expect("a terminal_* line");
        let mut take_a_pane: Vec<String> = Sawit::router()
            .list_all()
            .iter()
            .filter(|tool| tool.name.starts_with("terminal_"))
            .filter(|tool| {
                let properties = tool.input_schema.get("properties");
                properties.is_some_and(|p| p.get("pane").is_some())
            })
            .map(|tool| tool.name.to_string())
            .collect();
        take_a_pane.sort();
        assert_eq!(
            take_a_pane,
            [
                "terminal_close",
                "terminal_input",
                "terminal_read",
                "terminal_split"
            ]
        );
        for name in &take_a_pane {
            assert!(line.contains(name.as_str()), "no {name} in {line}");
        }
        assert!(
            line.contains("`pane`") && line.contains("app_state"),
            "{line}"
        );
        for name in ["terminal_run", "app_run_script", "app_ssh_connect"] {
            assert!(line.contains(name), "no {name} in {line}");
        }
    }

    /// The tools that start something in a terminal say which tab and pane, and tell the agent to
    /// pass them on: the user may be typing in another terminal, which stays in front, so
    /// terminal_read and terminal_input without them would act on the user's terminal.
    #[test]
    fn tools_that_start_a_terminal_say_where() {
        let tools = Sawit::router().list_all();
        for name in [
            "terminal_run",
            "terminal_split",
            "app_run_script",
            "app_ssh_connect",
        ] {
            let tool = tools.iter().find(|t| t.name == name).expect(name);
            let said = tool.description.as_deref().unwrap_or_default();
            for word in ["pane", "terminal_read", "terminal_input"] {
                assert!(said.contains(word), "{name} doesn't mention {word}: {said}");
            }
        }
    }

    /// The tools that end what runs in a terminal or change the user's key bindings say so, and
    /// take the argument that confirms it: terminal_close ended a dev server, an SSH session or
    /// the user's own agent on a bare call, and a rewrite of keymap.json drops its comments.
    #[test]
    fn tools_that_end_a_program_or_rewrite_the_keymap_say_so() {
        let tools = Sawit::router().list_all();
        let tool = |name: &str| {
            let tool = tools.iter().find(|t| t.name == name).expect(name);
            let confirm = tool
                .input_schema
                .get("properties")
                .and_then(|p| p.get("confirm"));
            (
                tool.description.as_deref().unwrap_or_default().to_string(),
                confirm.is_some(),
            )
        };
        let (close, confirm) = tool("terminal_close");
        assert!(confirm, "terminal_close takes no confirm");
        for said in [
            "DESTRUCTIVE",
            "ending whatever runs there",
            "confirm is true",
        ] {
            assert!(
                close.contains(said),
                "terminal_close doesn't say {said:?}: {close}"
            );
        }
        for name in ["keymap_bind", "keymap_unbind"] {
            let (said, confirm) = tool(name);
            assert!(confirm, "{name} takes no confirm");
            for words in ["DESTRUCTIVE", "without comments", "confirm is true"] {
                assert!(said.contains(words), "{name} doesn't say {words:?}: {said}");
            }
        }
        let (list, _) = tool("keymap_list");
        assert!(list.contains("Read-only"), "{list}");
        let (split, _) = tool("terminal_split");
        for said in ["never takes the keyboard", "20 columns by 4 rows"] {
            assert!(
                split.contains(said),
                "terminal_split doesn't say {said:?}: {split}"
            );
        }
    }

    #[test]
    fn no_tool_may_write_a_canvas_file() {
        for name in [
            "/p/.nyawit/canvas.figcraft",
            "/p/Board.FigCraft",
            "/p/.nyawit/canvas.excalidraw",
            "/p/.nyawit/canvas.figcraft.bak",
            "/p/.nyawit/canvas.figcraft.bak.3",
            "/p/.nyawit/canvas.figcraft.del.bak",
            "/p/design/home.figcraft.del.bak.4",
            "/p/.nyawit/canvas.figcraft.tmp",
        ] {
            assert_eq!(
                refuse_canvas_file("render_mermaid", Path::new(name)),
                Err(format!(
                    "render_mermaid would overwrite a canvas file: {name}"
                ))
            );
        }
        for name in [
            "/p/page.png",
            "/p/canvas.figcraft.svg",
            "/p/.nyawit/rows.csv",
        ] {
            assert_eq!(refuse_canvas_file("export_query", Path::new(name)), Ok(()));
        }
    }

    /// Q12: with the app closed, a canvas tool that names no project draws on the canvas of the
    /// project that holds the agent's folder, as the app does: within the agent's repository, the
    /// nearest folder up whose `.nyawit/` holds a canvas, else the repository; outside one, the
    /// folder itself. An agent started in `shop/web` edited `shop/web/.nyawit/canvas.figcraft`,
    /// which the app never shows. A `.nyawit/` with no canvas in it (one sawit-mcp made for an
    /// agent's diagrams), or a canvas above the repository (left by an agent run in `~/code`),
    /// decides nothing: either took the canvas of every agent below it. The home folder is never
    /// the project, whatever it holds.
    #[test]
    fn the_agents_project_is_the_nearest_folder_up_that_is_one() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let code = home.join("code");
        let shop = code.join("shop");
        let web = shop.join("web");
        let src = web.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let project = |folder: &Path| project_holding(folder, Some(&home));
        let canvas_in = |folder: &Path, name: &str| {
            let nyawit = sawit_core::nyawit::dir(folder);
            std::fs::create_dir_all(&nyawit).unwrap();
            std::fs::write(nyawit.join(name), "").unwrap();
        };
        assert_eq!(project(&src), src, "nothing marks a project");

        std::fs::create_dir(shop.join(".git")).unwrap();
        assert_eq!(project(&src), shop, "the repository");
        assert_eq!(project(&shop), shop);
        // The `.nyawit/` sawit-mcp made in the agent's subfolder for its diagrams.
        std::fs::create_dir_all(sawit_core::nyawit::diagrams_dir(&web)).unwrap();
        assert_eq!(project(&src), shop, "a .nyawit/ without a canvas took it");
        // A canvas above the repository, which an agent once run in `~/code` left there: the
        // repositories under it keep their own.
        canvas_in(&code, "canvas.figcraft");
        assert_eq!(project(&src), shop, "a canvas above the repository took it");
        let blog = code.join("blog");
        std::fs::create_dir_all(blog.join(".git")).unwrap();
        assert_eq!(project(&blog), blog);
        // A folder of the repository the app has made a canvas for, a package opened as a project
        // of its own: its old canvas counts too.
        canvas_in(&web, "canvas.excalidraw");
        assert_eq!(project(&src), web);
        canvas_in(&shop, "canvas.figcraft");
        assert_eq!(project(&src), web, "the nearest");
        assert_eq!(project(&shop), shop);
        // A package with a repository of its own (a submodule's `.git` is a file) is its own.
        let api = shop.join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(api.join(".git"), "gitdir: ../.git/modules/api").unwrap();
        assert_eq!(project(&api.join("src")), api);
        // A file at `.nyawit`'s name holds no canvas.
        std::fs::remove_dir_all(sawit_core::nyawit::dir(&web)).unwrap();
        std::fs::write(sawit_core::nyawit::dir(&web), "").unwrap();
        assert_eq!(project(&src), shop);

        // The home folder isn't one, nor is anything above it.
        std::fs::create_dir(home.join(".nyawit")).unwrap();
        std::fs::create_dir(home.join(".git")).unwrap();
        let notes = home.join("notes");
        std::fs::create_dir(&notes).unwrap();
        assert_eq!(project(&notes), notes);
        std::fs::create_dir(dir.path().join(".git")).unwrap();
        assert_eq!(project(&notes), notes);
    }

    #[test]
    fn clip_cuts_on_char_boundary() {
        assert_eq!(clip("abc".into(), 10), "abc");
        let cut = clip("ééé".into(), 3);
        assert!(cut.starts_with("é\n"), "{cut}");
        assert!(cut.contains("2 of 6 bytes"));
    }

    #[test]
    fn utc_dates() {
        assert_eq!(utc_time(0), "1970-01-01 00:00");
        assert_eq!(utc_time(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(utc_time(951_782_400), "2000-02-29 00:00");
    }
}
