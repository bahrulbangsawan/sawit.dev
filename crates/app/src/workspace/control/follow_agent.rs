//! Where an agent's canvas work is seen: an agent that starts drawing on a canvas the user doesn't
//! have on screen brings that canvas's tab forward, so its cursor and what it builds show as they
//! happen, rather than in a tab the user has to find and open. Only the first change of a burst of
//! work does it (none by that agent on that canvas for [`BURST_GAP`]): a user who goes back to the
//! tab they were in isn't pulled away again by each op that follows. Reads never do.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{App, Global};
use serde_json::Value;

/// A pause this long between an agent's changes to a canvas starts a new burst of work.
pub const BURST_GAP: Duration = Duration::from_secs(10 * 60);

/// When each agent last changed each canvas: by agent id and the canvas's key (its file, or its
/// project's).
#[derive(Default)]
struct LastChange(HashMap<(String, String), Instant>);

impl Global for LastChange {}

/// Whether canvas op `op` adds to, changes or draws on the canvas, as an agent at work does, or
/// opens Preview or Present on it; reading it, checking it or exporting it changes nothing on
/// screen.
pub fn changes_canvas(op: &Value) -> bool {
    // Preview and Present an agent opens or walks show on the canvas, for the user to see.
    let shows = op.get("op").and_then(Value::as_str) == Some("web")
        && matches!(
            op.get("action").and_then(Value::as_str),
            Some("preview" | "walk" | "present")
        );
    shows
        || matches!(
            op.get("op").and_then(Value::as_str),
            Some(
                "add"
                    | "update"
                    | "delete"
                    | "clear"
                    | "replace"
                    | "mermaid"
                    | "excalidraw"
                    | "begin"
                    | "progress"
                    | "screen"
                    | "screen_layers"
                    | "starter"
                    | "command"
                    | "routes"
                    | "pointer"
                    | "key"
                    | "type"
            )
        )
}

/// Whether agent `agent`'s change now to the canvas `canvas` starts a burst of work, and so
/// brings the canvas forward; it is noted either way.
pub fn starts_burst(agent: &str, canvas: &str, now: Instant, cx: &mut App) -> bool {
    let last = cx.default_global::<LastChange>();
    let key = (agent.to_string(), canvas.to_string());
    let starts = last
        .0
        .get(&key)
        .is_none_or(|at| now.saturating_duration_since(*at) >= BURST_GAP);
    last.0.insert(key, now);
    starts
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn writes_bring_the_canvas_forward_and_reads_do_not() {
        for op in ["add", "screen", "begin", "command", "mermaid"] {
            assert!(changes_canvas(&json!({"op": op})), "{op}");
        }
        for op in [
            "get",
            "commands",
            "export_png",
            "checks",
            "lint",
            "web",
            "handoff",
        ] {
            assert!(!changes_canvas(&json!({"op": op})), "{op}");
        }
    }

    #[gpui_kit::test]
    fn only_the_first_change_of_a_burst_brings_it_forward(cx: &mut gpui_kit::TestAppContext) {
        let t0 = Instant::now();
        cx.update(|cx| {
            assert!(starts_burst("a", "board", t0, cx));
            assert!(!starts_burst("a", "board", t0 + Duration::from_secs(5), cx));
            assert!(starts_burst("b", "board", t0, cx), "another agent");
            assert!(starts_burst("a", "other", t0, cx), "another canvas");
            let later = t0 + Duration::from_secs(5) + BURST_GAP;
            assert!(starts_burst("a", "board", later, cx), "after a long pause");
        });
    }
}
