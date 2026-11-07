//! The `starter` op (B12): a starter design system an agent builds on instead of starting from
//! nothing. Sawit's own tokens (`skills/sawit-canvas-design/references/tokens.md`) as variables,
//! colours with Light and Dark modes, the spacing scale and radii; then a Components frame beside
//! the page's content with a primary and a secondary button, an input and a card, each a
//! component built from the tokens with auto layout. A file that has the colour collection
//! already keeps it. Answers the variables and components made and the workflow's status.

use serde_json::{Value, json};

use super::failure;
use crate::doc::BoardDoc;

/// Colour tokens: name, Light, Dark.
const COLORS: &[(&str, &str, &str)] = &[
    ("bg/surface", "#ffffff", "#0f1115"),
    ("bg/subtle", "#f5f6f8", "#171a20"),
    ("fg/default", "#111827", "#f3f4f6"),
    ("fg/muted", "#4b5563", "#a1a7b3"),
    ("border", "#e5e7eb", "#2a2f38"),
    ("accent", "#2563eb", "#4f8cff"),
    ("accent/fg", "#ffffff", "#0b1220"),
    ("danger", "#b42318", "#f97066"),
];

/// Number tokens: name and value (px).
const NUMBERS: &[(&str, f64)] = &[
    ("space/1", 4.0),
    ("space/2", 8.0),
    ("space/3", 12.0),
    ("space/4", 16.0),
    ("space/6", 24.0),
    ("space/8", 32.0),
    ("radius/sm", 6.0),
    ("radius/md", 10.0),
    ("radius/lg", 16.0),
    ("type/caption", 13.0),
    ("type/body", 16.0),
    ("type/heading", 18.0),
    ("type/title", 24.0),
];

pub(super) fn starter(board: &mut BoardDoc, op: &Value) -> Value {
    match build(board, op) {
        Ok(reply) => reply,
        Err(error) => failure(format!("starter: {error}")),
    }
}

fn run(board: &mut BoardDoc, command: &str, params: Value) -> Result<Value, String> {
    board
        .execute(command, &params)
        .map_err(|e| format!("{command}: {e}"))
}

fn id(v: &Value) -> Result<u64, String> {
    v["id"]
        .as_u64()
        .or_else(|| v["id"].as_str()?.parse().ok())
        .ok_or_else(|| "no id".to_string())
}

fn bind(board: &mut BoardDoc, id: u64, field: &str, variable: &str) -> Result<(), String> {
    run(
        board,
        "variable.bind",
        json!({"ids": [id], "field": field, "variable": variable}),
    )
    .map(drop)
}

