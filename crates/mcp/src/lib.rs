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
pub mod db;
pub mod db_admin;
pub mod dev_tools;
pub mod diagrams;
pub mod files;
pub mod git;
pub mod instances;
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
- terminal_*: run commands in Sawit's terminal and read their output, type keys (ctrl-c), close tabs.
- canvas_*: the Excalidraw board: list, add shapes/arrows, Mermaid as editable shapes, update, delete, clear, load, export SVG/PNG.
- browser_*: a browser tab (WebKit) to check the app being built: navigate (default the project's dev server), snapshot the page as an outline with element refs, click/hover/type/press keys/select/scroll by ref or CSS selector, wait for text, screenshot, read console errors and network requests, evaluate JS. Take a new browser_snapshot after the page changes; refs reset.
Most app tools take `project` to pick the window that has that folder open; otherwise the frontmost window is used.
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

pub(crate) fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ if path == "~" => std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default(),
        _ => PathBuf::from(path),
    }
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
    pub fn new(paths: AppPaths) -> Self {
        Self {
            paths,
            tool_router: Self::router(),
        }
    }

    /// Every tool, from each area's router.
    pub fn router() -> ToolRouter<Sawit> {
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

    #[test]
    fn server_info_has_instructions() {
        let sawit = Sawit::new(AppPaths {
            root: std::env::temp_dir(),
        });
        let info = sawit.get_info();
        assert!(info.capabilities.tools.is_some());
        assert!(info.instructions.as_deref().unwrap().contains("app_*"));
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
