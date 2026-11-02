//! The canvas's default keys as the app's key bindings, and the keystrokes GPUI reports for them.

use std::sync::LazyLock;

use figcraft_engine::keymap;

use super::{defaults, is_offered};
use crate::input::{TEXT_CLIPBOARD, gpui_keystroke};
use crate::policy::{SAWIT_WINS, SAWIT_WINS_UNLESS_EDITING};

/// The app's key context of the canvas while it doesn't edit text, for its bindings.
pub const CANVAS_CONTEXT: &str = "Board && !BoardText";

/// The app's key context of the canvas while it edits text.
pub const TEXT_CONTEXT: &str = "BoardText";

/// One of FigCraft's shortcuts as an app key binding: its GPUI keystroke, its command and the key
/// context it is live in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyBinding {
    pub keystroke: String,
    pub id: &'static str,
    pub context: &'static str,
}

/// The canvas's default keys ([`defaults`]: FigCraft's shortcuts, then Sawit's) as the app binds
/// them, which the keymap editor shows and changes. Each command's keys, but those of the
/// commands the sheet leaves out (hidden, or of no use in the canvas) and the keys Sawit keeps
/// ([`SAWIT_WINS`]), in [`defaults`]' order:
///
/// - On the canvas ([`CANVAS_CONTEXT`]), the keys of the commands live there, but those Sawit
///   keeps while no text is edited ([`SAWIT_WINS_UNLESS_EDITING`]).
/// - While text is edited ([`TEXT_CONTEXT`]), the keys with ⌘, ⌥ or ⌃ of the commands live
///   there, and of copy and paste, which act on the text: plain keys type.
pub fn key_bindings() -> Vec<KeyBinding> {
    default_bindings().to_vec()
}

/// [`key_bindings`], made once: they follow from the default keys and the policy, which never
/// change while the app runs, and every key an agent sends is looked up in them
/// (`input::route_default_key`).
pub(crate) fn default_bindings() -> &'static [KeyBinding] {
    static BINDINGS: LazyLock<Vec<KeyBinding>> = LazyLock::new(make_bindings);
    &BINDINGS
}

/// [`key_bindings`], worked out.
fn make_bindings() -> Vec<KeyBinding> {
    let mut out = Vec::new();
    for binding in defaults() {
        if !is_offered(binding.id) {
            continue;
        }
        for spec in binding.keys {
            let Some(keystroke) = gpui_keystroke(spec) else {
                continue;
            };
            if SAWIT_WINS.contains(&keystroke.as_str()) {
                continue;
            }
            let Some(bound) = binding_keystroke(spec) else {
                continue;
            };
            let modified = ["ctrl-", "alt-", "cmd-"].iter().any(|m| bound.contains(m));
            let mut push = |context| {
                let key = KeyBinding {
                    keystroke: bound.clone(),
                    id: binding.id,
                    context,
                };
                if !out.contains(&key) {
                    out.push(key);
                }
            };
            if binding.context != keymap::Context::Text
                && !SAWIT_WINS_UNLESS_EDITING.contains(&keystroke.as_str())
            {
                push(CANVAS_CONTEXT);
            }
            let in_text =
                binding.context != keymap::Context::Canvas || TEXT_CLIPBOARD.contains(&binding.id);
            if in_text && modified {
                push(TEXT_CONTEXT);
            }
        }
    }
    out
}

/// The US keyboard's shifted symbols, each with its key.
pub(super) const SHIFTED: &[(char, char)] = &[
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
    ('~', '`'),
];

/// The keystroke a binding for `spec` matches, which is [`gpui_keystroke`]'s but for a shifted
/// symbol: GPUI reports ⇧1 as `!` and ⌃⇧/ as `ctrl-?`, Shift off, so the binding is the symbol
/// without Shift (`Shift+1` is `!`, `Ctrl+Shift+?` is `ctrl-?`). A letter keeps its Shift.
pub fn binding_keystroke(spec: &str) -> Option<String> {
    let keystroke = gpui_keystroke(spec)?;
    // The modifiers end at the last `-` but one when the key is `-` itself (`cmd--`).
    let split = if keystroke.ends_with("--") {
        keystroke.len() - 2
    } else {
        keystroke.rfind('-').unwrap_or(0)
    };
    let (modifiers, key) = if keystroke[split..].starts_with('-') {
        (&keystroke[..split], &keystroke[split + 1..])
    } else {
        ("", keystroke.as_str())
    };
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Some(keystroke);
    };
    let shifted = modifiers.split('-').any(|m| m == "shift");
    if !shifted || c.is_ascii_alphabetic() {
        return Some(keystroke);
    }
    let Some(&(symbol, _)) = SHIFTED
        .iter()
        .find(|(symbol, key)| *symbol == c || *key == c)
    else {
        return Some(keystroke);
    };
    let mut out: String = modifiers
        .split('-')
        .filter(|m| !m.is_empty() && *m != "shift")
        .map(|m| format!("{m}-"))
        .collect();
    out.push(symbol);
    Some(out)
}

/// The key that types `symbol` with Shift on a US keyboard (`.` for `>`), when `symbol` is one of
/// its shifted symbols: what a binding of the symbol, written as GPUI reports it
/// ([`binding_keystroke`]), is pressed with. `None` for any other character.
pub fn unshifted(symbol: char) -> Option<char> {
    SHIFTED
        .iter()
        .find(|&&(shifted, _)| shifted == symbol)
        .map(|&(_, key)| key)
}

/// Whether Sawit keeps `spec`, a FigCraft binding (`Cmd+W`), when the canvas has the keyboard.
/// The policy writes its keys as GPUI does, so `spec` is converted to compare.
pub(crate) fn owned_by_sawit(spec: &str) -> bool {
    gpui_keystroke(spec).is_some_and(|keystroke| SAWIT_WINS.contains(&keystroke.as_str()))
}
