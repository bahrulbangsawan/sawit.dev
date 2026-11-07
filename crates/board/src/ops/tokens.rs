//! Tokens over raw values (B12): a write's colour may name one of the file's colour variables
//! (`"backgroundColor": "fg/muted"`), which draws in its value; and every solid fill or stroke a
//! write leaves whose colour is exactly a colour variable's is bound to it, so the layer follows
//! the system (its modes, a change of the token). Raw values that match no token are left as they
//! are, for the lint to name (`col.16`).

use figcraft_color::PaintKind;
use figcraft_doc::{Document, NodeId, VariableValue};
use serde_json::{Value, json};

use crate::doc::BoardDoc;

/// The keys of an element whose value is a colour.
const COLOR_KEYS: &[&str] = &["backgroundColor", "strokeColor"];

/// The file's colour variables: name and value as 8-bit RGBA (their first mode's).
fn colour_tokens(doc: &Document) -> Vec<(String, [u8; 4])> {
    doc.variables
        .variables
        .iter()
        .filter_map(|v| match v.values.values().next()? {
            VariableValue::Color(c) => Some((v.name.clone(), bytes(c))),
            _ => None,
        })
        .collect()
}

fn bytes(c: &figcraft_color::Color) -> [u8; 4] {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c.r), b(c.g), b(c.b), b(c.a)]
}

fn hex([r, g, b, a]: [u8; 4]) -> String {
    match a {
        255 => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => format!("#{r:02x}{g:02x}{b:02x}{a:02x}"),
    }
}

/// `op`'s elements with each colour that names a token replaced by the token's value: what the
/// engine draws. The names it replaced, for the reply.
pub(super) fn resolve_names(board: &BoardDoc, op: &Value) -> (Value, Vec<String>) {
    let Ok(doc) = board.doc() else {
        return (op.clone(), Vec::new());
    };
    let tokens = colour_tokens(&doc);
    let mut op = op.clone();
    let mut named = Vec::new();
    if tokens.is_empty() {
        return (op, named);
    }
    for element in op
        .get_mut("elements")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        for key in COLOR_KEYS {
            let Some(name) = element.get(*key).and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            if let Some((token, value)) = tokens.iter().find(|(t, _)| t == name) {
                named.push(token.clone());
                element[*key] = json!(hex(*value));
            }
        }
    }
    (op, named)
}

/// Binds every solid fill and stroke of `ids` and their layers whose colour is exactly a colour
/// token's to that token (`variable.bind`). How many it bound.
pub(super) fn bind_matching(board: &mut BoardDoc, ids: &[NodeId]) -> usize {
    let Ok(doc) = board.doc() else {
        return 0;
    };
    let tokens = colour_tokens(&doc);
    if tokens.is_empty() {
        return 0;
    }
    let mut binds: Vec<(NodeId, String, String)> = Vec::new();
    let mut stack: Vec<NodeId> = ids.to_vec();
    while let Some(id) = stack.pop() {
        let Some(node) = doc.node(id) else {
            continue;
        };
        stack.extend(node.children.iter().copied());
        for (list, paints) in [("fills", &node.fills), ("strokes", &node.strokes)] {
            for (i, paint) in paints.iter().enumerate() {
                let PaintKind::Solid { color } = &paint.kind else {
                    continue;
                };
                let field = format!("{list}/{i}/color");
                if node.bound.contains_key(&field) {
                    continue;
                }
                let value = bytes(color);
                if let Some((token, _)) = tokens.iter().find(|(_, t)| *t == value) {
                    binds.push((id, field, token.clone()));
                }
            }
        }
    }
    let mut bound = 0;
    for (id, field, token) in binds {
        let params = json!({"ids": [id.0], "field": field, "variable": token});
        if board.execute("variable.bind", &params).is_ok() {
            bound += 1;
        }
    }
    bound
}

/// The layers an op's reply names (`ids`, or the elements' `id`s of an update).
pub(super) fn ids_in(reply: &Value, op: &Value, doc: &Document) -> Vec<NodeId> {
    let mut ids: Vec<NodeId> = (reply
        .get("ids")
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|v| v.as_u64().map(NodeId).or_else(|| v.as_str()?.parse().ok()))
    .collect();
    for element in op
        .get("elements")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let Some(id) = element.get("id").and_then(Value::as_str)
            && let Some(found) = crate::skeleton::resolve(doc, id)
        {
            ids.push(found);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_tokens() -> BoardDoc {
        let mut board = BoardDoc::new_board("Design").unwrap();
        board.session.doc_mut().unwrap().kind = figcraft_doc::FileKind::Design;
        board
            .execute("variable.createCollection", &json!({"name": "Color"}))
            .unwrap();
        board
            .execute("variable.create", &json!({"collection": "Color", "name": "accent", "type": "color", "value": "#2563eb"}))
            .unwrap();
        board
    }

    #[test]
    fn a_token_name_draws_its_value_and_a_matching_raw_value_is_bound() {
        let mut board = with_tokens();
        let op = json!({"op": "add", "elements": [
            {"type": "frame", "x": 0, "y": 0, "width": 200, "height": 100, "backgroundColor": "accent"},
            {"type": "frame", "x": 300, "y": 0, "width": 200, "height": 100, "backgroundColor": "#2563eb"},
            {"type": "frame", "x": 600, "y": 0, "width": 200, "height": 100, "backgroundColor": "#ff0000"},
        ]});
        let reply = super::super::run(&mut board, &op);
        assert_eq!(reply["ok"], json!(true), "{reply}");
        assert_eq!(reply["tokens"]["named"], json!(["accent"]));
        assert_eq!(reply["tokens"]["bound"], json!(2), "{reply}");
        let doc = board.doc().unwrap();
        let ids: Vec<NodeId> = reply["ids"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_u64().map(NodeId).or_else(|| v.as_str()?.parse().ok()))
            .collect();
        let bound = |id: NodeId| doc.node(id).unwrap().bound.contains_key("fills/0/color");
        assert!(bound(ids[0]) && bound(ids[1]));
        assert!(
            !bound(ids[2]),
            "a colour off the system stays raw, for the lint"
        );
    }
}
