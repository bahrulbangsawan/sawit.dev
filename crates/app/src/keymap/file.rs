//! The user's keymap file, `keymap.json` in Sawit's folder, in Zed's format: a list of sections,
//! each with an optional context and its bindings, keystrokes to an action.
//!
//! ```json
//! [
//!   {
//!     "context": "Workspace",
//!     "bindings": {
//!       "cmd-shift-w": "workspace::CloseAllTabs",
//!       "cmd-w": null,
//!       "cmd-k e": ["zed::Unbind", "workspace::CloseTabsLeft"],
//!       "ctrl-cmd-1": ["board::Command", { "id": "tool.move" }]
//!     }
//!   }
//! ]
//! ```
//!
//! An action is its name, or its name and its arguments in a list. `null` turns the keystrokes off
//! in that context, and `["zed::Unbind", name]` turns off only their binding to `name`, which is
//! how the keymap editor removes or replaces a default binding.
//!
//! `//` and `/* */` comments are read, as in Zed's file, but the keymap editor writes the file
//! back without them.

use serde_json::{Map, Value, json};

/// The name GPUI gives its action that unbinds one action from some keystrokes.
pub const UNBIND: &str = "zed::Unbind";

/// What some keystrokes do, as the file says.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// `null`: the keystrokes do nothing in the section's context.
    Disabled,
    /// An action, with its arguments if it takes any.
    Action { name: String, args: Option<Value> },
}

impl Entry {
    pub fn action(name: impl Into<String>, args: Option<Value>) -> Self {
        Entry::Action {
            name: name.into(),
            args,
        }
    }

    /// The `zed::Unbind` entry that turns off the binding of `name` to the keystrokes.
    pub fn unbind(name: &str) -> Self {
        Entry::action(UNBIND, Some(json!(name)))
    }

    /// The action this entry unbinds, if it is a `zed::Unbind`.
    pub fn unbinds(&self) -> Option<&str> {
        match self {
            Entry::Action { name, args } if name == UNBIND => args.as_ref()?.as_str(),
            _ => None,
        }
    }

    fn from_json(value: &Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Entry::Disabled),
            Value::String(name) => Ok(Entry::action(name.as_str(), None)),
            Value::Array(items) => match items.as_slice() {
                [Value::String(name)] => Ok(Entry::action(name.as_str(), None)),
                [Value::String(name), args] => Ok(Entry::action(name.as_str(), Some(args.clone()))),
                _ => Err("an action is a name, or a list of a name and its arguments".into()),
            },
            _ => Err("an action is a name, a list of a name and its arguments, or null".into()),
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Entry::Disabled => Value::Null,
            Entry::Action { name, args: None } => json!(name),
            Entry::Action {
                name,
                args: Some(args),
            } => json!([name, args]),
        }
    }
}

/// One binding in the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    /// GPUI keystrokes, space between the keys of a chord (`cmd-k e`).
    pub keystrokes: String,
    pub entry: Entry,
}

/// A section: its context and its bindings, in the file's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Section {
    pub context: Option<String>,
    pub bindings: Vec<Binding>,
}

/// Where a binding is in the file: its section, then its place in the section.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    pub section: usize,
    pub binding: usize,
}

/// The whole file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UserKeymap {
    pub sections: Vec<Section>,
}

