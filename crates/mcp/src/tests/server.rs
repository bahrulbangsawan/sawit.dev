//! The server's tools as a whole: every one routed with a description, the diagram tools' names,
//! the tool count README and the site give, and what the server says when an agent connects, in
//! the room a client keeps for it.

use rmcp::ServerHandler;
use sawit_core::instances::AppPaths;

use crate::{INSTRUCTIONS, Sawit};

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
        "render_diagram",
        "render_mermaid",
        "schema_diagram",
        // ports
        "list_ports",
        "stop_processes",
        // cloudflare local explorer
        "local_explorer",
        "local_explorer_request",
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
        "uninstall_dev_tools",
        // mcp servers
        "list_mcp_servers",
        "install_mcp_server",
        "uninstall_mcp_server",
        // skills
        "list_skills",
        "install_skill",
        "uninstall_skill",
        "install_sawit_skills",
        // config files
        "list_config_files",
        // new project
        "stack_options",
        "stack_check",
        "stack_create",
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
        "terminal_split",
        "terminal_read",
        "terminal_input",
        "terminal_close",
        "canvas_design_guide",
        "canvas_get",
        "canvas_add_elements",
        "canvas_add_diagram",
        "canvas_add_mermaid",
        "canvas_update",
        "canvas_delete",
        "canvas_clear",
        "canvas_load",
        "canvas_export",
        "canvas_command",
        "canvas_commands",
        "canvas_begin",
        "canvas_progress",
        "canvas_screen_write",
        "canvas_screen_get",
        "canvas_screen_export",
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

/// The diagram tools are named for diagrams, and list every type Pele draws, each keyword a
/// source can start with; the old Mermaid names still work, say they are deprecated and name
/// the tool to call instead, with the same arguments and hints.
#[test]
fn the_diagram_tools_list_every_type_and_the_old_names_are_deprecated() {
    let tools = Sawit::router().list_all();
    let tool = |name: &str| {
        let tool = tools.iter().find(|tool| tool.name == name);
        tool.unwrap_or_else(|| panic!("no tool {name}")).clone()
    };
    for (new, old) in [
        ("render_diagram", "render_mermaid"),
        ("canvas_add_diagram", "canvas_add_mermaid"),
    ] {
        let (new, old) = (tool(new), tool(old));
        let said = new.description.as_deref().unwrap_or_default();
        assert!(!said.contains("Deprecated"), "{said}");
        assert!(said.contains("Pele"), "{said}");
        for (keyword, _) in sawit_core::mermaid::TYPES {
            let named = said
                .split([',', ' ', ':', '(', ')'])
                .any(|word| word == *keyword);
            assert!(named, "{} doesn't name {keyword}: {said}", new.name);
        }
        let old_said = old.description.as_deref().unwrap_or_default();
        assert!(old_said.starts_with("Deprecated"), "{old_said}");
        assert!(old_said.contains(new.name.as_ref()), "{old_said}");
        assert_eq!(old.input_schema, new.input_schema, "{}", old.name);
        assert_eq!(old.annotations, new.annotations, "{}", old.name);
    }
    assert!(INSTRUCTIONS.contains("render_diagram") && INSTRUCTIONS.contains("Pele"));
}

/// README and the sawit.dev site say how many tools agents get: every count they give is the
/// router's, so adding or removing a tool shows each place that counts them. The site said 110
/// while the server had 117 (SITE-5).
#[test]
fn the_readme_and_the_site_give_the_tool_count() {
    let tools = Sawit::router().list_all().len();
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // "134 tools", and the site's stat: "Tools for agents</dt>", then "<dd>134</dd>".
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
    // The site's home page: its route, and the sections it is made of in components/home.
    let home = repo.join("site/src/components/home");
    let mut sections: Vec<std::path::PathBuf> = std::fs::read_dir(&home)
        .unwrap_or_else(|e| panic!("{} can't be read: {e}", home.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tsx"))
        .collect();
    sections.sort();
    let site = [vec![repo.join("site/src/routes/index.tsx")], sections].concat();
    for (place, files) in [
        ("README.md", vec![repo.join("README.md")]),
        ("the site's home page", site),
    ] {
        let text = files
            .iter()
            .map(|file| {
                std::fs::read_to_string(file)
                    .unwrap_or_else(|e| panic!("{} can't be read: {e}", file.display()))
            })
            .collect::<Vec<_>>()
            .join("\n");
        let found = counts(&text);
        assert!(found.len() >= 2, "{place} gives the tool count {found:?}");
        assert!(
            found.iter().all(|&n| n == tools),
            "{place} counts {found:?} tools; sawit-mcp has {tools}"
        );
    }
}

/// Claude Code keeps only the first 4,096 characters of an MCP server's instructions and drops the
/// rest unseen: a session's copy of Sawit 0.0.8's 4,920-character instructions ended exactly at
/// character 4,096, and they had since grown to 7,264, so everything from their middle on was
/// lost. They stay at 4,000 at most, the details being each tool's own description. The one step
/// an agent must take before designing UI on the canvas, reading canvas_design_guide, is said
/// within their first 700 characters, where a client that keeps even less still has it.
#[test]
fn the_instructions_fit_what_clients_keep_and_name_the_design_guide_first() {
    let chars = INSTRUCTIONS.chars().count();
    assert!(
        chars <= 4000,
        "the instructions are {chars} characters; Claude Code keeps the first 4,096"
    );
    let pointer = "Before you design, build or review UI on the Sawit canvas, read \
                   canvas_design_guide and follow it.";
    let at = INSTRUCTIONS
        .find(pointer)
        .expect("the design guide's sentence");
    let ends = INSTRUCTIONS[..at].chars().count() + pointer.chars().count();
    assert!(
        ends <= 700,
        "the design guide's sentence ends at character {ends}"
    );
    let named = INSTRUCTIONS.matches("canvas_design_guide").count();
    assert_eq!(named, 1, "{INSTRUCTIONS}");
}

/// The instructions' databases line gives D1 and SQLite only what their tools take: creating,
/// dropping, truncating, refreshing, backing up and importing CSV refuse them (PostgreSQL only).
#[test]
fn the_instructions_say_what_d1_and_sqlite_take() {
    let line = INSTRUCTIONS
        .lines()
        .find(|line| line.starts_with("- Databases:"))
        .expect("the databases line");
    let (postgres, rest) = line
        .split_once("D1 and SQLite:")
        .expect("D1 and SQLite's part");
    let lite = rest.split(';').next().unwrap();
    for only in [
        "create",
        "drop",
        "truncate",
        "refresh",
        "back up",
        "import CSV",
    ] {
        assert!(postgres.contains(only), "{only}: {line}");
        assert!(!lite.contains(only), "{only}: {line}");
    }
    for both in ["list", "describe", "export"] {
        assert!(
            postgres.contains(both) && lite.contains(both),
            "{both}: {line}"
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
