//! The key bindings as `sawit-mcp` and `sawit list keys` get them from the running app: the ops
//! they send ([`KeymapOp`], in [`crate::app_control::Command::Keymap`]) and the rows the app
//! answers with. Each feature binds its default keys in code, so only the running app can say what
//! a key does; the user's own bindings are in `keymap.json` ([`crate::instances::AppPaths::keymap_file`]).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What an agent or the CLI asks of the keymap.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum KeymapOp {
    /// The rows of the keymap editor: those matching every word of `filter` (in the action, its
    /// label, keys, context or arguments, any case), and starting with `keys` (as `keymap.json`
    /// writes keystrokes). With `unbound`, the actions no key runs too.
    List {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filter: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keys: Option<String>,
        #[serde(default)]
        unbound: bool,
    },
    /// Binds `keys` to `action` (with `args`) in `context` (none: anywhere), as the editor's
    /// Create Keybinding does; with `replace`, changes that binding instead, as its Edit does.
    /// `confirm` lets it rewrite a `keymap.json` that has comments, which the rewrite drops.
    Bind {
        keys: String,
        action: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        /// Boxed: it would make every op, and every app command, twice as big.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        replace: Option<Box<BindingRef>>,
        #[serde(default)]
        confirm: bool,
    },
    /// Takes the binding of `keys` out, as the editor's remove button does: the user's own leaves
    /// the file, a default is turned off there. With `restore`, brings a turned-off default back.
    /// `context`, `action` and `args` pick one binding when several have the keys.
    Unbind {
        keys: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        action: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<Value>,
        #[serde(default)]
        restore: bool,
        #[serde(default)]
        confirm: bool,
    },
}

/// A binding named as [`KeymapRow`] shows it: its keys, its context (none: whichever context
/// the only binding of those keys has, and `""` a binding that has none), its action and, for an
/// action that takes them, its arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindingRef {
    pub keys: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
}

/// Where a row of the keymap comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeySource {
    /// The app's default.
    Default,
    /// The user's, in `keymap.json`.
    User,
    /// A default the user turned off.
    Removed,
    /// An action no key runs.
    Unbound,
}

/// One row of the keymap, as the keymap editor shows it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapRow {
    /// `workspace::CloseActiveTab`, as `keymap.json` names it.
    pub action: String,
    /// What the editor calls it: `workspace: close active tab`, `canvas: rectangle`.
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    /// As `keymap.json` writes them: `cmd-k cmd-s`; empty for an unbound action.
    pub keys: String,
    /// As a Mac menu writes them: `⌘K ⌘S`.
    pub display: String,
    /// Where the keys work; none: anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    pub source: KeySource,
    /// Another binding that runs has the same keys in the same context: only one of them runs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub conflict: bool,
}

/// The answer to [`KeymapOp::List`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KeymapList {
    /// The user's keymap file.
    pub path: PathBuf,
    /// Whether the file can be written: not while it can't be read.
    pub writable: bool,
    /// Why the file can't be read, then why each of its bindings that doesn't load fails.
    #[serde(default)]
    pub errors: Vec<String>,
    pub rows: Vec<KeymapRow>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// The ops go over the socket as flat JSON under `op`, and what an agent leaves out is
    /// optional: a bare list, a bind with no context, an unbind with no confirmation.
    #[test]
    fn keymap_ops_round_trip_with_their_options_left_out() {
        let ops = [
            (
                KeymapOp::List {
                    filter: None,
                    keys: Some("cmd-k".into()),
                    unbound: false,
                },
                json!({"op": "list", "keys": "cmd-k", "unbound": false}),
            ),
            (
                KeymapOp::Bind {
                    keys: "cmd-shift-w".into(),
                    action: "workspace::CloseAllTabs".into(),
                    args: None,
                    context: Some("Workspace".into()),
                    replace: Some(Box::new(BindingRef {
                        keys: "cmd-k w".into(),
                        context: Some("Workspace".into()),
                        action: "workspace::CloseAllTabs".into(),
                        args: None,
                    })),
                    confirm: false,
                },
                json!({
                    "op": "bind",
                    "keys": "cmd-shift-w",
                    "action": "workspace::CloseAllTabs",
                    "context": "Workspace",
                    "replace": {
                        "keys": "cmd-k w",
                        "context": "Workspace",
                        "action": "workspace::CloseAllTabs",
                    },
                    "confirm": false,
                }),
            ),
            (
                KeymapOp::Unbind {
                    keys: "cmd-w".into(),
                    context: None,
                    action: None,
                    args: Some(json!({"id": "tool.move"})),
                    restore: true,
                    confirm: true,
                },
                json!({
                    "op": "unbind",
                    "keys": "cmd-w",
                    "args": {"id": "tool.move"},
                    "restore": true,
                    "confirm": true,
                }),
            ),
        ];
        for (op, wire) in ops {
            assert_eq!(serde_json::to_value(&op).unwrap(), wire);
            assert_eq!(serde_json::from_value::<KeymapOp>(wire).unwrap(), op);
        }
        let bare: KeymapOp = serde_json::from_value(json!({"op": "list"})).unwrap();
        assert_eq!(
            bare,
            KeymapOp::List {
                filter: None,
                keys: None,
                unbound: false
            }
        );
        let unbind: KeymapOp =
            serde_json::from_value(json!({"op": "unbind", "keys": "cmd-w"})).unwrap();
        assert!(matches!(
            unbind,
            KeymapOp::Unbind {
                restore: false,
                confirm: false,
                ..
            }
        ));
        assert!(
            serde_json::from_value::<KeymapOp>(json!({"op": "bind", "keys": "cmd-w"})).is_err()
        );
    }

    /// A row says only what it has: no arguments, context or conflict when there are none, so
    /// the hundreds of rows of a list stay short.
    #[test]
    fn a_row_leaves_out_what_it_has_not() {
        let row = KeymapRow {
            action: "workspace::CloseActiveTab".into(),
            label: "workspace: close active tab".into(),
            args: None,
            keys: "cmd-w".into(),
            display: "⌘W".into(),
            context: None,
            source: KeySource::Default,
            conflict: false,
        };
        let wire = serde_json::to_value(&row).unwrap();
        assert_eq!(
            wire,
            json!({
                "action": "workspace::CloseActiveTab",
                "label": "workspace: close active tab",
                "keys": "cmd-w",
                "display": "⌘W",
                "source": "default",
            })
        );
        assert_eq!(serde_json::from_value::<KeymapRow>(wire).unwrap(), row);
        let clashing = KeymapRow {
            context: Some("Workspace".into()),
            source: KeySource::Removed,
            conflict: true,
            ..row
        };
        let wire = serde_json::to_value(&clashing).unwrap();
        assert_eq!(wire["context"], "Workspace");
        assert_eq!(wire["source"], "removed");
        assert_eq!(wire["conflict"], true);
    }
}