impl UserKeymap {
    /// Reads the file's text. Empty text, or only comments, is an empty keymap. A section or a
    /// binding that can't be read is an error naming it: the editor shows it, and the bindings are
    /// left as they were until the file is fixed.
    pub fn parse(text: &str) -> Result<Self, String> {
        let text = strip_comments(text);
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let Value::Array(sections) = value else {
            return Err("the keymap is a list of sections".into());
        };
        let mut keymap = Self::default();
        for (ix, section) in sections.iter().enumerate() {
            let Value::Object(section) = section else {
                return Err(format!("section {} is not an object", ix + 1));
            };
            let context = match section.get("context") {
                None | Some(Value::Null) => None,
                Some(Value::String(context)) if context.trim().is_empty() => None,
                Some(Value::String(context)) => Some(context.clone()),
                Some(_) => return Err(format!("section {}: the context is not text", ix + 1)),
            };
            let bindings = match section.get("bindings") {
                None | Some(Value::Null) => Vec::new(),
                Some(Value::Object(bindings)) => bindings
                    .iter()
                    .map(|(keystrokes, value)| {
                        Entry::from_json(value)
                            .map(|entry| Binding {
                                keystrokes: keystrokes.clone(),
                                entry,
                            })
                            .map_err(|e| format!("section {}, \"{keystrokes}\": {e}", ix + 1))
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(format!("section {}: bindings is not an object", ix + 1)),
            };
            keymap.sections.push(Section { context, bindings });
        }
        Ok(keymap)
    }

    /// The file's text: pretty JSON, sections without bindings left out.
    pub fn to_json(&self) -> String {
        let sections: Vec<Value> = self
            .sections
            .iter()
            .filter(|s| !s.bindings.is_empty())
            .map(|section| {
                let mut object = Map::new();
                if let Some(context) = &section.context {
                    object.insert("context".into(), json!(context));
                }
                let bindings: Map<String, Value> = section
                    .bindings
                    .iter()
                    .map(|b| (b.keystrokes.clone(), b.entry.to_json()))
                    .collect();
                object.insert("bindings".into(), Value::Object(bindings));
                Value::Object(object)
            })
            .collect();
        let mut text = serde_json::to_string_pretty(&sections).unwrap_or_else(|_| "[]".into());
        text.push('\n');
        text
    }

    /// Every binding with where it is, in the file's order.
    pub fn bindings(&self) -> impl Iterator<Item = (Location, Option<&str>, &Binding)> {
        self.sections.iter().enumerate().flat_map(|(s, section)| {
            section
                .bindings
                .iter()
                .enumerate()
                .map(move |(b, binding)| {
                    (
                        Location {
                            section: s,
                            binding: b,
                        },
                        section.context.as_deref(),
                        binding,
                    )
                })
        })
    }

    /// Sets `keystrokes` to `entry` in `context`, in the last section with that context (a new one
    /// at the end if there is none). The keystrokes' binding there, if any, is replaced: a JSON
    /// object holds each key once.
    pub fn set(&mut self, context: Option<&str>, keystrokes: &str, entry: Entry) {
        let context = context.map(str::trim).filter(|c| !c.is_empty());
        let ix = match self
            .sections
            .iter()
            .rposition(|s| s.context.as_deref() == context)
        {
            Some(ix) => ix,
            None => {
                self.sections.push(Section {
                    context: context.map(String::from),
                    bindings: Vec::new(),
                });
                self.sections.len() - 1
            }
        };
        let bindings = &mut self.sections[ix].bindings;
        match bindings.iter_mut().find(|b| b.keystrokes == keystrokes) {
            Some(binding) => binding.entry = entry,
            None => bindings.push(Binding {
                keystrokes: keystrokes.to_string(),
                entry,
            }),
        }
    }

    /// Takes out the binding at `at`. Its section goes too when nothing is left in it.
    pub fn remove(&mut self, at: Location) -> Option<Binding> {
        let section = self.sections.get_mut(at.section)?;
        if at.binding >= section.bindings.len() {
            return None;
        }
        let binding = section.bindings.remove(at.binding);
        if section.bindings.is_empty() {
            self.sections.remove(at.section);
        }
        Some(binding)
    }

    /// Where the `zed::Unbind` of `action` from `keystrokes` in `context` is, if the file has one.
    pub fn find_unbind(
        &self,
        context: Option<&str>,
        keystrokes: &str,
        action: &str,
    ) -> Option<Location> {
        self.bindings()
            .find(|(_, ctx, b)| {
                *ctx == context && b.keystrokes == keystrokes && b.entry.unbinds() == Some(action)
            })
            .map(|(at, _, _)| at)
    }
}

/// `text` without its `//` and `/* */` comments, strings left as they are. A comment becomes
/// spaces (a line comment's newline stays), so serde's errors keep their line and column.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                    out.push(' ');
                }
            }
            ('/', Some('*')) => {
                chars.next();
                out.push_str("  ");
                let mut last = ' ';
                for c in chars.by_ref() {
                    out.push(if c == '\n' { '\n' } else { ' ' });
                    if last == '*' && c == '/' {
                        break;
                    }
                    last = c;
                }
            }
            _ => out.push(c),
        }
    }
    strip_trailing_commas(&out)
}