fn build(board: &mut BoardDoc, op: &Value) -> Result<Value, String> {
    let has = board
        .doc()
        .map_err(|e| e.to_string())?
        .variables
        .collections
        .iter()
        .any(|c| c.name == "Color");
    let mut made = Vec::new();
    if !has {
        run(
            board,
            "variable.createCollection",
            json!({"name": "Color", "modes": ["Light", "Dark"]}),
        )?;
        for (name, light, dark) in COLORS {
            run(
                board,
                "variable.create",
                json!({"collection": "Color", "name": name, "type": "color",
                "values": {"Light": light, "Dark": dark}}),
            )?;
            made.push(*name);
        }
        run(board, "variable.createCollection", json!({"name": "Scale"}))?;
        for (name, value) in NUMBERS {
            run(
                board,
                "variable.create",
                json!({"collection": "Scale", "name": name, "type": "number", "value": value}),
            )?;
            made.push(*name);
        }
    }
    // Beside the page's content, or where the op says.
    let (x, y) = match (
        op.get("x").and_then(Value::as_f64),
        op.get("y").and_then(Value::as_f64),
    ) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            let doc = board.doc().map_err(|e| e.to_string())?;
            let page = board.session.ds().map_err(|e| e.to_string())?.page;
            doc.page_content_bounds(page)
                .map_or((0.0, 0.0), |b| (b.x1 + 200.0, b.y0))
        }
    };
    let section = id(&run(
        board,
        "frame.create",
        json!({"x": x, "y": y, "width": 880, "height": 360, "name": "Components"}),
    )?)?;
    bind(board, section, "fills/0/color", "bg/subtle")?;
    let mut components = Vec::new();
    let button = |board: &mut BoardDoc,
                  x: f64,
                  name: &str,
                  fill: &str,
                  ink: &str|
     -> Result<u64, String> {
        let b = id(&run(
            board,
            "frame.create",
            json!({"x": x, "y": 40, "width": 120, "height": 44, "parent": section, "name": name}),
        )?)?;
        let label = id(&run(
            board,
            "text.create",
            json!({"text": "Button", "x": 0, "y": 0, "parent": b, "props": {"fontSize": 16}}),
        )?)?;
        run(
            board,
            "layout.add",
            json!({"ids": [b], "direction": "horizontal"}),
        )?;
        bind(board, b, "fills/0/color", fill)?;
        bind(board, label, "fills/0/color", ink)?;
        bind(board, b, "paddingHorizontal", "space/4")?;
        bind(board, b, "paddingVertical", "space/3")?;
        bind(board, b, "cornerRadius", "radius/md")?;
        bind(board, label, "fontSize", "type/body")?;
        run(board, "component.create", json!({"ids": [b]}))?;
        Ok(b)
    };
    components.push((
        "Button / Primary",
        button(board, 40.0, "Button / Primary", "accent", "accent/fg")?,
    ));
    components.push((
        "Button / Secondary",
        button(
            board,
            200.0,
            "Button / Secondary",
            "bg/surface",
            "fg/default",
        )?,
    ));
    // An input: a bordered field with its label above.
    let input = id(&run(
        board,
        "frame.create",
        json!({"x": 40, "y": 140, "width": 280, "height": 72, "parent": section, "name": "Input"}),
    )?)?;
    let label = id(&run(
        board,
        "text.create",
        json!({"text": "Email", "x": 0, "y": 0, "parent": input, "props": {"fontSize": 13}}),
    )?)?;
    let field = id(&run(
        board,
        "frame.create",
        json!({"x": 0, "y": 24, "width": 280, "height": 44, "parent": input, "name": "Field"}),
    )?)?;
    let value = id(&run(
        board,
        "text.create",
        json!({"text": "you@example.com", "x": 12, "y": 12, "parent": field, "props": {"fontSize": 16}}),
    )?)?;
    run(
        board,
        "node.set",
        json!({"ids": [field], "props": {"stroke": "#e5e7eb"}}),
    )?;
    run(
        board,
        "layout.add",
        json!({"ids": [input], "direction": "vertical"}),
    )?;
    run(
        board,
        "layout.add",
        json!({"ids": [field], "direction": "horizontal"}),
    )?;
    bind(board, input, "gap", "space/2")?;
    bind(board, input, "fills/0/color", "bg/subtle")?;
    bind(board, label, "fills/0/color", "fg/default")?;
    bind(board, field, "fills/0/color", "bg/surface")?;
    bind(board, field, "strokes/0/color", "border")?;
    bind(board, field, "cornerRadius", "radius/sm")?;
    bind(board, field, "paddingHorizontal", "space/3")?;
    bind(board, value, "fills/0/color", "fg/muted")?;
    run(board, "component.create", json!({"ids": [input]}))?;
    components.push(("Input", input));
    // A card: a title and a line of body copy.
    let card = id(&run(
        board,
        "frame.create",
        json!({"x": 400, "y": 40, "width": 320, "height": 140, "parent": section, "name": "Card"}),
    )?)?;
    let title = id(&run(
        board,
        "text.create",
        json!({"text": "Card title", "x": 0, "y": 0, "parent": card, "props": {"fontSize": 18}}),
    )?)?;
    let body = id(&run(
        board,
        "text.create",
        json!({"text": "One or two lines that say what this card holds.", "x": 0, "y": 0, "parent": card, "props": {"fontSize": 14}}),
    )?)?;
    run(
        board,
        "node.set",
        json!({"ids": [card], "props": {"stroke": "#e5e7eb"}}),
    )?;
    run(
        board,
        "layout.add",
        json!({"ids": [card], "direction": "vertical"}),
    )?;
    bind(board, card, "fills/0/color", "bg/surface")?;
    bind(board, card, "strokes/0/color", "border")?;
    bind(board, card, "cornerRadius", "radius/lg")?;
    bind(board, card, "paddingHorizontal", "space/6")?;
    bind(board, card, "paddingVertical", "space/6")?;
    bind(board, card, "gap", "space/2")?;
    bind(board, title, "fills/0/color", "fg/default")?;
    bind(board, body, "fills/0/color", "fg/muted")?;
    run(board, "component.create", json!({"ids": [card]}))?;
    components.push(("Card", card));
    let doc = board.doc().map_err(|e| e.to_string())?;
    Ok(json!({
        "ok": true,
        "variables": made,
        "frame": section.to_string(),
        "components": components.iter().map(|(name, id)| json!({"name": name, "id": id.to_string()})).collect::<Vec<_>>(),
        "status": crate::checks::lint::design_status(&doc),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_starter_lays_out_tokens_and_components_from_them() {
        let mut board = BoardDoc::new_board("Design").unwrap();
        board.session.doc_mut().unwrap().kind = figcraft_doc::FileKind::Design;
        let reply = super::super::run(&mut board, &json!({"op": "starter"}));
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["components"].as_array().unwrap().len(), 4);
        assert_eq!(reply["status"]["stage"], "components", "{reply}");
        // The components are clean by the lint: their colours are the tokens'.
        let doc = board.doc().unwrap();
        let frame: figcraft_doc::NodeId = reply["frame"].as_str().unwrap().parse().unwrap();
        let lint = crate::checks::lint::lint(&doc, frame, false);
        let raw: Vec<_> = lint
            .findings
            .iter()
            .filter(|f| f.finding.rule == "col.16")
            .collect();
        assert!(raw.is_empty(), "{raw:?}");
        // Run again: the tokens are kept, not made twice.
        let again = super::super::run(&mut board, &json!({"op": "starter"}));
        assert_eq!(again["variables"], json!([]));
    }
}

/// Writes the design file the screens e2e spec presents from (`e2e/fixtures/screens/design.sawit`):
/// `cargo test -p sawit-board write_design_fixture -- --ignored`.
#[cfg(test)]
#[test]
#[ignore]
fn write_design_fixture() {
    let mut board = BoardDoc::new_board("Screens").unwrap();
    board.session.doc_mut().unwrap().kind = figcraft_doc::FileKind::Design;
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../e2e/fixtures/screens/design.sawit");
    std::fs::write(path, board.to_bytes().unwrap()).unwrap();
}
