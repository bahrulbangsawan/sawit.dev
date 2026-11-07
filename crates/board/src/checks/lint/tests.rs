//! Each lint rule on a small page: a clean page from tokens, components and auto layout passes;
//! a sloppy one fires each rule; sketch mode stays quiet; the workflow's stage follows the file.

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::{Severity, design_status, lint};
use crate::BoardDoc;

fn run(board: &mut BoardDoc, command: &str, params: Value) -> Value {
    board
        .execute(command, &params)
        .unwrap_or_else(|e| panic!("{command}: {e}"))
}

fn id(v: &Value) -> u64 {
    v["id"].as_u64().unwrap()
}

/// The rules a lint of `scope` fires.
fn rules(board: &BoardDoc, scope: NodeId, sketch: bool) -> Vec<String> {
    let doc = board.doc().unwrap();
    lint(&doc, scope, sketch)
        .findings
        .iter()
        .map(|f| f.finding.rule.clone())
        .collect()
}

fn design() -> BoardDoc {
    let mut board = BoardDoc::new_board("Design").unwrap();
    board.session.doc_mut().unwrap().kind = figcraft_doc::FileKind::Design;
    board
}

/// Colour tokens, as the workflow's first step makes them.
fn tokens(board: &mut BoardDoc) {
    run(board, "variable.createCollection", json!({"name": "Color"}));
    for (name, hex) in [
        ("bg/surface", "#ffffff"),
        ("fg/default", "#111827"),
        ("accent", "#2563eb"),
    ] {
        run(
            board,
            "variable.create",
            json!({"collection": "Color", "name": name, "type": "color", "value": hex}),
        );
    }
}

#[test]
fn a_clean_screen_passes_and_a_sloppy_one_fires_each_rule() {
    // Clean: tokens, a screen with auto layout, its fill bound, a heading and a line of copy.
    let mut clean = design();
    tokens(&mut clean);
    let screen = id(&run(
        &mut clean,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 390, "height": 844, "name": "Home"}),
    ));
    run(&mut clean, "layout.add", json!({"ids": [screen]}));
    run(
        &mut clean,
        "variable.bind",
        json!({"ids": [screen], "field": "fills/0/color", "variable": "bg/surface"}),
    );
    let t = id(&run(
        &mut clean,
        "text.create",
        json!({"text": "Today", "x": 0, "y": 0, "parent": screen, "props": {"fontSize": 28}}),
    ));
    run(
        &mut clean,
        "variable.bind",
        json!({"ids": [t], "field": "fills/0/color", "variable": "fg/default"}),
    );
    let page = clean.session.ds().unwrap().page;
    let fired = rules(&clean, page, false);
    assert!(
        fired
            .iter()
            .all(|r| r != "cnv.15" && r != "cpy.19" && r != "cnv.11" && r != "cnv.13"),
        "a clean screen: {fired:?}"
    );
    let doc = clean.doc().unwrap();
    assert_eq!(lint(&doc, page, false).errors, 0, "{fired:?}");

    // Sloppy: no tokens, placeholder copy, an emoji icon, three copied cards placed by hand,
    // centred text, a purple-to-blue gradient.
    let mut slop = design();
    let screen = id(&run(
        &mut slop,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 390, "height": 844, "name": "Landing"}),
    ));
    run(
        &mut slop,
        "text.create",
        json!({"text": "Welcome to our platform", "x": 24, "y": 40, "parent": screen, "props": {"fontSize": 32}}),
    );
    run(
        &mut slop,
        "text.create",
        json!({"text": "🚀", "x": 24, "y": 100, "parent": screen, "props": {"fontSize": 24}}),
    );
    for i in 0..3 {
        let card = id(&run(
            &mut slop,
            "frame.create",
            json!({"x": 24, "y": 160 + i * 200 + i * i * 7, "width": 342, "height": 180, "parent": screen, "name": "Feature"}),
        ));
        run(
            &mut slop,
            "text.create",
            json!({"text": "Lorem ipsum", "x": 16, "y": 16, "parent": card}),
        );
    }
    {
        let doc = slop.session.doc_mut().unwrap();
        let ids: Vec<NodeId> = doc
            .nodes()
            .filter(|n| matches!(n.kind, figcraft_doc::NodeKind::Text(_)))
            .map(|n| n.id)
            .collect();
        for t in ids {
            if let Some(figcraft_doc::Node {
                kind: figcraft_doc::NodeKind::Text(text),
                ..
            }) = doc.node_mut(t)
            {
                text.align_h = figcraft_doc::TextAlignH::Center;
            }
        }
        let node = doc.node_mut(NodeId(screen)).unwrap();
        node.fills = vec![figcraft_color::Paint {
            kind: figcraft_color::PaintKind::Gradient(figcraft_color::Gradient {
                kind: figcraft_color::GradientKind::Linear,
                stops: vec![
                    figcraft_color::ColorStop {
                        position: 0.0,
                        color: figcraft_color::Color {
                            r: 0.49,
                            g: 0.23,
                            b: 0.93,
                            a: 1.0,
                        },
                    },
                    figcraft_color::ColorStop {
                        position: 1.0,
                        color: figcraft_color::Color {
                            r: 0.15,
                            g: 0.39,
                            b: 0.92,
                            a: 1.0,
                        },
                    },
                ],
                from: (0.0, 0.0),
                to: (1.0, 1.0),
                width: (1.0, 0.0),
            }),
            opacity: 1.0,
            visible: true,
            blend: Default::default(),
            color_alpha: 1.0,
        }];
    }
    let page = slop.session.ds().unwrap().page;
    let fired = rules(&slop, page, false);
    for rule in [
        "cnv.15", "cpy.19", "cmp.1", "cnv.13", "cnv.11", "cnv.12", "lay.13", "col.6",
    ] {
        assert!(
            fired.iter().any(|r| r == rule),
            "{rule} fires on the sloppy page: {fired:?}"
        );
    }
    let doc = slop.doc().unwrap();
    let scored = lint(&doc, page, false);
    assert!(
        scored.errors >= 1 && scored.warnings >= 6,
        "{} errors, {} warnings",
        scored.errors,
        scored.warnings
    );
    // Each finding names a layer and says what to do; the copies come with their command.
    assert!(
        scored
            .findings
            .iter()
            .all(|f| f.finding.id.is_some() && !f.finding.fix.is_empty())
    );
    let copies = scored
        .findings
        .iter()
        .find(|f| f.finding.rule == "cnv.13")
        .unwrap();
    assert_eq!(
        copies.command.as_ref().unwrap()["command"],
        "component.create"
    );

    // Sketch mode: only the errors.
    let sketch = lint(&doc, page, true);
    assert_eq!(sketch.warnings, 0);
    assert!(
        sketch
            .findings
            .iter()
            .all(|f| f.severity == Severity::Error)
    );
}

