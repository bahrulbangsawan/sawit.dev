//! The tool buttons of each kind of file: their groups, commands, icons and shortcuts.

use super::*;
use crate::input::{KeyOutcome, Mods, route_key};
use crate::policy::is_hidden;

/// Every tool button of the toolbar of a file of `kind`, in order.
fn buttons(kind: DocKind) -> Vec<ToolButton> {
    groups(kind)
        .into_iter()
        .flat_map(|group| group.tools)
        .collect()
}

/// The commands of those buttons.
fn commands(kind: DocKind) -> Vec<&'static str> {
    buttons(kind).iter().map(|button| button.command).collect()
}

/// The shortcut of the button for `command` in the toolbar of a file of `kind`.
fn shortcut(kind: DocKind, command: &str) -> Option<String> {
    let found = buttons(kind)
        .into_iter()
        .find(|button| button.command == command);
    found.unwrap().shortcut
}

/// The shape tools' commands, in the toolbar's order.
const SHAPES_TOOLS: [&str; 6] = [
    "tool.rectangle",
    "tool.line",
    "tool.arrow",
    "tool.ellipse",
    "tool.polygon",
    "tool.star",
];

/// Drags on the page of `doc` from `from` to `to`, at 100%, with the tool in use.
fn drag(doc: &mut BoardDoc, from: (f64, f64), to: (f64, f64)) {
    use figcraft_engine::{PointerEvent, PointerKind};
    let mid = ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0);
    for (kind, (x, y)) in [
        (PointerKind::Down, from),
        (PointerKind::Move, mid),
        (PointerKind::Move, to),
        (PointerKind::Up, to),
    ] {
        crate::input::pointer(doc, &PointerEvent::at(kind, x, y, 1.0)).unwrap();
    }
}

/// How many layers the page of `doc` holds, however deep.
fn layer_count(doc: &BoardDoc) -> usize {
    let page = doc.session.page().unwrap();
    doc.doc().unwrap().subtree(page).len()
}

#[test]
fn groups_differ_by_kind() {
    let on_board = commands(DocKind::Board);
    let on_design = commands(DocKind::Design);
    assert!(on_board.contains(&"tool.sticky"));
    assert!(on_design.contains(&"tool.frame"));
    assert!(!on_design.contains(&"tool.sticky"));
    // The frame and the design tools are on a board too, where the engine makes them.
    for command in on_design.iter().filter(|command| **command != "tool.scale") {
        assert!(on_board.contains(command), "{command} isn't on a board");
    }
    // Comments are not part of the canvas.
    assert!(!on_board.contains(&"tool.comment"));
    assert!(!on_design.contains(&"tool.comment"));
}

