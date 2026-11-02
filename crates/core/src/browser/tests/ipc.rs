//! What the app reads of a page's IPC messages, and who may send them.

use crate::browser::{AGENT_JS, ConsoleEntry, PageEvent, page_script, parse_ipc};

#[test]
fn ipc_needs_the_token() {
    let body = "t0k\nconsole\n{\"level\":\"error\",\"text\":\"boom\",\"time_ms\":3}";
    assert_eq!(
        parse_ipc(body, "t0k"),
        Some(PageEvent::Console(ConsoleEntry {
            level: "error".into(),
            text: "boom".into(),
            url: None,
            line: None,
            time_ms: 3.,
        }))
    );
    assert_eq!(parse_ipc(body, "other"), None);
    assert_eq!(parse_ipc("console\n{}", ""), None);
    assert_eq!(
        parse_ipc("t0k\nurl\n\"http://a/#b\"", "t0k"),
        Some(PageEvent::Url("http://a/#b".into()))
    );
    assert_eq!(parse_ipc("t0k\nnope\n{}", "t0k"), None);
}

#[test]
fn ipc_reads_picks_hovers_and_keys() {
    let picked = parse_ipc(
        "tok\npicked\n{\"mode\":\"annotate\",\"element\":{\"selector\":\"#a\"}}",
        "tok",
    );
    assert!(matches!(picked, Some(PageEvent::Picked(v)) if v["element"]["selector"] == "#a"));
    let hovered = parse_ipc("tok\nhovered\n{\"selector\":\"#a\"}", "tok");
    assert!(matches!(hovered, Some(PageEvent::Hovered(v)) if v["selector"] == "#a"));
    assert_eq!(
        parse_ipc("tok\nkey\n\"escape\"", "tok"),
        Some(PageEvent::Key("escape".into()))
    );
    assert_eq!(parse_ipc("bad\nkey\n\"escape\"", "tok"), None);
    let changed = parse_ipc("tok\nannotation\n{\"action\":\"remove\",\"n\":2}", "tok");
    assert!(matches!(changed, Some(PageEvent::Annotation(v)) if v["n"] == 2));
    assert_eq!(
        parse_ipc("bad\nannotation\n{\"action\":\"remove\",\"n\":2}", "tok"),
        None
    );
}

#[test]
fn ipc_reads_a_press_in_the_page() {
    assert_eq!(
        parse_ipc("tok\npressed\ntrue", "tok"),
        Some(PageEvent::Pressed)
    );
    assert_eq!(parse_ipc("bad\npressed\ntrue", "tok"), None);
    // The page agent sends it for the user's press only, before the page's own listeners.
    // The agent's own clicks (`browser_click`) and a page script's are synthetic
    // (`isTrusted` false): they give the page no keyboard, so they mustn't move GPUI's.
    let start = AGENT_JS
        .find(r#"addEventListener("pointerdown""#)
        .expect("the agent's press listener");
    let listener = &AGENT_JS[start..];
    let listener = &listener[..listener.find('\n').unwrap_or(listener.len())];
    assert!(
        listener.contains(r#"if (e.isTrusted) send("pressed", true);"#),
        "{listener}"
    );
    assert!(listener.contains("capture: true"), "{listener}");
    // Nothing else in the page script sends it.
    assert_eq!(
        page_script("tok").matches(r#""pressed""#).count(),
        1,
        "another sender of \"pressed\""
    );
}
