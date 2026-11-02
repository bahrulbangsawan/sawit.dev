//! The agents' tests: names and ids, colours and labels, presence, and the words for their ops.

use std::time::Duration;

use serde_json::{Value, json};

use super::*;

fn agent(id: &str, name: &str, project: Option<&str>) -> AgentIdentity {
    AgentIdentity {
        id: id.into(),
        name: name.into(),
        label: project.map(Into::into),
        version: None,
    }
}

const fn secs(s: u64) -> Duration {
    Duration::from_secs(s)
}

/// The clients people use are called what people call them; others get a readable name.
#[test]
fn clients_are_called_what_people_call_them() {
    assert_eq!(client_name("claude-code", None), "Claude Code");
    assert_eq!(client_name("codex-mcp-client", Some("Codex")), "Codex");
    assert_eq!(client_name("cursor-vscode", None), "Cursor");
    assert_eq!(client_name("Visual Studio Code", None), "VS Code");
    assert_eq!(client_name("gemini-cli-mcp-client", None), "Gemini CLI");
    assert_eq!(client_name("acme-agent-mcp-client", None), "Acme Agent");
    assert_eq!(client_name("my_tool", Some("  My Tool  ")), "My Tool");
    assert_eq!(client_name("", None), "Agent");
    assert_eq!(client_name("--", None), "Agent");
    let long = client_name(&"x".repeat(80), None);
    assert_eq!(long.chars().count(), MAX_NAME);
}

#[test]
fn a_session_id_is_the_process_and_when_it_started() {
    assert_eq!(session_id(4821, 0x1_2345_6789), "mcp-4821-23456789");
    assert_ne!(session_id(4821, 1), session_id(4821, 2));
}

/// Each agent keeps the colour and label it was given when first met, a second agent never
/// renames the first, and agents of one client are told apart by project, else by number.
#[test]
fn colours_and_labels_are_given_once_and_duplicates_are_numbered() {
    let mut roster = Roster::default();
    let first = roster
        .meet(&agent("a", "Claude Code", Some("shop")), secs(0))
        .clone();
    assert_eq!((first.label.as_str(), first.color), ("Claude Code", 0));
    let codex = roster
        .meet(&agent("b", "Codex", Some("shop")), secs(1))
        .clone();
    assert_eq!((codex.label.as_str(), codex.color), ("Codex", 1));
    let blog = roster
        .meet(&agent("c", "Claude Code", Some("blog")), secs(2))
        .clone();
    assert_eq!(blog.label, "Claude Code · blog");
    let again = roster
        .meet(&agent("d", "Claude Code", Some("shop")), secs(3))
        .clone();
    assert_eq!(again.label, "Claude Code 3");
    let unnamed = roster
        .meet(&agent("e", "Claude Code", None), secs(4))
        .clone();
    assert_eq!(unnamed.label, "Claude Code 4");
    // Met again, each is as it was: the same colour and the same label.
    for known in [&first, &codex, &blog, &again, &unnamed] {
        let met = roster.meet(&known.identity, secs(5));
        assert_eq!((&met.label, met.color), (&known.label, known.color));
    }
    let colours: Vec<usize> = roster.all().iter().map(|k| k.color).collect();
    assert_eq!(
        colours,
        [0, 1, 2, 3, 4],
        "five agents at once, five colours"
    );
}

/// A colour comes free once its agent has gone quiet for [`GONE_AFTER`], and when all are in
/// use the next one in turn is shared.
#[test]
fn colours_are_reused_once_free() {
    let mut roster = Roster::default();
    for (i, id) in ["a", "b", "c", "d", "e", "f", "g", "h"].iter().enumerate() {
        assert_eq!(roster.meet(&agent(id, id, None), secs(0)).color, i);
    }
    assert_eq!(roster.meet(&agent("i", "i", None), secs(1)).color, 0);
    // `a` to `h` went quiet; `i` (colour 0) too, but `j` meets it at 31 s, and `i` at 1 s.
    roster.meet(&agent("b", "b", None), secs(29));
    let later = roster.meet(&agent("j", "j", None), secs(31)).color;
    assert_eq!(later, 0, "the first colour no recent agent has");
}

#[test]
fn the_anonymous_agent_is_one_agent() {
    let mut roster = Roster::default();
    let one = roster.meet(&AgentIdentity::anonymous(), secs(0)).clone();
    let two = roster.meet(&AgentIdentity::anonymous(), secs(1)).clone();
    assert_eq!(one.color, two.color);
    assert_eq!(roster.all().len(), 1);
    assert_eq!(one.subject(), "An agent");
    let named = roster.meet(&agent("x", "Codex", None), secs(1)).clone();
    assert_eq!(named.subject(), "Codex");
}

/// Presence is whole for a few seconds, fades, stays faded, and is gone after 30 s.
#[test]
fn presence_fades_then_expires() {
    assert_eq!(opacity(secs(0)), Some(1.0));
    assert_eq!(opacity(FADE_AFTER), Some(1.0));
    let half = opacity(FADE_AFTER + FADE_TIME / 2).unwrap();
    assert!(half < 1.0 && half > FADED, "{half}");
    assert_eq!(opacity(FADE_AFTER + FADE_TIME), Some(FADED));
    assert_eq!(opacity(secs(29)), Some(FADED));
    assert_eq!(opacity(GONE_AFTER), None);
    assert_eq!(opacity(secs(600)), None);
    assert!(shows_action(secs(2)) && !shows_action(secs(10)));
}

