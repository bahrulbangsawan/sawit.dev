//! Sawit's design standard applied with the work: the digest with a session's first UI work, the
//! sections later works name, the machine checks after a screen write and a work done, and the
//! guide as MCP resources and the design-ui prompt.

use rmcp::handler::server::wrapper::Parameters;

use super::work::begin;
use super::*;
use crate::app::canvas::design::{self, GUIDE_URI, PROMPT};
use crate::app::canvas::{CanvasProgressArgs, CanvasScreenWriteArgs};
use crate::app::tests::{answer, native};
use sawit_core::skills::bundled::DESIGN_DIGEST;

/// The texts of a tool's result, each block apart.
fn texts(result: &rmcp::model::CallToolResult) -> Vec<String> {
    let all = result.content.iter().filter_map(|c| c.as_text());
    all.map(|t| t.text.clone()).collect()
}

/// The first UI work of a session gets the digest, once; later works get the sections their steps
/// name, each once; a screen-sized frame drawn from shapes is told to be a screen.
#[tokio::test]
async fn the_standard_goes_with_the_work() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 1);
    let phone = json!({"x": 0, "y": 0, "width": 393, "height": 852});

    let mut first = begin(&project, "Settings", phone.clone());
    first.kind = Some("screen".into());
    let sent = texts(&server.canvas_begin(Parameters(first)).await);
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert_eq!(sent[1], DESIGN_DIGEST);

    let mut second = begin(&project, "Profile", phone.clone());
    second.kind = Some("screen".into());
    second.steps = Some(vec![
        "Laying out the form".into(),
        "Empty and error states".into(),
    ]);
    let sent = texts(&server.canvas_begin(Parameters(second)).await);
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(sent[1].starts_with("## Components"), "{}", sent[1]);
    assert!(sent[2].starts_with("## States"), "{}", sent[2]);
    assert!(
        !sent[1].contains("## Checklist"),
        "the digest has the checklist already"
    );

    let mut again = begin(&project, "Billing", phone.clone());
    again.kind = Some("screen".into());
    again.steps = Some(vec!["Form".into()]);
    let sent = texts(&server.canvas_begin(Parameters(again)).await);
    assert_eq!(sent.len(), 1, "a section goes once: {sent:?}");

    let vector = begin(&project, "Checkout / Mobile", phone);
    let begun = answer(&server.canvas_begin(Parameters(vector)).await);
    assert_eq!(begun["use_screen"], design::USE_A_SCREEN);
    // A component's own size isn't a screen's.
    let card = begin(
        &project,
        "Pricing card",
        json!({"x": 0, "y": 900, "width": 320, "height": 240}),
    );
    let begun = answer(&server.canvas_begin(Parameters(card)).await);
    assert!(begun.get("use_screen").is_none(), "{begun}");
}

/// Another session's first UI work gets the digest too, and a diagram or a routes work none.
#[test]
fn what_is_ui() {
    let size = |w: f64, h: f64| json!({"width": w, "height": h});
    assert!(design::is_ui(Some("screen"), "Thing", &json!({})));
    assert!(design::is_ui(None, "Thing", &size(1440.0, 1024.0)));
    assert!(design::is_ui(None, "Payment / Error", &size(100.0, 100.0)));
    assert!(design::is_ui(None, "Pricing card", &size(320.0, 240.0)));
    assert!(!design::is_ui(None, "Auth sequence", &size(320.0, 240.0)));
    assert!(!design::is_ui(
        Some("routes"),
        "Home page",
        &size(1440.0, 900.0)
    ));
    assert!(design::screen_like("Login", &size(10.0, 10.0)));
    assert!(!design::screen_like("Pricing card", &size(320.0, 240.0)));
}

/// A screen written with a low-contrast note, 10 px text and a 30 px button answers those three
/// failures, with their rule ids; a work said done answers its layers' checks.
#[tokio::test]
async fn writes_and_done_works_answer_the_checks() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let project = project_in(dir.path());
    write_board(&board_file(&project), 1);
    let mut args = begin(
        &project,
        "Account",
        json!({"x": 0, "y": 0, "width": 393, "height": 852}),
    );
    args.kind = Some("screen".into());
    let begun = answer(&server.canvas_begin(Parameters(args)).await);
    let page = r#"<!doctype html><html><head><style>
