//! Key names: GPUI's and FigCraft's for the same key, both ways, and a key as FigCraft's own
//! keyboard names it.

use std::borrow::Cow;

use figcraft_engine::keymap;

use super::Mods;

/// The keys FigCraft names in words: GPUI's name, then FigCraft's.
const NAMED_KEYS: &[(&str, &str)] = &[
    ("backspace", "Backspace"),
    ("delete", "Delete"),
    ("left", "ArrowLeft"),
    ("right", "ArrowRight"),
    ("up", "ArrowUp"),
    ("down", "ArrowDown"),
    ("enter", "Enter"),
    ("escape", "Escape"),
    ("tab", "Tab"),
    ("space", "Space"),
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "PageUp"),
    ("pagedown", "PageDown"),
    ("f1", "F1"),
    ("f2", "F2"),
    ("f3", "F3"),
    ("f4", "F4"),
    ("f5", "F5"),
    ("f6", "F6"),
    ("f7", "F7"),
    ("f8", "F8"),
    ("f9", "F9"),
    ("f10", "F10"),
    ("f11", "F11"),
    ("f12", "F12"),
];

/// FigCraft's name for a key as GPUI reports it (`Keystroke::key`): `backspace` is `Backspace`,
/// `left` is `ArrowLeft`, and a character stays as it is. `None` for a key FigCraft has no name
/// for, such as Caps Lock.
pub fn key_name(gpui_key: &str) -> Option<String> {
    if gpui_key.chars().count() == 1 {
        return Some(gpui_key.to_string());
    }
    NAMED_KEYS
        .iter()
        .find(|(gpui, _)| *gpui == gpui_key)
        .map(|(_, figcraft)| (*figcraft).to_string())
}

/// The GPUI keystroke for a FigCraft binding: `Shift+Cmd+K` is `shift-cmd-k`. The modifiers come
/// in the order ctrl, alt, shift, cmd, as Sawit's workspace writes keys. A shifted symbol is its
/// key with Shift (`Plus` is `shift-=`), and `Delete` is the key that deletes backwards, which
/// GPUI calls `backspace`. `None` when `spec` is no binding FigCraft can read.
pub fn gpui_keystroke(spec: &str) -> Option<String> {
    let (mods, key) = keymap::parse(spec)?;
    let key = gpui_key(key)?;
    let mut keystroke = String::new();
    for (on, modifier) in [
        (mods.ctrl, "ctrl-"),
        (mods.alt, "alt-"),
        (mods.shift, "shift-"),
        (mods.cmd, "cmd-"),
    ] {
        if on {
            keystroke.push_str(modifier);
        }
    }
    keystroke.push_str(&key);
    Some(keystroke)
}

/// GPUI's name for a key as [`keymap::parse`] gives it.
fn gpui_key(key: &str) -> Option<String> {
    // FigCraft reads Backspace and Delete as one key, `Delete`: on a Mac keyboard that is ⌫.
    if key == "Delete" {
        return Some("backspace".into());
    }
    if key.chars().count() == 1 {
        return Some(key.to_ascii_lowercase());
    }
    // A binding may write a named key in any case (`parse` keeps it as written).
    NAMED_KEYS
        .iter()
        .find(|(_, figcraft)| figcraft.eq_ignore_ascii_case(key))
        .map(|(gpui, _)| (*gpui).to_string())
}

/// The US keyboard's digit row with Shift, each symbol and its digit. GPUI reports ⇧0 by the
/// character it types, `)`, with Shift off, and FigCraft's [`keymap::normalize`] reads that as a
/// key of its own: of these shifted digits it only knows `!`.
const SHIFTED_DIGITS: &[(&str, &str)] = &[
    (")", "0"),
    ("@", "2"),
    ("#", "3"),
    ("$", "4"),
    ("%", "5"),
    ("^", "6"),
    ("&", "7"),
    ("*", "8"),
    ("(", "9"),
];

/// `key` with `mods` as FigCraft's own keyboard names them: a letter in upper case, as vector edit
/// mode's keys expect (`A`), and a symbol from the US digit row as its digit with Shift (`)` is
/// ⇧0). Other keys stay as they are.
pub(super) fn figcraft_key(key: &str, mut mods: Mods) -> (Cow<'_, str>, Mods) {
    if let Some(&(_, digit)) = SHIFTED_DIGITS.iter().find(|&&(symbol, _)| symbol == key) {
        mods.shift = true;
        return (Cow::Borrowed(digit), mods);
    }
    if matches!(key.as_bytes(), [b'a'..=b'z']) {
        return (Cow::Owned(key.to_ascii_uppercase()), mods);
    }
    (Cow::Borrowed(key), mods)
}