#[test]
fn glides_ease_out_and_end() {
    let total = Duration::from_millis(200);
    assert_eq!(glide(Duration::ZERO, total), 0.0);
    let half = glide(total / 2, total);
    assert!(half > 0.5, "eased out: {half}");
    assert_eq!(glide(total, total), 1.0);
    assert_eq!(glide(Duration::ZERO, Duration::ZERO), 1.0, "reduced motion");
}

#[test]
fn canvas_ops_say_what_they_do() {
    let doing = |op: Value| canvas_action(&op).doing;
    let stickies = json!({"op": "add", "elements": [
            {"type": "sticky"}, {"type": "sticky"}, {"type": "sticky"}]});
    assert_eq!(doing(stickies.clone()), "adding 3 stickies");
    assert_eq!(canvas_action(&stickies).done, "added 3 stickies");
    assert_eq!(
        doing(json!({"op": "add", "elements": [{"type": "rectangle"}]})),
        "adding a shape"
    );
    assert_eq!(
        doing(json!({"op": "add", "elements": [{"type": "image"}]})),
        "adding an image"
    );
    assert_eq!(
        doing(json!({"op": "add", "elements": [{"type": "text"}, {"type": "arrow"}]})),
        "adding 2 elements"
    );
    assert_eq!(
        doing(json!({"op": "update", "elements": [{"id": "a", "x": 1}, {"id": "b", "y": 2}]})),
        "moving 2 layers"
    );
    assert_eq!(
        doing(json!({"op": "update", "elements": [{"id": "a", "text": "Hi"}]})),
        "editing text"
    );
    assert_eq!(
        doing(json!({"op": "delete", "ids": ["a"]})),
        "deleting a layer"
    );
    assert_eq!(
        doing(json!({"op": "mermaid", "source": "graph TD"})),
        "drawing a diagram"
    );
    assert_eq!(
        doing(json!({"op": "command", "command": "align.left"})),
        "running align.left"
    );
    assert_eq!(
        doing(json!({"op": "command", "command": "command.batch", "params": {"steps": [{}, {}]}})),
        "running 2 steps"
    );
    assert_eq!(doing(json!({"op": "nope"})), "working");
}

/// A page an agent opens is named by its path, or its host at the root.
#[test]
fn browser_ops_say_what_they_do() {
    use crate::browser::BrowserOp;
    assert_eq!(
        page_name("http://localhost:5173/checkout?step=2"),
        "/checkout"
    );
    assert_eq!(page_name("https://example.com/"), "example.com");
    assert_eq!(page_name("about:blank"), "about:blank");
    let open = browser_action(&BrowserOp::Navigate {
        url: "http://localhost:5173/checkout".into(),
    });
    assert_eq!(open.doing, "opening /checkout");
    let press = browser_action(&BrowserOp::Press {
        key: "Enter".into(),
    });
    assert_eq!(press.done, "pressed Enter");
    assert_eq!(
        browser_action(&BrowserOp::Snapshot).doing,
        "reading the page"
    );
    assert!(browser_op_changes_the_page(&BrowserOp::Press {
        key: "a".into()
    }));
    assert!(!browser_op_changes_the_page(&BrowserOp::State));
    assert!(!browser_op_changes_the_page(&BrowserOp::Snapshot));
}

#[test]
fn slow_ops_are_the_ones_that_take_time() {
    assert!(slow_canvas_op(
        &json!({"op": "mermaid", "source": "graph TD"})
    ));
    assert!(slow_canvas_op(
        &json!({"op": "replace", "path": "/a.sawit"})
    ));
    assert!(slow_canvas_op(
        &json!({"op": "command", "command": "image.place"})
    ));
    let steps: Vec<Value> = (0..SLOW_BATCH).map(|_| json!({})).collect();
    assert!(slow_canvas_op(
        &json!({"op": "command", "command": "command.batch", "params": {"steps": steps}})
    ));
    assert!(!slow_canvas_op(
        &json!({"op": "command", "command": "command.batch", "params": {"steps": [{}]}})
    ));
    assert!(slow_canvas_op(
        &json!({"op": "add", "elements": [{"type": "image"}]})
    ));
    assert!(!slow_canvas_op(
        &json!({"op": "add", "elements": [{"type": "sticky"}]})
    ));
    assert!(!slow_canvas_op(&json!({"op": "get"})));
}

/// The identity goes on the wire as `{id, name, label}`, the version only when known.
#[test]
fn identities_round_trip() {
    let identity = agent("mcp-1-2", "Codex", Some("shop"));
    let json = serde_json::to_value(&identity).unwrap();
    assert_eq!(
        json,
        json!({"id": "mcp-1-2", "name": "Codex", "label": "shop"})
    );
    assert_eq!(
        serde_json::from_value::<AgentIdentity>(json).unwrap(),
        identity
    );
    let bare: AgentIdentity = serde_json::from_value(json!({"id": "x", "name": "Y"})).unwrap();
    assert_eq!(bare.label, None);
}
