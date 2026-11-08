//! `canvas_preview`: Preview and Present for agents, as the user drives them from a screen's bar.
//! Preview shows a screen full size and live; it works as a prototype (switches flip, tabs select,
//! a link to another screen walks there), and an agent can walk it the same way and read where it
//! is. Each action is the canvas's `web` op; Preview opens at the canvas's next draw, so the tool
//! reads its state back a moment later.

use std::time::Duration;

use rmcp::model::CallToolResult;
use serde_json::{Value, json};

use super::CanvasPreviewArgs;
use crate::app::pretty;
use crate::{Sawit, fail, ok};

/// How long Preview takes to open or walk (its page builds at the canvas's next draw).
const SETTLE: Duration = Duration::from_millis(700);

/// The canvas's `web` op for `a`, or why there is none.
pub(super) fn preview_op(a: &CanvasPreviewArgs) -> Result<Value, String> {
    let action = a.action.trim().to_lowercase();
    let needs_screen = matches!(action.as_str(), "open" | "walk" | "present");
    let screen = a.screen.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if needs_screen && screen.is_none() {
        return Err(format!(
            "{action} needs `screen`, a screen's folder name (canvas_screen_get lists them)."
        ));
    }
    let web = |what: &str| json!({"op": "web", "action": what, "screen": screen});
    Ok(match action.as_str() {
        "open" => web("preview"),
        "walk" => web("walk"),
        "back" => web("back"),
        "state" => web("preview_state"),
        "close" => web("close_preview"),
        "present" => web("present"),
        "close_present" => web("close_present"),
        "set" => {
            if a.size.is_none()
                && a.zoom.is_none()
                && a.device_frame.is_none()
                && a.rotated.is_none()
            {
                return Err("set needs size, zoom, device_frame or rotated.".into());
            }
            json!({"op": "web", "action": "preview_set", "size": a.size, "zoom": a.zoom,
                   "frame": a.device_frame, "rotated": a.rotated})
        }
        other => {
            return Err(format!(
                "Unknown action “{other}”: use open, walk, back, set, state, close, present or close_present."
            ));
        }
    })
}

/// The `web` op's answer as a result: its `result`, or its `error` as a failure.
fn answer(v: Value) -> Result<Value, String> {
    match v.get("ok").and_then(Value::as_bool) {
        Some(false) => Err(v["error"]
            .as_str()
            .unwrap_or("The canvas refused it")
            .to_string()),
        _ => Ok(v.get("result").cloned().unwrap_or(v)),
    }
}

impl Sawit {
    pub async fn canvas_preview_tool(&self, a: CanvasPreviewArgs) -> CallToolResult {
        let op = match preview_op(&a) {
            Ok(op) => op,
            Err(e) => return fail(e),
        };
        let action = a.action.trim().to_lowercase();
        let done = match self
            .canvas(a.project.clone(), a.file.clone(), op)
            .await
            .and_then(answer)
        {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        if !matches!(action.as_str(), "open" | "walk" | "back" | "set" | "close") {
            return ok(pretty(&json!({"action": action, "result": done})));
        }
        // What Preview shows now: it opens and walks at the canvas's next draw.
        tokio::time::sleep(SETTLE).await;
        let state = self
            .canvas(
                a.project,
                a.file,
                json!({"op": "web", "action": "preview_state"}),
            )
            .await
            .and_then(answer)
            .unwrap_or(Value::Null);
        let mut reply = json!({"action": action, "result": done, "preview": state});
        if state.is_null() && action != "close" {
            reply["note"] = json!(
                "Preview opens once the canvas is on screen: it shows when the user's window has the canvas's tab in front."
            );
        }
        ok(pretty(&reply))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> CanvasPreviewArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn each_action_is_its_web_op() {
        let op = preview_op(&args(json!({"action": "open", "screen": "cards"}))).unwrap();
        assert_eq!(
            op,
            json!({"op": "web", "action": "preview", "screen": "cards"})
        );
        let op = preview_op(&args(json!({"action": "walk", "screen": "home"}))).unwrap();
        assert_eq!(op["action"], "walk");
        let op = preview_op(&args(json!({"action": "back"}))).unwrap();
        assert_eq!(op["action"], "back");
        let op = preview_op(&args(json!({"action": "set", "zoom": "100%"}))).unwrap();
        assert_eq!(op["action"], "preview_set");
        assert_eq!(op["zoom"], "100%");
    }

    #[test]
    fn a_missing_screen_or_an_unknown_action_says_what_to_pass() {
        let e = preview_op(&args(json!({"action": "open"}))).unwrap_err();
        assert!(e.contains("needs `screen`"), "{e}");
        let e = preview_op(&args(json!({"action": "set"}))).unwrap_err();
        assert!(e.contains("size, zoom"), "{e}");
        let e = preview_op(&args(json!({"action": "fly"}))).unwrap_err();
        assert!(e.contains("open, walk, back"), "{e}");
    }
}
