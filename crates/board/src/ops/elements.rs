//! The ops on the layers of the page on show, as an agent's elements: `get`, `add`, `update`,
//! `delete` and `clear`.

use figcraft_doc::{BoardItem, Document, NodeId};
use figcraft_engine::{EngineError, Result as EngineResult, Session};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::{failure, success};
use crate::doc::{BoardDoc, BoardError, DocKind};
use crate::skeleton::{self, Applied, Label, Skeleton};
use crate::summary;

pub(super) fn get(board: &BoardDoc) -> Value {
    match summaries(board) {
        Ok(elements) => success("elements", Value::Array(elements)),
        Err(error) => failure(error),
    }
}

/// The summaries of the layers of the page on show.
fn summaries(board: &BoardDoc) -> Result<Vec<Value>, BoardError> {
    let ds = board.session.ds()?;
    Ok(summary::summarize(&ds.doc, ds.page))
}

/// The top-level layers of the page on show, a connector's label among them.
pub(super) fn page_layers(board: &BoardDoc) -> Result<Vec<NodeId>, BoardError> {
    let ds = board.session.ds()?;
    let page = ds.doc.node(ds.page);
    Ok(page.map(|page| page.children.clone()).unwrap_or_default())
}

/// An id as text: a string that isn't empty, or a number (the engine's node ids are numbers).
fn id_text(id: &Value) -> Option<String> {
    match id {
        Value::String(id) if !id.is_empty() => Some(id.clone()),
        Value::Number(id) => Some(id.to_string()),
        _ => None,
    }
}

/// The layer `id` names, for an op: an agent's id, else a node id ([`skeleton::resolve`]), if it
/// is a layer of a page. The page is no element, though its node id is a number like the others'.
fn layer_named(doc: &Document, id: &str) -> Option<NodeId> {
    let layer = skeleton::resolve(doc, id)?;
    let is_page = doc.node(layer)?.is_page();
    (!is_page && doc.page_of(layer).is_some()).then_some(layer)
}

pub(super) fn add(board: &mut BoardDoc, op: &Value) -> Value {
    let items = match skeletons(op) {
        Ok(items) => items,
        Err(error) => return failure(format!("add: {error}")),
    };
    match skeleton::apply(board, &items, (0.0, 0.0)) {
        Ok(applied) => added(&applied),
        Err(error) if needs_a_board(board, &error.to_string()) => {
            failure(format!("add: {error}. {ADD_ON_A_DESIGN_FILE}"))
        }
        Err(error) => failure(format!("add: {error}")),
    }
}

/// What `add`'s refusal goes on to say when an element is one only a board has and the canvas is a
/// design file: what it draws there, and the engine's commands that draw the rest (SMOKE-3).
const ADD_ON_A_DESIGN_FILE: &str = "On a design file add draws text, frames and images; draw \
     shapes and lines with the command op: shape.rectangle, shape.ellipse, shape.line or \
     shape.arrow";

/// Whether `error` is the engine refusing a board's command because the canvas is a design file
/// ([`crate::doc::BOARDS_ONLY`]).
pub(super) fn needs_a_board(board: &BoardDoc, error: &str) -> bool {
    board.kind() == DocKind::Design && error.contains(crate::doc::BOARDS_ONLY)
}

