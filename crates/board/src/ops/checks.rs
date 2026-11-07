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

/// The `lint` op (`{work? | frame? | page?, sketch?}`, B12): the design lint of a frame (by its
/// `work` or its id), else of the page on show, answered as `{ok, scope, errors, warnings,
/// findings, status}`, `status` being where the file is in the design workflow. Nothing changes.
pub(super) fn lint(board: &mut BoardDoc, op: &Value) -> Value {
    let doc = match board.doc() {
        Ok(doc) => doc,
        Err(error) => return failure(format!("lint: {error}")),
    };
    let named = match work_of(op) {
        Ok(Some(work)) => match work_frame(&doc, work) {
            Some(frame) => Some(frame),
            None => return failure(format!("lint: no work {work} on the canvas")),
        },
        Ok(None) => op
            .get("frame")
            .or_else(|| op.get("id"))
            .and_then(|v| {
                v.as_str()
                    .map(str::to_string)
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            })
            .and_then(|f| f.parse::<NodeId>().ok()),
        Err(error) => return failure(format!("lint: {error}")),
    };
    let page = board.session.ds().ok().map(|ds| ds.page);
    let Some(scope) = named.or(page).filter(|s| doc.contains(*s)) else {
        return failure("lint: name a frame on the canvas, by its `work` or its id");
    };
    let sketch = op.get("sketch").and_then(Value::as_bool).unwrap_or(false);
    let found = crate::checks::lint::lint(&doc, scope, sketch);
    let mut reply = serde_json::to_value(&found).unwrap_or_else(|_| json!({}));
    reply["ok"] = json!(true);
    reply["scope"] = json!(scope.to_string());
    reply["status"] = crate::checks::lint::design_status(&doc);
    reply
}

/// The `design_status` op: where the file is in the design workflow ([`crate::checks::lint::design_status`]).
pub(super) fn design_status(board: &mut BoardDoc) -> Value {
    match board.doc() {
        Ok(doc) => {
            let mut reply = crate::checks::lint::design_status(&doc);
            reply["ok"] = json!(true);
            reply
        }
        Err(error) => failure(format!("design_status: {error}")),
    }
}
