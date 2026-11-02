//! What agents are told of the canvas tools: that the canvas is the native one, the page each acts
//! on, the project whose canvas they use, the canvas file they take, and that Sawit's design
//! standard comes first.

use crate::{INSTRUCTIONS, Sawit};

/// The canvas tools that work on a canvas: every one but canvas_design_guide, which reads Sawit's
/// design standard and no canvas.
fn on_a_canvas(name: &str) -> bool {
    name.starts_with("canvas_") && name != "canvas_design_guide"
}

/// The canvas tools that make UI end by pointing at Sawit's design standard, and the guide says
/// when to read it, to follow it and to run its checklist. It reads no canvas, so it takes no
/// `file` or `project`, and it only reads.
#[test]
fn the_tools_that_make_ui_point_at_the_design_guide() {
    let tools = Sawit::router().list_all();
    let tool = |name: &str| tools.iter().find(|tool| tool.name == name).expect(name);
    let said = |name: &str| {
        tool(name)
            .description
            .as_deref()
            .unwrap_or_default()
            .to_string()
    };
    for name in [
        "canvas_add_elements",
        "canvas_add_diagram",
        "canvas_add_mermaid",
        "canvas_update",
        "canvas_command",
        "canvas_commands",
    ] {
        let text = said(name);
        let points = text.ends_with(". Designing UI? Read canvas_design_guide first and follow it");
        assert!(points, "{name} doesn't point at the guide: {text}");
    }
    let guide = said("canvas_design_guide");
    for words in [
        "Read it before you design, build or review any UI on the canvas",
        "follow it",
        "run its checklist before you say a design is done",
        "`section`",
    ] {
        assert!(guide.contains(words), "no {words:?} in {guide}");
    }
    let guide = tool("canvas_design_guide");
    let properties = guide
        .input_schema
        .get("properties")
        .and_then(|p| p.as_object());
    let names: Vec<&String> = properties.map(|p| p.keys().collect()).unwrap_or_default();
    assert_eq!(names, ["section"]);
    let hints = guide.annotations.as_ref().expect("its hints");
    assert_eq!(hints.read_only_hint, Some(true));
    assert_eq!(hints.open_world_hint, Some(false));
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
        elements.contains("Types: rectangle, ellipse, diamond, sticky, text, arrow, line, frame"),
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
        &format!("A line through more than two points is drawn as ink, {ink} unless strokeColor"),
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
            clear.contains("The board it held is kept beside the canvas file as <file>.bak (canvas.sawit.bak for the project's canvas, home.sawit.bak for home.sawit)")
                && clear.contains("the 4 kept before it moving to .bak.1 to .bak.4")
                && clear.contains("never a deletion that leaves layers on the canvas")
                && clear.contains("canvas_load brings it back")
                && clear.contains("in the app the user can also undo it"),
            "{clear}"
        );
    let delete = tool("canvas_delete");
    assert!(
            delete.contains(
                "The board they were on is first kept beside the canvas file as <file>.del.bak (canvas.sawit.del.bak for the project's canvas, home.sawit.del.bak for home.sawit)"
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
            "A .sawit file to load, or an .excalidraw file to import (replaces the board)"
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
    assert!(canvas.contains("the Sawit canvas"), "{canvas}");
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
    for tool in tools.iter().filter(|tool| on_a_canvas(&tool.name)) {
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
/// alone, though the app opens any `.sawit` file in a canvas tab of its own.
#[test]
fn every_canvas_tool_takes_a_file_and_the_instructions_say_so() {
    let tools = Sawit::router().list_all();
    let canvas_tools = tools.iter().filter(|tool| on_a_canvas(&tool.name));
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
        "your project's canvas (.nyawit/canvas.sawit)",
        "the .sawit file named by `file`",
        "list, clear and export SVG/PNG the page on show",
        "update and delete layers by id, on any page",
        "load another canvas in place of every page",
        "keep what they clear, load over or delete in backups beside it (.bak, .del.bak)",
    ] {
        assert!(line.contains(said), "no {said:?} in {line}");
    }
}
