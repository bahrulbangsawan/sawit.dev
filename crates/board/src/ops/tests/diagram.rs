//! The `mermaid` op.

use std::sync::Arc;

use serde_json::json;

use super::*;
use crate::ops::diagram::second_try;

/// SMOKE-3: a Mermaid diagram drawn as shapes and connectors doesn't go on a design file, and
/// the refusal says why in Sawit's words, not the engine's for one of the diagram's own
/// elements. A chart, which comes in as one picture, still does.
#[test]
fn mermaid_on_a_design_file_says_why() {
    let mut design = design();
    let before = design.doc().unwrap();
    let error = refusal(
        &mut design,
        json!({"op": "mermaid", "source": "graph TD; A-->B; B-->C"}),
    );
    assert_eq!(
        error,
        "mermaid: this canvas is a design file, and a diagram's shapes and connectors are for \
             boards: bring the diagram in as an SVG file with the command op's file.import instead"
    );
    assert!(Arc::ptr_eq(&before, &design.doc().unwrap()));
    let pie = "pie title Pets\n \"Dogs\" : 386\n \"Cats\" : 85";
    ask(&mut design, json!({"op": "mermaid", "source": pie}));
    assert_eq!(elements(&mut design).len(), 1);
}

/// CANVAS-CORE-10: a Mermaid source with double quotes that fails is tried once more with
/// single ones, but not after its render was stopped for running past the time limit: the
/// second try froze the window as long again, 10 s in all.
#[test]
fn a_mermaid_render_stopped_for_time_is_not_tried_again() {
    let quoted = r#"graph TD; A["a"] --> B"#;
    assert_eq!(
        second_try(quoted, "Parse error on line 1").as_deref(),
        Some("graph TD; A['a'] --> B")
    );
    assert_eq!(
        second_try("graph TD; A --> B", "Parse error on line 1"),
        None
    );
    assert_eq!(
        second_try(quoted, "The diagram can't be drawn: it took too long."),
        None
    );
}

/// A `mermaid` op made ready off the board's thread ([`prepare_diagram`]) draws and answers as
/// the op does when run at once ([`run`]), its errors and its refusals too; no other op is
/// made ready. What it carries between threads can be sent there.
#[test]
fn a_diagram_made_ready_elsewhere_draws_as_the_op_does() {
    fn sendable<T: Send + 'static>(_: &T) {}
    let source = "flowchart LR\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]\n";
    let op = json!({"op": "mermaid", "source": source, "x": 10, "y": 20});
    let (mut now, mut later) = (board(), board());
    let ran = run(&mut now, &op);
    let prepared = prepare_diagram(&op).expect("a mermaid op is made ready");
    sendable(&prepared);
    let drawn = run_prepared(&mut later, prepared);
    assert_eq!(ran["ok"], json!(true), "{ran}");
    assert_eq!(drawn["added"], ran["added"], "{drawn}");
    assert_eq!(elements(&mut now).len(), elements(&mut later).len());
    assert_eq!(later.session.undo_depth(), (1, 0), "one undo step");
    for bad in [
        json!({"op": "mermaid", "source": "flowchart LR\n  A[unclosed --> B"}),
        json!({"op": "mermaid", "source": " "}),
        json!({"op": "mermaid", "source": "zenuml\n  A->B: hi"}),
    ] {
        let ran = run(&mut board(), &bad);
        let drawn = run_prepared(&mut board(), prepare_diagram(&bad).unwrap());
        assert_eq!(drawn, ran, "{bad}");
        assert_eq!(drawn["ok"], json!(false), "{drawn}");
    }
    assert!(prepare_diagram(&json!({"op": "get"})).is_none());
    assert!(prepare_diagram(&json!({"op": "add", "elements": []})).is_none());
}
