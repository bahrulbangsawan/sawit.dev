//! The small dialogs of the commands that need a value the canvas asks for: a link's URL, an
//! offset, a page, the new names, a colour profile and the nudge amounts. FigCraft's own app asks
//! with dialogs of its UI; here each is a [`Prompt`]: its fields, filled in with what the
//! document holds now, and the command's params made from what the user typed ([`params`]). The
//! view in `crates/app/src/board/prompt.rs` draws it and runs the command.
//!
//! An agent passes the values in the command's params, as before; only the menus, the Actions
//! palette and the keys open the dialog ([`is_prompt`]).

use figcraft_engine::Session;
use serde_json::{Map, Value, json};

/// The commands that open a dialog for their value when the user runs them.
pub const PROMPTS: &[&str] = &[
    "text.link",
    "vector.offset",
    "object.moveToPage",
    "object.batchRename",
    "file.colorProfile",
    "pref.nudge",
    "edit.findReplace",
    "file.saveVersion",
];

/// Whether command `id` opens a dialog for its value when the user runs it.
pub fn is_prompt(id: &str) -> bool {
    PROMPTS.contains(&id)
}

/// What a field takes.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    /// Any text; `required` refuses an empty one.
    Text { required: bool },
    /// A number between `min` and `max`.
    Number { min: f64, max: f64 },
    /// One of `options`, each its value and its label.
    Choice(Vec<(String, String)>),
    /// On or off.
    Toggle,
}

/// One field of a dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    /// The command's param it fills.
    pub key: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
    /// What it shows first: the text or number, a choice's value, or `"true"` / `"false"`.
    pub value: String,
    /// The hint an empty text field shows.
    pub placeholder: &'static str,
}

/// A command's dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub command: &'static str,
    pub title: &'static str,
    /// A line under the title, saying what the command does with it.
    pub note: &'static str,
    pub fields: Vec<Field>,
    /// The button that runs it.
    pub submit: &'static str,
}

fn field(
    key: &'static str,
    label: &'static str,
    kind: FieldKind,
    value: impl Into<String>,
) -> Field {
    Field {
        key,
        label,
        kind,
        value: value.into(),
        placeholder: "",
    }
}

/// Command `id`'s dialog, filled in from `session`; `None` for a command that opens none, or
/// when there is nothing to ask about (no other page to move to).
pub fn prompt(session: &Session, id: &str) -> Option<Prompt> {
    Some(match id {
        "text.link" => Prompt {
            command: "text.link",
            title: "Create link",
            note: "The selected text, or the text layers selected, link to this address.",
            fields: vec![Field {
                placeholder: "https://example.com",
                ..field("url", "URL", FieldKind::Text { required: true }, "")
            }],
            submit: "Link",
        },
        "vector.offset" => Prompt {
            command: "vector.offset",
            title: "Offset path",
            note: "Grows the selected shapes' outlines by this much; less than 0 shrinks them.",
            fields: vec![field(
                "offset",
                "Offset",
                FieldKind::Number {
                    min: -1000.0,
                    max: 1000.0,
                },
                "8",
            )],
            submit: "Offset",
        },
        "object.moveToPage" => {
            let pages = other_pages(session);
            let first = pages.first()?.0.clone();
            Prompt {
                command: "object.moveToPage",
                title: "Move to page",
                note: "The selected layers move to this page, where they are.",
                fields: vec![field("page", "Page", FieldKind::Choice(pages), first)],
                submit: "Move",
            }
        }
        "object.batchRename" => Prompt {
            command: "object.batchRename",
            title: "Rename layers",
            note: "Find and replace in the selected layers' names, or give each a new name. \
                   $& is the current name, $n counts up and $N is how many there are.",
            fields: vec![
                Field {
                    placeholder: "Text to find",
                    ..field("find", "Find", FieldKind::Text { required: false }, "")
                },
                Field {
                    placeholder: "Replace with",
                    ..field(
                        "replace",
                        "Replace",
                        FieldKind::Text { required: false },
                        "",
                    )
                },
                Field {
                    placeholder: "Card $n",
                    ..field(
                        "name",
                        "Or rename to",
                        FieldKind::Text { required: false },
                        "",
                    )
                },
                field(
                    "regex",
                    "Find is a regular expression",
                    FieldKind::Toggle,
                    "false",
                ),
                field(
                    "start",
                    "$n starts at",
                    FieldKind::Number {
                        min: 0.0,
                        max: 1_000_000.0,
                    },
                    "1",
                ),
            ],
            submit: "Rename",
        },
        "file.colorProfile" => {
            let p3 = session
                .ds()
                .is_ok_and(|ds| ds.doc.color_profile == figcraft_color::ColorProfile::DisplayP3);
            let current = if p3 { "displayP3" } else { "srgb" };
            Prompt {
                command: "file.colorProfile",
                title: "Color profile",
                note: "The colour space the file's colours are in, and that its exports are \
                       tagged with.",
                fields: vec![
                    field(
                        "profile",
                        "Profile",
                        FieldKind::Choice(vec![
                            ("srgb".into(), "sRGB".into()),
                            ("displayP3".into(), "Display P3".into()),
                        ]),
                        current,
                    ),
                    field(
                        "convert",
                        "Keep how colours look (convert their values)",
                        FieldKind::Toggle,
                        "true",
                    ),
                ],
                submit: "Change",
            }
        }
        "pref.nudge" => Prompt {
            command: "pref.nudge",
            title: "Nudge amount",
            note: "How far the arrow keys move the selection, and with Shift.",
            fields: vec![
                field(
                    "small",
                    "Small nudge",
                    FieldKind::Number {
                        min: 0.01,
                        max: 1000.0,
                    },
                    trimmed(session.prefs.nudge_small),
                ),
                field(
                    "big",
                    "Big nudge (Shift)",
                    FieldKind::Number {
                        min: 0.01,
                        max: 10_000.0,
                    },
                    trimmed(session.prefs.nudge_big),
                ),
            ],
            submit: "Save",
        },
        "edit.findReplace" => Prompt {
            command: "edit.findReplace",
            title: "Find and replace",
            note: "Finds the text in the page's text layers, and selects them; with a replacement, \
                   replaces it in each.",
            fields: vec![
                Field {
                    placeholder: "Text to find",
                    ..field("find", "Find", FieldKind::Text { required: true }, "")
                },
                Field {
                    placeholder: "Leave empty to only find",
                    ..field(
                        "replace",
                        "Replace with",
                        FieldKind::Text { required: false },
                        "",
                    )
                },
                field(
                    "layerNames",
                    "In layer names too",
                    FieldKind::Toggle,
                    "false",
                ),
            ],
            submit: "Find",
        },
        "file.saveVersion" => Prompt {
            command: "file.saveVersion",
            title: "Save to version history",
            note: "Keeps the design as it is now in the file's history, to go back to later.",
            fields: vec![
                Field {
                    placeholder: "What changed",
                    ..field("name", "Name", FieldKind::Text { required: false }, "")
                },
                field(
                    "description",
                    "Description",
                    FieldKind::Text { required: false },
                    "",
                ),
            ],
            submit: "Save",
        },
        _ => return None,
    })
}

