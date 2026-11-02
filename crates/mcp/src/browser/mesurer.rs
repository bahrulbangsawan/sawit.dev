//! `browser_mesurer`'s arguments as the app's `mesurer` op (`BrowserOp::Mesurer`): each action
//! with what it takes, and nothing that goes with another action, so an agent learns of a mistake
//! from the tool rather than from a call that did something else. Comments' words are the user's:
//! no action writes one.

use sawit_core::browser::BrowserOp;
use serde_json::{Map, Value, json};

use super::MesurerArgs;

/// The arguments `set` takes, by name, with their values.
fn set_args(a: &MesurerArgs) -> Map<String, Value> {
    let mut args = Map::new();
    let mut put = |name: &str, value: Option<Value>| {
        if let Some(value) = value {
            args.insert(name.into(), value);
        }
    };
    put("xray", a.xray.map(Value::from));
    put("rulers", a.rulers.map(Value::from));
    put("tool", a.tool.clone().map(Value::from));
    put("minimized", a.minimized.map(Value::from));
    put("guides", a.guides.as_ref().map(|g| json!(g)));
    put("layout_guides", a.layout_guides.as_ref().map(|l| json!(l)));
    args
}

/// The app's op for `a`.
pub(crate) fn mesurer_op(a: &MesurerArgs) -> Result<BrowserOp, String> {
    let action = a
        .action
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("state")
        .to_ascii_lowercase();
    let set = set_args(a);
    let stray = |allowed: &[&str]| -> Result<(), String> {
        let mut given: Vec<&str> = set.keys().map(String::as_str).collect();
        if a.what.is_some() {
            given.push("what");
        }
        if a.full {
            given.push("full");
        }
        match given.into_iter().find(|name| !allowed.contains(name)) {
            Some(name) => Err(format!(
                "{name} doesn't go with action {action}: {}",
                match name {
                    "what" => "it is clear's.",
                    "full" => "it is state's.",
                    _ => "it is set's.",
                }
            )),
            None => Ok(()),
        }
    };
    let mut args = Map::new();
    match action.as_str() {
        "state" => {
            stray(&["full"])?;
            if a.full {
                args.insert("html".into(), true.into());
            }
        }
        "open" | "close" => stray(&[])?,
        "set" => {
            stray(&[
                "xray",
                "rulers",
                "tool",
                "minimized",
                "guides",
                "layout_guides",
            ])?;
            if set.is_empty() {
                return Err(
                    "set needs one of xray, rulers, tool, minimized, guides, layout_guides.".into(),
                );
            }
            args = set;
        }
        "clear" => {
            stray(&["what"])?;
            let what = a.what.as_deref().map(str::trim).unwrap_or_default();
            if what.is_empty() {
                return Err(
                    "clear needs what: drawings, measurements, guides, layout_guides, comments \
                     or all."
                        .into(),
                );
            }
            args.insert("what".into(), what.into());
        }
        other => {
            return Err(format!(
                "Unknown action “{other}”: use state, open, close, set or clear."
            ));
        }
    }
    Ok(BrowserOp::Mesurer { action, args })
}

/// `browser_screenshot`'s `mesurer`: `hide`, `drawings` or `show`, as the app takes it.
pub(crate) fn screenshot_mesurer(mode: Option<&str>) -> Result<Option<String>, String> {
    match mode.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        None | Some("") => Ok(None),
        Some(mode @ ("hide" | "drawings" | "show")) => Ok(Some(mode.to_string())),
        Some(other) => Err(format!("mesurer is hide, drawings or show, not “{other}”.")),
    }
}
