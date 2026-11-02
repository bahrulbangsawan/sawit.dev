//! The console and network logs: the ring, the filters, the page's errors.

use super::request;
use crate::browser::{ConsoleEntry, Logs, MAX_CONSOLE};

fn console(level: &str, text: &str) -> ConsoleEntry {
    ConsoleEntry {
        level: level.into(),
        text: text.into(),
        url: None,
        line: None,
        time_ms: 0.,
    }
}

#[test]
fn page_errors_since_navigation() {
    let mut logs = Logs::default();
    logs.push_console(console("error", "old"));
    logs.push_console(console("info", "— navigated to http://x/"));
    for i in 0..25 {
        logs.push_console(console("error", &format!("e{i}")));
    }
    let errors = logs.page_errors(20);
    assert_eq!(errors.len(), 20);
    assert_eq!(errors[0].text, "e5");
    assert_eq!(errors[19].text, "e24");
}

#[test]
fn console_ring_and_filters() {
    let mut logs = Logs::default();
    for i in 0..MAX_CONSOLE + 5 {
        logs.push_console(console("log", &i.to_string()));
    }
    assert_eq!(logs.console.len(), MAX_CONSOLE);
    assert_eq!(logs.console.front().unwrap().text, "5");
    logs.push_console(console("error", "TypeError: x is undefined"));
    logs.push_console(console("warning", "deprecated"));
    let v = logs.console_json(Some("error"), Some("TYPEerror"), Some(10));
    assert_eq!(v["matching"], 1);
    assert_eq!(v["errors"], 1);
    assert_eq!(v["messages"][0]["text"], "TypeError: x is undefined");
    assert_eq!(logs.console_json(Some("warn"), None, None)["matching"], 1);
    // The newest `limit`, oldest first.
    let v = logs.console_json(Some("log"), None, Some(2));
    assert_eq!(v["messages"][0]["text"], (MAX_CONSOLE + 3).to_string());
    assert_eq!(v["messages"][1]["text"], (MAX_CONSOLE + 4).to_string());
}

#[test]
fn failed_requests() {
    let mut logs = Logs::default();
    logs.push_network(request("http://a/ok", Some(200), None));
    logs.push_network(request("http://a/missing", Some(404), None));
    logs.push_network(request("http://b/cors", None, Some("Load failed")));
    let v = logs.network_json(None, true, None);
    assert_eq!(v["matching"], 2);
    assert_eq!(v["failed"], 2);
    assert_eq!(logs.network_json(Some("A/OK"), false, None)["matching"], 1);
}
