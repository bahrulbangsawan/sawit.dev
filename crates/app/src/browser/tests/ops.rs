//! Calls into the page agent and its answers.

use serde_json::json;

use crate::browser::ops::{agent_call, agent_result, rect_op};
use sawit_core::browser::Target;

#[test]
fn agent_answers() {
    let wire = serde_json::to_string(r#"{"ok":true,"n":1}"#).unwrap();
    assert_eq!(agent_result(&wire), Ok(json!({"n": 1})));
    let wire = serde_json::to_string(r#"{"ok":false,"error":"No element e9."}"#).unwrap();
    assert_eq!(agent_result(&wire), Err("No element e9.".into()));
    for nothing in ["", "\"\"", "null"] {
        assert!(agent_result(nothing).unwrap_err().contains("isn't ready"));
    }
}

#[test]
fn agent_calls_embed_the_op() {
    let call = agent_call(&json!({"op": "click", "ref": "e2"}));
    assert!(call.contains(r#"a.run({"op":"click","ref":"e2"})"#));
    let op = rect_op(&Target {
        reference: Some("e5".into()),
        selector: None,
    });
    assert_eq!(op, json!({"ref": "e5", "op": "rect"}));
}