/// The elements of an `add`, each read as a [`Skeleton`].
fn skeletons(op: &Value) -> Result<Vec<Skeleton>, String> {
    let items = op.get("elements").and_then(Value::as_array).ok_or(
        r#"it needs the elements to draw, like {"elements": [{"type": "rectangle", "x": 0, "y": 0}]}"#,
    )?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Skeleton::deserialize(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

/// `{ok, added, ids}` for what `apply` made, and the `notes` on what it skipped when it skipped
/// something.
fn added(applied: &Applied) -> Value {
    let mut reply = json!({"ok": true, "added": applied.ids.len(), "ids": applied.ids});
    if !applied.notes.is_empty() {
        reply["notes"] = json!(applied.notes);
    }
    reply
}

/// One element of an `update`: the layer it names, and what it changes.
struct Change {
    id: String,
    fields: Fields,
}

/// What an `update` element can change. What it leaves out stays as it is, and any other key is
/// ignored.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Fields {
    x: Option<f64>,
    y: Option<f64>,
    width: Option<f64>,
    height: Option<f64>,
    text: Option<String>,
    label: Option<Label>,
    /// Text added to the end of the layer's: long copy sent in parts.
    append: Option<String>,
    name: Option<String>,
    #[serde(rename = "backgroundColor")]
    background_color: Option<String>,
    #[serde(rename = "strokeColor")]
    stroke_color: Option<String>,
}

impl Change {
    fn read(item: &Value) -> Result<Self, String> {
        let id = item
            .get("id")
            .and_then(id_text)
            .ok_or("it needs the `id` of a layer")?;
        let fields = Fields::deserialize(item).map_err(|error| error.to_string())?;
        Ok(Self { id, fields })
    }

    /// The text to set: `text`, else the `label`'s, as for a shape in a skeleton.
    fn text(&self) -> Option<&str> {
        let label = self.fields.label.as_ref().map(|label| label.text.as_str());
        self.fields.text.as_deref().or(label)
    }
}

/// The elements of an `update`.
fn changes(op: &Value) -> Result<Vec<Change>, String> {
    let items = op
        .get("elements")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the changes to make, like {"elements": [{"id": "a", "x": 10}]}"#)?;
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            Change::read(item).map_err(|error| format!("elements[{index}]: {error}"))
        })
        .collect()
}

pub(super) fn update(board: &mut BoardDoc, op: &Value) -> Value {
    let changes = match changes(op) {
        Ok(changes) => changes,
        Err(error) => return failure(format!("update: {error}")),
    };
    let (mut updated, mut missing, mut notes) = (0, Vec::new(), Vec::new());
    let result = crate::doc::grouped(&mut board.session, "Update elements", |session| {
        for (index, change) in changes.iter().enumerate() {
            let Some(layer) = layer_named(session.doc()?, &change.id) else {
                missing.push(change.id.clone());
                continue;
            };
            change_layer(session, layer, index, change, &mut notes).map_err(|error| {
                EngineError::Other(format!("elements[{index}] ({}): {error}", change.id))
            })?;
            updated += 1;
        }
        Ok(())
    });
    match result {
        Ok(()) => {
            let mut reply = json!({"ok": true, "updated": updated, "missing": missing});
            if !notes.is_empty() {
                reply["notes"] = json!(notes);
            }
            reply
        }
        Err(error) => failure(format!("update: {error}")),
    }
}

/// The numbers of `pairs` that are given, as params.
fn numbers<const N: usize>(pairs: [(&str, Option<f64>); N]) -> Map<String, Value> {
    pairs
        .into_iter()
        .filter_map(|(key, number)| Some((key.to_string(), json!(number?))))
        .collect()
}

/// Makes `change`, element `index` of an `update`, to `layer`: its size, then its place (so that
/// it ends with the top-left it was given), its text, its name (which wins over what the text
/// would name the layer) and its colours.
///
/// A place is in page coordinates, the ones `get` shows and `add` takes, whatever the layer is
/// inside of: `node.set`'s `x` and `y` are the parent's, and a layer put into a frame by its
/// `frameId` would land elsewhere.
fn change_layer(
    session: &mut Session,
    layer: NodeId,
    index: usize,
    change: &Change,
    notes: &mut Vec<String>,
) -> EngineResult<()> {
    let fields = &change.fields;
    let size = numbers([("width", fields.width), ("height", fields.height)]);
    if !size.is_empty() {
        session.execute("node.set", &json!({"ids": [layer], "props": size}))?;
    }
    let mut at = numbers([("x", fields.x), ("y", fields.y)]);
    if !at.is_empty() {
        at.insert("ids".into(), json!([layer]));
        session.execute("transform.moveTo", &Value::Object(at))?;
    }
    let appended = (fields.append.as_deref()).map(|more| {
        let doc = session.doc().ok();
        let now = doc.and_then(|doc| {
            let text = summary::text_layer(doc, layer)?;
            Some(doc.node(text)?.kind.text()?.characters.clone())
        });
        // After the `text` this change sets, if it sets one.
        let start = change
            .text()
            .map(str::to_string)
            .or(now)
            .unwrap_or_default();
        start + more
    });
    if let Some(text) = appended.as_deref().or(change.text())
        && !set_text(session, layer, text)?
    {
        let kind = summary::element_type(session.doc()?, layer);
        notes.push(format!(
            "elements[{index}]: `text` is ignored, because a layer of type {kind} has no text"
        ));
    }
    if let Some(name) = &fields.name {
        let props = json!({"name": name});
        session.execute("node.set", &json!({"ids": [layer], "props": props}))?;
    }
    if fields.background_color.is_some() || fields.stroke_color.is_some() {
        skeleton::set_colors(
            session,
            layer,
            fields.background_color.as_deref(),
            fields.stroke_color.as_deref(),
        )?;
    }
    Ok(())
}

