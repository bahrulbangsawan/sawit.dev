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
- terminal_*: run commands in Sawit's terminal and read their output, type keys (ctrl-c), close tabs or panes. terminal_run, app_run_script and app_ssh_connect return the terminal (tab) and `pane` they used: pass both to terminal_read, terminal_input and terminal_close, since the user may be typing in another terminal, which stays in front (app_state lists tabs and panes). terminal_input needs the tab, and a split tab needs its pane; terminal_read without a tab reads the pane the user has in front, and terminal_close without a pane closes the whole tab.
- canvas_*: the canvas (FigCraft engine): list, add shapes/stickies/arrows, Mermaid as editable shapes, update, delete, clear, load, export SVG/PNG. They also work on the saved file when the app is closed.
- canvas_command: run any command of the canvas engine by id with its parameters (board.sticky, node.set, align.left, edit.undo, …); canvas_commands lists the ids, and with a filter what each takes.
- browser_*: a browser tab (WebKit) to check the app being built: navigate (default the project's dev server), snapshot the page as an outline with element refs, click/hover/type/press keys/select/scroll by ref or CSS selector, wait for text, screenshot, read console errors and network requests, evaluate JS, resize to a device, read the user's annotations, inspect an element's design, read the page's design system, export it as DESIGN.md + tokens + assets. Take a new browser_snapshot after the page changes; refs reset.
Most app tools take `project` to pick the window that has that folder open; without it, the window that has the folder sawit-mcp runs in open is used, else the frontmost window.
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
/// `/` or the home folder. Canvas and diagram files default to its
/// `.nyawit/`.
pub(crate) fn project_dir(project: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = project.map(str::trim).filter(|p| !p.is_empty()) {
        return Some(expand_home(p));
    }
    let cwd = std::env::current_dir().ok()?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    (cwd.parent().is_some() && Some(&cwd) != home.as_ref()).then_some(cwd)
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
            + Self::browser_design_router()
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
        let instructions = info.instructions.as_deref().unwrap();
        assert!(instructions.contains("app_*"));
        // The engine's commands are reachable, and an agent is told where to find their ids.
        assert!(instructions.contains("canvas_command"), "{instructions}");
        assert!(instructions.contains("canvas_commands"), "{instructions}");
    }

    /// The canvas the tools draw on is the native one, and what an agent is sent says so. Nothing
    /// in it names the web canvas that this one replaced: not a tool's description, not the
    /// description of one of its arguments (those are in the input schema), and not the
    /// instructions. `canvas_load` alone speaks of that canvas's format, to import it, and only as
    /// the extension of the files it takes.
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
            // `canvas_load` takes `.excalidraw` files: that extension is a file's, not a name.
            let named = match name.as_str() {
                "canvas_load" => text.replace(".excalidraw", ""),
                _ => text.clone(),
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
        // ones before it (CANVAS-CORE-3).
        assert!(
            elements.contains("Opens the canvas tab when the app is running"),
            "{elements}"
        );
        let clear = tool("canvas_clear");
        assert!(
            clear.contains(
                "The board it held is kept beside the canvas file as canvas.figcraft.bak"
            ) && clear.contains("the 4 kept before it moving to .bak.1 to .bak.4")
                && clear.contains("in the app the user can also undo it"),
            "{clear}"
        );
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
            ["terminal_close", "terminal_input", "terminal_read"]
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
        for name in ["terminal_run", "app_run_script", "app_ssh_connect"] {
            let tool = tools.iter().find(|t| t.name == name).expect(name);
            let said = tool.description.as_deref().unwrap_or_default();
            for word in ["pane", "terminal_read", "terminal_input"] {
                assert!(said.contains(word), "{name} doesn't mention {word}: {said}");
            }
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
