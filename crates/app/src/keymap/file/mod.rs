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

    /// Adds `keystrokes` → `entry` in `context` after every binding the file has in that context,
    /// so that it runs before them (GPUI runs the binding bound last first): at the end of the
    /// last section with that context, or of a new one at the end of the file. Nothing is
    /// replaced. A JSON object holds each key once, so when that section has the keystrokes
    /// already, a new section with the same context takes the binding; the same binding again is
    /// not added twice.
    pub fn add(&mut self, context: Option<&str>, keystrokes: &str, entry: Entry) {
        let context = context.map(str::trim).filter(|c| !c.is_empty());
        let last = self
            .sections
            .iter()
            .rposition(|s| s.context.as_deref() == context);
        let ix = match last {
            Some(ix) => match self.sections[ix]
                .bindings
                .iter()
                .find(|b| same_keys(&b.keystrokes, keystrokes))
            {
                Some(binding) if binding.entry == entry => return,
                Some(_) => self.push_section(context),
                None => ix,
            },
            None => self.push_section(context),
        };
        self.sections[ix].bindings.push(Binding {
            keystrokes: keystrokes.to_string(),
            entry,
        });
    }

    /// Changes the binding at `at` to `keystrokes` → `entry` in `context`. It stays where it is
    /// when its section has that context and no other binding in the context has the keystrokes;
    /// otherwise it moves to where [`Self::add`] puts a new binding, after the ones it now shares
    /// its keys with, so that it runs before them. No other binding changes.
    pub fn replace(&mut self, at: Location, context: Option<&str>, keystrokes: &str, entry: Entry) {
        let context = context.map(str::trim).filter(|c| !c.is_empty());
        let in_its_section = self.sections.get(at.section).is_some_and(|section| {
            section.context.as_deref() == context && at.binding < section.bindings.len()
        });
        let shared = self.bindings().any(|(other, other_context, binding)| {
            other != at && other_context == context && same_keys(&binding.keystrokes, keystrokes)
        });
        if in_its_section && !shared {
            let binding = &mut self.sections[at.section].bindings[at.binding];
            binding.keystrokes = keystrokes.to_string();
            binding.entry = entry;
        } else {
            self.remove(at);
            self.add(context, keystrokes, entry);
        }
    }

    /// Adds an empty section with `context` at the end, and gives its index.
    fn push_section(&mut self, context: Option<&str>) -> usize {
        self.sections.push(Section {
            context: context.map(String::from),
            bindings: Vec::new(),
        });
        self.sections.len() - 1
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

    /// The binding at `at`, if there is one.
    pub fn binding(&self, at: Location) -> Option<&Binding> {
        self.sections.get(at.section)?.bindings.get(at.binding)
    }
}

/// Whether two keystrokes as the file writes them are the same keys, in any order of modifiers.
fn same_keys(a: &str, b: &str) -> bool {
    a == b
        || super::names::normalize_keystrokes(a)
            .is_some_and(|a| super::names::normalize_keystrokes(b) == Some(a))
}

/// Whether `text` has a `//` or `/* */` comment (outside its strings), which writing the keymap
/// again would drop.
pub fn has_comments(text: &str) -> bool {
    without_comments(text) != text
}

/// `text` without its `//` and `/* */` comments, strings left as they are. A comment becomes
/// spaces (a line comment's newline stays), so serde's errors keep their line and column.
fn strip_comments(text: &str) -> String {
    strip_trailing_commas(&without_comments(text))
}

