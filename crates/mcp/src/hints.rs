//! MCP's hints for every tool (MCP-15): whether it only reads, whether it can destroy what was
//! there, and whether it reaches past this Mac, which `Sawit::router` puts on each one.

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
    "local_explorer",
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
    "list_skills",
    "list_config_files",
    "stack_options",
    "stack_check",
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
    "canvas_design_guide",
    "canvas_get",
    "canvas_commands",
    "canvas_screen_get",
    "browser_snapshot",
    "browser_hover",
    "browser_scroll",
    "browser_wait_for",
    "browser_resize",
    "browser_inspect",
    "browser_design",
    "keymap_list",
];

/// The tools that can delete or replace what was there before the call (a database, rows, a file
/// at a path given, a setting, the canvas, a running process, the working tree's files and what
/// was staged, the page the browser shows, the browser's console and network logs, which
/// browser_console and browser_network empty when asked to), or run what they can't bound: SQL, a
/// shell command, a script, JavaScript, an install script. MCP's `destructiveHint`, which a client
/// may act on by asking first. Every other tool that isn't [`READ_ONLY`] only adds.
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
    "render_diagram",
    "render_mermaid",
    "schema_diagram",
    "stop_processes",
    "local_explorer_request",
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
    "uninstall_dev_tools",
    "install_mcp_server",
    "uninstall_mcp_server",
    "install_skill",
    "uninstall_skill",
    "install_sawit_skills",
    "stack_create",
    "app_close_tab",
    "app_run_script",
    "app_stop_script",
    "app_open_sql",
    "app_instance",
    "terminal_run",
    "terminal_input",
    "terminal_close",
    "terminal_split",
    "canvas_update",
    "canvas_delete",
    "canvas_clear",
    "canvas_load",
    "canvas_export",
    "canvas_command",
    "canvas_screen_write",
    "canvas_screen_export",
    "browser_navigate",
    "browser_click",
    "browser_type",
    "browser_press_key",
    "browser_select_option",
    "browser_screenshot",
    "browser_console",
    "browser_network",
    "browser_evaluate",
    "browser_annotations",
    "browser_mesurer",
    "browser_export_design",
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
    "canvas_add_diagram",
    "canvas_add_mermaid",
    "canvas_begin",
    "canvas_progress",
    "canvas_add_routes",
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
    "local_explorer_request",
    "probe_ssh_host",
    "ssh_exec",
    "list_dev_tools",
    "install_dev_tools",
    "upgrade_dev_tools",
    "install_skill",
    "stack_create",
    "app_run_script",
    "app_ssh_connect",
    "app_open_table",
    "app_open_sql",
    "app_network_diagnose",
    "app_open_diagram",
    "terminal_run",
    "terminal_input",
    "terminal_split",
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
    "browser_mesurer",
    "browser_export_design",
];

/// MCP's hints for the tool `name`, from [`READ_ONLY`], [`DESTRUCTIVE`], [`ADDS`] and
/// [`OPEN_WORLD`] (MCP-15). A tool that only reads says nothing of destroying, which MCP reads only
/// of one that doesn't; one in no list counts as destructive, as MCP's own default has it.
pub(super) fn hints(name: &str) -> rmcp::model::ToolAnnotations {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sawit;

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
            "list_config_files",
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
            "uninstall_dev_tools",
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
        for name in [
            "canvas_get",
            "git_status",
            "list_prompts",
            "search_files",
            "list_config_files",
            "uninstall_dev_tools",
        ] {
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
}
