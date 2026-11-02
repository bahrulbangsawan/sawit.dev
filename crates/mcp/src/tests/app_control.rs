//! What agents are told of the tools that need the running app: what a closed app keeps, a
//! terminal's tabs and panes, and the tools that end a program or rewrite the keymap.

use crate::{INSTRUCTIONS, Sawit};

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
            "terminal_rename",
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
