//! What a page sends the app over IPC, read only when it carries the webview's token.

use serde_json::Value;

use super::logs::{ConsoleEntry, NetworkEntry};

/// What a page sends over IPC.
#[derive(Debug, Clone, PartialEq)]
pub enum PageEvent {
    Console(ConsoleEntry),
    Network(NetworkEntry),
    /// The address changed without a load (`pushState`, hash).
    Url(String),
    /// The user picked an element: `{mode, element}`.
    Picked(Value),
    /// The pointer is over an element in inspect mode: `{selector, label, rect}`.
    Hovered(Value),
    /// A key pressed in the page: `escape`, while picking.
    Key(String),
    /// The user changed an annotation from its popover on the page:
    /// `{action: "update", n, comment, intent}` or `{action: "remove", n}`.
    Annotation(Value),
    /// The user pressed in the page (a real `pointerdown`, never a script's), which AppKit then
    /// gives the keyboard.
    Pressed,
}

/// A page message, if it carries `token`.
pub fn parse_ipc(body: &str, token: &str) -> Option<PageEvent> {
    let rest = body.strip_prefix(token)?.strip_prefix('\n')?;
    let (kind, json) = rest.split_once('\n')?;
    match kind {
        "console" => serde_json::from_str(json).ok().map(PageEvent::Console),
        "network" => serde_json::from_str(json).ok().map(PageEvent::Network),
        "url" => serde_json::from_str::<String>(json)
            .ok()
            .map(PageEvent::Url),
        "picked" => serde_json::from_str(json).ok().map(PageEvent::Picked),
        "hovered" => serde_json::from_str(json).ok().map(PageEvent::Hovered),
        "key" => serde_json::from_str::<String>(json)
            .ok()
            .map(PageEvent::Key),
        "annotation" => serde_json::from_str(json).ok().map(PageEvent::Annotation),
        "pressed" => Some(PageEvent::Pressed),
        _ => None,
    }
}
