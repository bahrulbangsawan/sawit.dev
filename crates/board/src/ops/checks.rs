//! The `checks` op (`{work?, frame?, findings?}`): the design standard's machine checks of a
//! frame ([`crate::checks`]), answered as `{ok, frame, findings}`. A frame named by its `work`, or
//! by its id; with `findings` (a screen's, which its caller checks from its files) they are
//! answered as given, for the app to show on the frame. Nothing in the document changes.

use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::failure;
use super::work::{work_frame, work_of};
use crate::checks::{Finding, layers};
use crate::doc::BoardDoc;

pub(super) fn checks(board: &mut BoardDoc, op: &Value) -> Value {
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("checks: {error}")),
    };
    let frame = match work_of(op) {
        Ok(Some(work)) => work_frame(&doc, work),
        Ok(None) => op
            .get("frame")
            .and_then(Value::as_str)
            .and_then(|f| f.parse::<NodeId>().ok()),
        Err(error) => return failure(format!("checks: {error}")),
    };
    let Some(frame) = frame.filter(|f| doc.contains(*f)) else {
        return failure("checks: name a frame on the canvas, by its `work` or its id");
    };
    let findings: Vec<Finding> = match op.get("findings") {
        Some(given) => match serde_json::from_value(given.clone()) {
            Ok(findings) => findings,
            Err(error) => return failure(format!("checks: findings: {error}")),
        },
        None => layers::check(&doc, frame),
    };
    json!({"ok": true, "frame": frame.to_string(), "findings": findings})
}