#[test]
fn groups_follow_the_specs_lists() {
    let layout = |kind| {
        groups(kind)
            .into_iter()
            .map(|group| {
                let tools: Vec<_> = group.tools.iter().map(|tool| tool.command).collect();
                (group.key, tools, group.flyout)
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        layout(DocKind::Board),
        [
            ("move", vec!["tool.move", "tool.hand"], None),
            (
                "ink",
                vec!["tool.marker", "tool.highlighter", "tool.eraser"],
                None
            ),
            ("sticky", vec!["tool.sticky"], Some(Flyout::StickyColors)),
            ("shape", vec!["tool.shape"], Some(Flyout::Shapes)),
            (
                "connector",
                vec!["tool.connector"],
                Some(Flyout::ConnectorStyles)
            ),
            (
                "text",
                vec!["tool.text", "tool.textOnPath"],
                Some(Flyout::Tools("text"))
            ),
            (
                "frame",
                vec!["tool.frame", "tool.section", "tool.slice"],
                Some(Flyout::Tools("frame"))
            ),
            (
                "geometry",
                SHAPES_TOOLS.to_vec(),
                Some(Flyout::Tools("geometry"))
            ),
            (
                "pen",
                vec!["tool.pen", "tool.pencil"],
                Some(Flyout::Tools("pen"))
            ),
            ("stamp", vec!["tool.stamp"], Some(Flyout::Stamps)),
        ]
    );
    assert_eq!(
        layout(DocKind::Design),
        [
            ("move", vec!["tool.move", "tool.scale", "tool.hand"], None),
            (
                "frame",
                vec!["tool.frame", "tool.section", "tool.slice"],
                Some(Flyout::Tools("frame"))
            ),
            ("shape", SHAPES_TOOLS.to_vec(), Some(Flyout::Tools("shape"))),
            (
                "pen",
                vec!["tool.pen", "tool.pencil"],
                Some(Flyout::Tools("pen"))
            ),
            (
                "text",
                vec!["tool.text", "tool.textOnPath"],
                Some(Flyout::Tools("text"))
            ),
        ]
    );
}

/// The user's ask: every kind of layer the engine's tools make is on the toolbar, on a board
/// as on a design file, and none of them only errors there. A drag with each drawing tool
/// makes a layer on both kinds of file; the pen makes a path of two clicks; text on a path
/// goes on a rectangle's outline.
#[test]
fn every_drawing_tool_makes_a_layer_on_both_kinds_of_file() {
    for (kind, _) in KINDS {
        let shown = commands(kind);
        for command in [
            "tool.frame",
            "tool.section",
            "tool.slice",
            "tool.rectangle",
            "tool.line",
            "tool.arrow",
            "tool.ellipse",
            "tool.polygon",
            "tool.star",
            "tool.pencil",
        ] {
            assert!(shown.contains(&command), "{kind:?}: {command} isn't shown");
            let mut doc = empty(kind);
            doc.execute(command, &json!({})).unwrap();
            drag(&mut doc, (10.0, 10.0), (120.0, 90.0));
            assert!(layer_count(&doc) > 1, "{kind:?}: {command} made nothing");
        }
        // The pen: two clicks and Enter make a path.
        let mut doc = empty(kind);
        doc.execute("tool.pen", &json!({})).unwrap();
        drag(&mut doc, (10.0, 10.0), (10.0, 10.0));
        drag(&mut doc, (120.0, 90.0), (120.0, 90.0));
        let _ = crate::input::route_key(&mut doc, "Enter", Mods::default());
        assert!(layer_count(&doc) > 1, "{kind:?}: the pen made nothing");
        // Text on a path: a click on a rectangle's outline.
        let mut doc = empty(kind);
        doc.execute("tool.rectangle", &json!({})).unwrap();
        drag(&mut doc, (0.0, 0.0), (200.0, 100.0));
        doc.execute("tool.textOnPath", &json!({})).unwrap();
        drag(&mut doc, (100.0, 0.0), (100.0, 0.0));
        // The text takes the rectangle's place, along its outline.
        let page = doc.session.page().unwrap();
        let on_path = doc.doc().unwrap().subtree(page).into_iter().any(|id| {
            let doc = doc.doc().unwrap();
            doc.node(id)
                .and_then(|node| node.kind.text())
                .is_some_and(|text| text.path.is_some())
        });
        assert!(on_path, "{kind:?}: no text on the path");
    }
}

#[test]
fn a_group_shows_the_tool_in_use_else_the_one_used_last() {
    let groups = groups(DocKind::Design);
    let shapes = groups.iter().find(|group| group.key == "shape").unwrap();
    // Nothing of the group used yet: its first tool.
    let shown = |active, last| shown_tool(shapes, active, last).tool;
    assert_eq!(shown(ToolId::Move, None), ToolId::Rectangle);
    // The tool in use, when it is one of them.
    assert_eq!(shown(ToolId::Star, None), ToolId::Star);
    assert_eq!(shown(ToolId::Star, Some(ToolId::Ellipse)), ToolId::Star);
    // Else the one used last.
    assert_eq!(shown(ToolId::Move, Some(ToolId::Ellipse)), ToolId::Ellipse);
    // A tool of another group used last counts for nothing here.
    assert_eq!(shown(ToolId::Move, Some(ToolId::Pen)), ToolId::Rectangle);
}

#[test]
fn every_tool_button_chooses_its_tool() {
    for (kind, count) in KINDS {
        let mut doc = match kind {
            DocKind::Board => board(),
            DocKind::Design => design(),
        };
        assert_eq!(buttons(kind).len(), count, "{kind:?}");
        for button in buttons(kind) {
            assert_eq!(button.command, format!("tool.{}", button.tool.id()));
            assert!(!button.label.is_empty(), "{}", button.command);
            // From another tool, so that the click is what makes this one active.
            let other = if button.tool == ToolId::Hand {
                "tool.move"
            } else {
                "tool.hand"
            };
            doc.execute(other, &json!({})).unwrap();
            assert_ne!(active(&doc.session), button.tool);
            if let Err(e) = doc.execute(button.command, &json!({})) {
                panic!("{kind:?} {}: {e}", button.command);
            }
            assert_eq!(active(&doc.session), button.tool, "{}", button.command);
        }
    }
}

#[test]
fn each_tool_has_an_icon_of_its_own() {
    // Section and Text are in both toolbars, with the same icon in both.
    let all: Vec<ToolButton> = buttons(DocKind::Board)
        .into_iter()
        .chain(buttons(DocKind::Design))
        .collect();
    assert_eq!(
        all.len(),
        KINDS.iter().map(|(_, count)| count).sum::<usize>()
    );
    for a in &all {
        for b in &all {
            let same = (a.tool == b.tool, a.icon == b.icon);
            assert_eq!(same.0, same.1, "{} and {}", a.command, b.command);
        }
    }
}

#[test]
fn no_tool_is_one_the_policy_hides() {
    // A hidden command is dead on the keyboard too, so its button would be the only way in.
    for (kind, count) in KINDS {
        let commands = commands(kind);
        assert_eq!(commands.len(), count, "{kind:?}");
        for command in commands {
            assert!(!is_hidden(command), "{command}");
        }
    }
}

#[test]
fn shortcuts_use_mac_glyphs() {
    let (on_board, on_design) = (DocKind::Board, DocKind::Design);
    assert_eq!(shortcut(on_board, "tool.move").as_deref(), Some("V"));
    assert_eq!(shortcut(on_board, "tool.section").as_deref(), Some("⇧S"));
    assert_eq!(shortcut(on_design, "tool.section").as_deref(), Some("⇧S"));
    // The other keys with Shift.
    assert_eq!(shortcut(on_design, "tool.arrow").as_deref(), Some("⇧L"));
    assert_eq!(shortcut(on_design, "tool.pencil").as_deref(), Some("⇧P"));
    // A tool with two keys shows the first, as the menu does.
    assert_eq!(shortcut(on_design, "tool.frame").as_deref(), Some("F"));
    // The board tools.
    for (command, key) in [
        ("tool.hand", "H"),
        ("tool.marker", "M"),
        ("tool.highlighter", "U"),
        ("tool.eraser", "E"),
        ("tool.sticky", "Q"),
        ("tool.shape", "B"),
        ("tool.connector", "X"),
        ("tool.text", "T"),
        ("tool.stamp", "W"),
    ] {
        let shown = shortcut(on_board, command);
        assert_eq!(shown.as_deref(), Some(key), "{command}");
    }
    // Polygon and Star have Sawit's keys, ^ and * (FigCraft has none for them).
    assert_eq!(shortcut(on_design, "tool.polygon").as_deref(), Some("⇧6"));
    assert_eq!(shortcut(on_board, "tool.star").as_deref(), Some("⇧8"));
    // The design tools on a board have their keys there too.
    for (command, key) in [
        ("tool.frame", "F"),
        ("tool.rectangle", "R"),
        ("tool.ellipse", "O"),
        ("tool.line", "L"),
        ("tool.arrow", "⇧L"),
        ("tool.pen", "P"),
        ("tool.slice", "S"),
    ] {
        assert_eq!(
            shortcut(on_board, command).as_deref(),
            Some(key),
            "{command}"
        );
    }
    // Text on a path has none in FigCraft: Sawit gives it ⇧T, beside Text's T.
    assert_eq!(shortcut(on_board, "tool.textOnPath").as_deref(), Some("⇧T"));
}

#[test]
fn active_tool_follows_session() {
    let mut b = board();
    assert_eq!(active(&b.session), ToolId::Move);
    // The keys of Sticky, Shapes and Connector, then Move.
    for (key, tool) in [
        ("q", ToolId::Sticky),
        ("b", ToolId::BoardShape),
        ("x", ToolId::Connector),
        ("v", ToolId::Move),
    ] {
        assert_eq!(
            route_key(&mut b, key, Mods::default()),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(active(&b.session), tool, "{key}");
    }
}
