//! The `handoff` op (`{ids? | work? | screen?, dir, pictures?}`): Send to Agent's files for the
//! frames named, written into the handoff folder beside the canvas's screens folder `dir`
//! ([`crate::handoff`]). `ids` are frames by id or name (several hand off as a flow, in canvas
//! order); `work` the frame a work made; `screen` a screen by name. `pictures` (from the app) are
//! the screens' latest pictures, `[{frame, png_base64}]`. Answers `{ok, name, folder, design,
//! spec, prompt, prompt_file, files}`. The document doesn't change.

use std::path::PathBuf;

use base64::Engine as _;
use figcraft_doc::NodeId;
use serde_json::{Value, json};

use super::failure;
use super::work::{work_frame, work_of};
use crate::doc::BoardDoc;
use crate::frame_code::find_frame;
use crate::handoff::{folder, name, targets, write};
use crate::screens;

pub(super) fn handoff(board: &mut BoardDoc, op: &Value) -> Value {
    let (doc, page) = match board.session.ds() {
        Ok(ds) => (ds.doc.clone(), ds.page),
        Err(error) => return failure(format!("handoff: {error}")),
    };
    let Some(dir) = op.get("dir").and_then(Value::as_str).map(PathBuf::from) else {
        return failure("handoff: the canvas's screens folder (`dir`) is missing");
    };
    let mut ids: Vec<NodeId> = Vec::new();
    match work_of(op) {
        Ok(Some(work)) => ids.extend(work_frame(&doc, work)),
        Ok(None) => {}
        Err(error) => return failure(format!("handoff: {error}")),
    }
    if let Some(screen) = op
        .get("screen")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
    {
        match screens::find(&doc, screen.trim()) {
            Some(found) => ids.push(found.frame),
            None => return failure(format!("handoff: no screen “{screen}”")),
        }
    }
    let named: Vec<String> = match op.get("ids") {
        Some(Value::String(one)) => vec![one.clone()],
        Some(Value::Array(all)) => all
            .iter()
            .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    for wanted in named {
        match find_frame(&doc, Some(page), &wanted) {
            Some(id) => ids.push(id),
            None => return failure(format!("handoff: no frame “{wanted}” on the canvas")),
        }
    }
    let picked = match targets(&doc, &ids) {
        Ok(picked) => picked,
        Err(error) => return failure(format!("handoff: {error}")),
    };
    let pictures: Vec<(NodeId, Vec<u8>)> = op
        .get("pictures")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let frame = p.get("frame")?.as_str()?.parse().ok()?;
            let png = base64::engine::general_purpose::STANDARD
                .decode(p.get("png_base64")?.as_str()?)
                .ok()?;
            Some((frame, png))
        })
        .collect();
    let out = folder(&dir, &name(&picked));
    match write(board, &picked, &dir, &out, &pictures) {
        Ok(done) => json!({
            "ok": true, "name": done.name, "folder": done.folder, "design": done.design,
            "spec": done.spec, "prompt": done.prompt, "prompt_file": done.prompt_file,
            "files": done.files,
            "frames": picked.iter().map(|t| json!({"id": t.frame.to_string(), "name": t.title, "screen": t.screen})).collect::<Vec<_>>(),
        }),
        Err(error) => failure(format!("handoff: {error}")),
    }
}