/// The command's params from `values`, one per field of `prompt` in its order, with the
/// selection's `ids` when the command acts on layers. Why not, for a value that is no good: a
/// required text left empty, a number that isn't one or is out of its range, an unknown choice.
pub fn params(
    prompt: &Prompt,
    values: &[String],
    ids: &[figcraft_doc::NodeId],
) -> Result<Value, String> {
    let mut params = Map::new();
    for (field, value) in prompt.fields.iter().zip(values) {
        let value = value.trim();
        let typed = match &field.kind {
            FieldKind::Text { required } => {
                if *required && value.is_empty() {
                    return Err(format!("{} can't be empty", field.label));
                }
                if value.is_empty() {
                    continue;
                }
                json!(value)
            }
            FieldKind::Number { min, max } => {
                let n: f64 = value
                    .parse()
                    .ok()
                    .filter(|n: &f64| n.is_finite())
                    .ok_or_else(|| format!("{} must be a number", field.label))?;
                if n < *min || n > *max {
                    return Err(format!(
                        "{} must be from {} to {}",
                        field.label,
                        trimmed(*min),
                        trimmed(*max)
                    ));
                }
                json!(n)
            }
            FieldKind::Choice(options) => {
                if !options.iter().any(|(option, _)| option == value) {
                    return Err(format!("Pick a {}", field.label.to_lowercase()));
                }
                json!(value)
            }
            FieldKind::Toggle => json!(value == "true"),
        };
        params.insert(field.key.to_string(), typed);
    }
    if prompt.command == "object.batchRename"
        && !params.contains_key("find")
        && !params.contains_key("name")
    {
        return Err("Type something to find, or a new name".into());
    }
    if prompt.command == "object.moveToPage"
        && let Some(page) = params
            .get("page")
            .and_then(Value::as_str)
            .map(str::to_string)
    {
        // The engine takes a page by its id, a number.
        if let Ok(id) = page.parse::<u64>() {
            params.insert("page".into(), json!(id));
        }
    }
    let on_layers = !matches!(
        prompt.command,
        "file.colorProfile" | "pref.nudge" | "text.link" | "edit.findReplace" | "file.saveVersion"
    );
    if on_layers && !ids.is_empty() {
        params.insert("ids".into(), json!(ids));
    }
    Ok(Value::Object(params))
}

/// The pages but the one on show, as (id, name).
fn other_pages(session: &Session) -> Vec<(String, String)> {
    let Ok(ds) = session.ds() else {
        return Vec::new();
    };
    let doc = &ds.doc;
    doc.pages
        .iter()
        .filter(|page| **page != ds.page)
        .filter_map(|page| Some((page.0.to_string(), doc.node(*page)?.name.clone())))
        .collect()
}

/// `n` without a trailing `.0`.
fn trimmed(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests;