#[test]
fn a_raw_colour_off_the_tokens_names_the_nearest_one() {
    let mut board = design();
    tokens(&mut board);
    let screen = id(&run(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 390, "height": 844}),
    ));
    run(
        &mut board,
        "node.set",
        json!({"id": screen, "props": {"fills": [{"type": "solid", "color": "#2463ea"}]}}),
    );
    let doc = board.doc().unwrap();
    let found = lint(&doc, NodeId(screen), false);
    let raw = found
        .findings
        .iter()
        .find(|f| f.finding.rule == "col.16")
        .expect("col.16");
    assert!(raw.finding.fix.contains("accent"), "{}", raw.finding.fix);
    assert_eq!(
        raw.command.as_ref().unwrap()["params"]["variable"],
        "accent"
    );
}

#[test]
fn the_workflow_stage_follows_the_file() {
    let mut board = design();
    assert_eq!(design_status(&board.doc().unwrap())["stage"], "no system");
    tokens(&mut board);
    assert_eq!(design_status(&board.doc().unwrap())["stage"], "system");
    let button = id(&run(
        &mut board,
        "frame.create",
        json!({"x": 0, "y": 0, "width": 120, "height": 44, "name": "Button"}),
    ));
    run(&mut board, "component.create", json!({"ids": [button]}));
    assert_eq!(design_status(&board.doc().unwrap())["stage"], "components");
    run(
        &mut board,
        "frame.create",
        json!({"x": 400, "y": 0, "width": 390, "height": 844, "name": "Home"}),
    );
    assert_eq!(design_status(&board.doc().unwrap())["stage"], "screens");
    run(
        &mut board,
        "frame.create",
        json!({"x": 900, "y": 0, "width": 1200, "height": 800, "name": "Cover"}),
    );
    assert_eq!(design_status(&board.doc().unwrap())["stage"], "cover");
}