/// Sets the text of `layer`: the label of a connector (an empty one removes it), the note of a
/// sticky, the text of a shape or of a text layer. `false` when the layer has no text to set.
fn set_text(session: &mut Session, layer: NodeId, text: &str) -> EngineResult<bool> {
    enum Place {
        Label,
        Layer(NodeId),
        Nowhere,
    }
    let place = {
        let doc = session.doc()?;
        match doc.node(layer).and_then(|node| node.board.as_ref()) {
            Some(BoardItem::Connector(_) | BoardItem::Label { .. }) => Place::Label,
            _ => summary::text_layer(doc, layer).map_or(Place::Nowhere, Place::Layer),
        }
    };
    match place {
        Place::Label => {
            session.execute("board.label", &json!({"id": layer, "text": text}))?;
        }
        Place::Layer(text_layer) => {
            let props = json!({"characters": text});
            session.execute("node.set", &json!({"ids": [text_layer], "props": props}))?;
        }
        Place::Nowhere => return Ok(false),
    }
    Ok(true)
}

pub(super) fn delete(board: &mut BoardDoc, op: &Value) -> Value {
    let ids = match id_list(op) {
        Ok(ids) => ids,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("delete: {error}")),
    };
    let (mut found, mut missing) = (Vec::new(), Vec::new());
    for id in ids {
        match layer_named(&doc, &id) {
            Some(layer) if !found.contains(&layer) => found.push(layer),
            Some(_) => {}
            None => missing.push(id),
        }
    }
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if !found.is_empty() {
        let selection = board.session.selection();
        if let Err(error) = board.execute("edit.delete", &json!({"ids": found})) {
            return failure(format!("delete: {error}"));
        }
        // The selection is the user's: what the deletion left of it stays selected.
        if !selection.is_empty() {
            board.session.set_selection(selection);
        }
    }
    json!({"ok": true, "deleted": found.len(), "missing": missing})
}

/// The `ids` of a `delete`, each an id as [`id_text`] reads it.
fn id_list(op: &Value) -> Result<Vec<String>, String> {
    let ids = op
        .get("ids")
        .and_then(Value::as_array)
        .ok_or(r#"it needs the ids to delete, like {"ids": ["a", "b"]}"#)?;
    ids.iter()
        .enumerate()
        .map(|(index, id)| id_text(id).ok_or_else(|| format!("ids[{index}] is not an id")))
        .collect()
}

pub(super) fn clear(board: &mut BoardDoc) -> Value {
    let layers = match page_layers(board) {
        Ok(layers) => layers,
        Err(error) => return failure(format!("clear: {error}")),
    };
    // Without layers the engine's delete takes the selection, so it isn't asked.
    if layers.is_empty() {
        return json!({"ok": true});
    }
    let cleared = crate::doc::grouped(&mut board.session, "Clear canvas", |session| {
        session.execute("edit.delete", &json!({"ids": layers}))
    });
    match cleared {
        Ok(_) => json!({"ok": true}),
        Err(error) => failure(format!("clear: {error}")),
    }
}
