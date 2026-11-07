//! The outline written from the page's nodes, as the page wrote it before.

use serde_json::json;

use super::*;

fn nodes(v: serde_json::Value) -> Vec<Node> {
    serde_json::from_value(v).unwrap()
}

#[test]
fn writes_lines_as_the_page_did() {
    let n = nodes(json!([
        {"d": 0, "role": "banner", "ref": 1, "holds": true},
        {"d": 1, "role": "link", "name": "\"Home\"", "url": "/", "ref": 2},
        {"d": 2, "role": "button", "name": "\"Inner\"", "ref": 3},
        {"d": 0, "role": "main", "ref": 4, "holds": true},
        {"d": 1, "role": "heading", "name": "\"Title\"", "level": "1", "ref": 5},
        {"d": 1, "text": "\"Some text\""},
        {"d": 1, "role": "textbox", "name": "\"Email\"", "value": "\"a@b.co\"", "states": ["required", "focused"], "ref": 6},
        {"d": 1, "role": "list", "ref": 7, "holds": true},
        {"d": 1, "role": "checkbox", "states": ["checked", "disabled"], "ref": 8},
        {"d": 1, "role": "button", "states": ["collapsed"], "ref": 9}
    ]));
    assert_eq!(
        outline(&n, false),
        "- banner [ref=e1]:\n\
         \x20 - link \"Home\" /url: / [ref=e2]\n\
         \x20   - button \"Inner\" [ref=e3]\n\
         - main [ref=e4]:\n\
         \x20 - heading \"Title\" [level=1] [ref=e5]\n\
         \x20 - text: \"Some text\"\n\
         \x20 - textbox \"Email\" value=\"a@b.co\" [required] [focused] [ref=e6]\n\
         \x20 - list [ref=e7]\n\
         \x20 - checkbox [checked] [disabled] [ref=e8]\n\
         \x20 - button [collapsed] [ref=e9]"
    );
}

#[test]
fn a_cut_outline_says_so() {
    let n = nodes(json!([{"d": 0, "role": "main", "ref": 1, "holds": true}]));
    assert_eq!(
        outline(&n, true),
        "- main [ref=e1]\n… (cut at 2000 lines; scroll or use a selector)"
    );
    assert_eq!(outline(&[], false), "");
}

#[test]
fn the_answer_keeps_the_pages_fields() {
    let page = json!({
        "url": "http://x/", "title": "X", "ready": "complete", "refs": 1, "truncated": false,
        "nodes": [{"d": 0, "role": "button", "name": "\"Buy\"", "ref": 1}]
    });
    let answer = answer(page);
    assert_eq!(answer["snapshot"], "- button \"Buy\" [ref=e1]");
    assert!(answer.get("nodes").is_none());
    assert_eq!(answer["refs"], 1);
    // An answer without nodes (an older page script) passes as it came.
    let old = json!({"snapshot": "- x", "refs": 0});
    assert_eq!(super::answer(old.clone()), old);
}