/// `text` without the commas just before a `]` or `}`, which Zed's files often have. A comma
/// becomes a space, so serde's errors keep their column.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut ix = 0;
    while ix < chars.len() {
        let c = chars[ix];
        ix += 1;
        if in_string {
            out.push(c);
            match c {
                '\\' if ix < chars.len() => {
                    out.push(chars[ix]);
                    ix += 1;
                }
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        let trailing = c == ','
            && chars[ix..]
                .iter()
                .find(|c| !c.is_whitespace())
                .is_some_and(|next| matches!(next, ']' | '}'));
        out.push(if trailing { ' ' } else { c });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_zeds_format() {
        let keymap = UserKeymap::parse(
            r#"
            // Mine.
            [
              { "bindings": { "cmd-q": null } },
              {
                "context": "Workspace", /* the window */
                "bindings": {
                  "cmd-shift-w": "workspace::CloseAllTabs",
                  "cmd-k e": ["zed::Unbind", "workspace::CloseTabsLeft"],
                  "ctrl-1": ["board::Command", { "id": "tool.move" }],
                  "ctrl-2": ["workspace::ShowFiles"],
                },
              },
            ]
            "#,
        )
        .unwrap();
        assert_eq!(keymap.sections.len(), 2);
        assert_eq!(keymap.sections[0].context, None);
        assert_eq!(keymap.sections[0].bindings[0].entry, Entry::Disabled);
        let workspace = &keymap.sections[1];
        assert_eq!(workspace.context.as_deref(), Some("Workspace"));
        let entries: Vec<_> = workspace.bindings.iter().map(|b| &b.entry).collect();
        assert_eq!(
            entries,
            [
                &Entry::action("workspace::CloseAllTabs", None),
                &Entry::unbind("workspace::CloseTabsLeft"),
                &Entry::action("board::Command", Some(json!({"id": "tool.move"}))),
                &Entry::action("workspace::ShowFiles", None),
            ]
        );
        assert_eq!(
            workspace.bindings[1].entry.unbinds(),
            Some("workspace::CloseTabsLeft")
        );
    }

    #[test]
    fn comments_inside_strings_stay() {
        let keymap =
            UserKeymap::parse(r#"[{"context": "a // b", "bindings": {"/": "x::Y"}}]"#).unwrap();
        assert_eq!(keymap.sections[0].context.as_deref(), Some("a // b"));
        assert_eq!(keymap.sections[0].bindings[0].keystrokes, "/");
    }

    #[test]
    fn empty_file_is_an_empty_keymap() {
        assert_eq!(UserKeymap::parse("").unwrap(), UserKeymap::default());
        assert_eq!(
            UserKeymap::parse("// nothing\n").unwrap(),
            UserKeymap::default()
        );
    }

    #[test]
    fn errors_name_the_binding() {
        let error = UserKeymap::parse(r#"[{"bindings": {"cmd-x": 3}}]"#).unwrap_err();
        assert!(error.contains("\"cmd-x\""), "{error}");
        assert!(UserKeymap::parse("{}").is_err());
    }

    #[test]
    fn round_trips() {
        let mut keymap = UserKeymap::default();
        keymap.set(
            Some("Workspace"),
            "cmd-k e",
            Entry::unbind("workspace::CloseTabsLeft"),
        );
        keymap.set(
            Some("Workspace"),
            "cmd-e",
            Entry::action("workspace::CloseTabsLeft", None),
        );
        keymap.set(None, "cmd-q", Entry::Disabled);
        keymap.set(
            Some("Board"),
            "ctrl-1",
            Entry::action("board::Command", Some(json!({"id": "tool.move"}))),
        );
        let text = keymap.to_json();
        assert_eq!(UserKeymap::parse(&text).unwrap(), keymap);
    }

    #[test]
    fn set_replaces_the_same_keystrokes_in_the_same_context() {
        let mut keymap = UserKeymap::default();
        keymap.set(Some("Workspace"), "cmd-e", Entry::action("a::B", None));
        keymap.set(Some(" Workspace "), "cmd-e", Entry::action("c::D", None));
        keymap.set(Some(""), "cmd-e", Entry::action("e::F", None));
        assert_eq!(keymap.sections.len(), 2);
        assert_eq!(keymap.sections[0].bindings.len(), 1);
        assert_eq!(
            keymap.sections[0].bindings[0].entry,
            Entry::action("c::D", None)
        );
        assert_eq!(keymap.sections[1].context, None);
    }

    #[test]
    fn remove_drops_an_empty_section() {
        let mut keymap = UserKeymap::default();
        keymap.set(Some("A"), "cmd-e", Entry::action("a::B", None));
        keymap.set(Some("C"), "cmd-e", Entry::action("a::B", None));
        let removed = keymap.remove(Location {
            section: 0,
            binding: 0,
        });
        assert!(removed.is_some());
        assert_eq!(keymap.sections.len(), 1);
        assert_eq!(keymap.sections[0].context.as_deref(), Some("C"));
    }

    #[test]
    fn finds_an_unbind() {
        let mut keymap = UserKeymap::default();
        keymap.set(
            Some("Workspace"),
            "cmd-w",
            Entry::unbind("workspace::CloseActiveTab"),
        );
        assert_eq!(
            keymap.find_unbind(Some("Workspace"), "cmd-w", "workspace::CloseActiveTab"),
            Some(Location {
                section: 0,
                binding: 0
            })
        );
        assert_eq!(
            keymap.find_unbind(None, "cmd-w", "workspace::CloseActiveTab"),
            None
        );
    }
}