/// [`strip_comments`] with the commas left.
fn without_comments(text: &str) -> String {
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
    out
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

    /// A file with a comment anywhere outside its strings loses it when the keymap is written
    /// again, so an agent's change asks first; one whose `//` is only inside a string, or that
    /// has trailing commas alone, loses nothing of the user's.
    #[test]
    fn a_comment_is_found_outside_the_strings() {
        for commented in [
            "// Mine.\n[]",
            "[{\"bindings\": {\"cmd-q\": null}}] // the end",
            "[/* none yet */]",
            "[{\"bindings\": {/**/}}]",
        ] {
            assert!(has_comments(commented), "{commented}");
        }
        for plain in [
            "",
            "[]",
            r#"[{"context": "a // b", "bindings": {"/": "x::Y", "cmd-/": "x::Z"}}]"#,
            "[{\"bindings\": {\"cmd-q\": null,},},]",
            r#"[{"bindings": {"cmd-k": ["x::Y", {"text": "/* not one */"}]}}]"#,
        ] {
            assert!(!has_comments(plain), "{plain}");
        }
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
        keymap.add(
            Some("Workspace"),
            "cmd-k e",
            Entry::unbind("workspace::CloseTabsLeft"),
        );
        keymap.add(
            Some("Workspace"),
            "cmd-e",
            Entry::action("workspace::CloseTabsLeft", None),
        );
        keymap.add(None, "cmd-q", Entry::Disabled);
        keymap.add(
            Some("Board"),
            "ctrl-1",
            Entry::action("board::Command", Some(json!({"id": "tool.move"}))),
        );
        let text = keymap.to_json();
        assert_eq!(UserKeymap::parse(&text).unwrap(), keymap);
    }

    /// A second binding of the same keys in the same context goes into a section of its own
    /// after the first, which stays: a JSON object holds each key once.
    #[test]
    fn add_keeps_the_other_binding_of_the_same_keys() {
        let mut keymap = UserKeymap::default();
        keymap.add(Some("Workspace"), "cmd-e", Entry::action("a::B", None));
        keymap.add(
            Some(" Workspace "),
            "shift-cmd-e",
            Entry::action("a::B", None),
        );
        keymap.add(Some("Workspace"), "cmd-e", Entry::unbind("c::D"));
        keymap.add(Some(""), "cmd-e", Entry::action("e::F", None));
        let found: Vec<_> = keymap
            .bindings()
            .map(|(_, context, b)| (context, b.keystrokes.as_str(), b.entry.clone()))
            .collect();
        assert_eq!(
            found,
            [
                (Some("Workspace"), "cmd-e", Entry::action("a::B", None)),
                (
                    Some("Workspace"),
                    "shift-cmd-e",
                    Entry::action("a::B", None)
                ),
                (Some("Workspace"), "cmd-e", Entry::unbind("c::D")),
                (None, "cmd-e", Entry::action("e::F", None)),
            ]
        );
        assert_eq!(keymap.sections.len(), 3);
        // The file holds every one of them.
        assert_eq!(UserKeymap::parse(&keymap.to_json()).unwrap(), keymap);

        // The keys in another order are the same keys; the same binding again changes nothing.
        let mut keymap = UserKeymap::default();
        keymap.add(Some("Editor"), "shift-cmd-k", Entry::action("a::B", None));
        keymap.add(Some("Editor"), "cmd-shift-k", Entry::action("x::Y", None));
        assert_eq!(keymap.sections.len(), 2);
        let before = keymap.clone();
        keymap.add(Some("Editor"), "cmd-shift-k", Entry::action("x::Y", None));
        assert_eq!(keymap, before);
    }

    /// A changed binding stays in its place while its keys are its own in its context, and moves
    /// after the bindings it would share them with otherwise, which stay.
    #[test]
    fn replace_changes_only_its_binding() {
        let mut keymap = UserKeymap::default();
        keymap.add(Some("Workspace"), "cmd-a", Entry::action("a::A", None));
        keymap.add(Some("Workspace"), "cmd-b", Entry::action("b::B", None));
        let first = Location {
            section: 0,
            binding: 0,
        };
        keymap.replace(
            first,
            Some("Workspace"),
            "cmd-c",
            Entry::action("a::A", None),
        );
        assert_eq!(keymap.sections.len(), 1);
        assert_eq!(keymap.sections[0].bindings[0].keystrokes, "cmd-c");
        keymap.replace(
            first,
            Some("Workspace"),
            "cmd-b",
            Entry::action("a::A", None),
        );
        let found: Vec<_> = keymap
            .bindings()
            .map(|(_, _, b)| (b.keystrokes.as_str(), b.entry.clone()))
            .collect();
        assert_eq!(
            found,
            [
                ("cmd-b", Entry::action("b::B", None)),
                ("cmd-b", Entry::action("a::A", None)),
            ]
        );
        // Into another context, it moves there.
        keymap.replace(
            Location {
                section: 1,
                binding: 0,
            },
            Some("Editor"),
            "cmd-b",
            Entry::action("a::A", None),
        );
        assert_eq!(keymap.sections.len(), 2);
        assert_eq!(keymap.sections[1].context.as_deref(), Some("Editor"));
    }

    #[test]
    fn remove_drops_an_empty_section() {
        let mut keymap = UserKeymap::default();
        keymap.add(Some("A"), "cmd-e", Entry::action("a::B", None));
        keymap.add(Some("C"), "cmd-e", Entry::action("a::B", None));
        let removed = keymap.remove(Location {
            section: 0,
            binding: 0,
        });
        assert!(removed.is_some());
        assert_eq!(keymap.sections.len(), 1);
        assert_eq!(keymap.sections[0].context.as_deref(), Some("C"));
    }

    #[test]
    fn finds_a_binding_by_its_place() {
        let mut keymap = UserKeymap::default();
        keymap.add(
            Some("Workspace"),
            "cmd-w",
            Entry::unbind("workspace::CloseActiveTab"),
        );
        let at = Location {
            section: 0,
            binding: 0,
        };
        let binding = keymap.binding(at).unwrap();
        assert_eq!(binding.entry.unbinds(), Some("workspace::CloseActiveTab"));
        assert_eq!(keymap.binding(Location { binding: 1, ..at }), None);
        assert_eq!(keymap.binding(Location { section: 1, ..at }), None);
    }
}