body { margin: 0; padding: 16px; color: #111827; background: #fff; }
.note { color: #aaaaaa; } .legal { font-size: 10px; }
.save { height: 30px; padding: 0 16px; font-size: 16px; color: #fff; background: #1d4ed8; border: 0; }
:focus-visible { outline: 2px solid #1d4ed8; outline-offset: 2px; }
</style></head><body><h1>Account</h1><p class="note">Saved here.</p><p class="legal">Terms.</p>
<button class="save">Save changes</button></body></html>"#;
    let write = CanvasScreenWriteArgs {
        agent: None,
        work: begun["work"].as_str().map(Into::into),
        screen: None,
        html: Some(page.into()),
        edits: None,
        files: None,
        file: None,
        tab: None,
        project: project_arg(&project),
    };
    let wrote = answer(&server.canvas_screen_write(Parameters(write)).await);
    let checks = &wrote["design_checks"];
    assert_eq!(checks["passed"], false, "{wrote}");
    let rules: Vec<&str> = checks["failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["rule"].as_str().unwrap())
        .collect();
    assert_eq!(rules, ["col.10", "typ.11", "a11y.2"]);
    assert_eq!(checks["failures"][0]["selector"], "p.note");

    let button = begin(
        &project,
        "Save button",
        json!({"x": 0, "y": 1000, "width": 120, "height": 30}),
    );
    let begun = answer(&server.canvas_begin(Parameters(button)).await);
    let done = CanvasProgressArgs {
        agent: None,
        work: begun["work"].as_str().unwrap().into(),
        step: None,
        status: Some("done".into()),
        message: None,
        file: None,
        tab: None,
        project: project_arg(&project),
    };
    let said = answer(&server.canvas_progress(Parameters(done)).await);
    let failures = said["design_checks"]["failures"]
        .as_array()
        .cloned()
        .unwrap();
    assert_eq!(failures.len(), 1, "{said}");
    assert_eq!(failures[0]["rule"], "a11y.2");
    assert_eq!(failures[0]["layer"], "Save button");
}

/// The guide is a resource whole and by section, and design-ui a prompt with the digest.
#[test]
fn the_guide_is_resources_and_a_prompt() {
    let resources = design::resources();
    let uris: Vec<&str> = resources.iter().map(|r| r.uri.as_str()).collect();
    assert_eq!(uris[0], GUIDE_URI);
    assert!(
        uris.contains(&"sawit://design-guide/canvas-craft"),
        "{uris:?}"
    );
    assert!(
        uris.contains(&"sawit://design-guide/accessibility"),
        "{uris:?}"
    );
    let read = |uri: &str| {
        let result = design::read(uri).unwrap();
        match &result.contents[0] {
            rmcp::model::ResourceContents::TextResourceContents { text, .. } => text.clone(),
            other => panic!("{other:?}"),
        }
    };
    assert!(read(GUIDE_URI).contains("## Checklist"));
    assert!(read("sawit://design-guide/typography").starts_with("## Typography"));
    assert!(design::read("sawit://design-guide/nothing").is_err());
    let prompt = design::prompt(PROMPT, Some("a settings page")).unwrap();
    let text = serde_json::to_string(&prompt.messages[0]).unwrap();
    assert!(
        text.contains("a settings page") && text.contains("canvas_begin"),
        "{text}"
    );
    assert!(design::prompt("other", None).is_err());
}

/// A canvas tool's `agent` goes with its op as its lane, and only when it is given.
#[test]
fn the_agent_argument_is_the_ops_lane() {
    let op = json!({"op": "add"});
    assert_eq!(crate::app::canvas::with_lane(op.clone(), None), op);
    assert_eq!(
        crate::app::canvas::with_lane(op.clone(), Some(&Value::Null)),
        op
    );
    let lane = json!({"id": "header", "name": "Header agent"});
    assert_eq!(
        crate::app::canvas::with_lane(op, Some(&lane)),
        json!({"op": "add", "lane": lane})
    );
}
