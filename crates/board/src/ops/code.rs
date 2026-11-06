//! The `frame_code` op (`{id, format, component?, css_file?}`): an artboard as code, Export HTML
//! and Export React's ([`crate::frame_code`]). `id` names the frame by its id or its name (the
//! page on show first); `format` is `html` (the default) or `react`. Answers `{ok, frame, name,
//! format, html}` or `{ok, frame, name, format, component, tsx, css, css_file}`, with `notes` on
//! what the conversion left out. A screen is no artboard: its code is its own HTML, which the
//! caller exports from its folder. Nothing in the document changes.

use serde_json::{Value, json};

use super::failure;
use crate::doc::BoardDoc;
use crate::frame_code::{find_frame, to_html, to_react};
use crate::screens;

pub(super) fn frame_code(board: &mut BoardDoc, op: &Value) -> Value {
    let (doc, page) = match board.session.ds() {
        Ok(ds) => (ds.doc.clone(), ds.page),
        Err(error) => return failure(format!("frame_code: {error}")),
    };
    let wanted = op.get("id").and_then(Value::as_str).unwrap_or_default();
    let Some(frame) = find_frame(&doc, Some(page), wanted) else {
        return failure(format!(
            "frame_code: no frame “{wanted}” on the canvas: give its id or name"
        ));
    };
    let node = doc.node(frame).expect("a frame found");
    if let Some(screen) = screens::screen_of(node) {
        return failure(format!(
            "frame_code: “{}” is a screen (“{}”): export it as one",
            node.name, screen.screen
        ));
    }
    let name = node.name.clone();
    let format = op
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or("html")
        .trim()
        .to_lowercase();
    match format.as_str() {
        "" | "html" => match to_html(board, frame) {
            Ok((html, notes)) => json!({
                "ok": true, "frame": frame.to_string(), "name": name, "format": "html",
                "html": html, "notes": notes,
            }),
            Err(error) => failure(format!("frame_code: {}", error.for_user())),
        },
        "react" | "tsx" => {
            let component = op.get("component").and_then(Value::as_str);
            let component = component
                .map(str::to_string)
                .unwrap_or_else(|| screens::component_name(&name));
            let css_file = op
                .get("css_file")
                .and_then(Value::as_str)
                .map_or_else(|| format!("{component}.module.css"), str::to_string);
            match to_react(board, frame, Some(&component), &css_file) {
                Ok(code) => json!({
                    "ok": true, "frame": frame.to_string(), "name": name, "format": "react",
                    "component": code.component, "tsx": code.tsx, "css": code.css,
                    "css_file": css_file, "notes": code.notes,
                }),
                Err(error) => failure(format!("frame_code: {}", error.for_user())),
            }
        }
        other => failure(format!(
            "frame_code: unknown format “{other}”: html or react"
        )),
    }
}
