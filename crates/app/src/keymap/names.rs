//! How the keymap editor writes actions and keystrokes for people.

use gpui_kit::{KeybindingKeystroke, Keystroke, Modifiers, OperatingSystem};

/// `workspace::CloseActiveTab` as `workspace: close active tab`, `file_editor::Save` as
/// `file editor: save`: Zed's command palette's names.
pub fn humanize_action(name: &str) -> String {
    let (namespace, action) = name.rsplit_once("::").unwrap_or(("", name));
    let mut out = String::with_capacity(name.len() + 8);
    for (ix, part) in namespace.split("::").enumerate() {
        if ix > 0 {
            out.push(' ');
        }
        out.push_str(&part.replace('_', " "));
    }
    if !out.is_empty() {
        out.push_str(": ");
    }
    let mut previous: Option<char> = None;
    for c in action.chars() {
        if c == '_' {
            out.push(' ');
        } else if c.is_uppercase() {
            if previous.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit()) {
                out.push(' ');
            }
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
        previous = Some(c);
    }
    out
}

/// Keystrokes as the keymap file writes them: `cmd-k e`.
pub fn unparse(keystrokes: &[KeybindingKeystroke]) -> String {
    keystrokes
        .iter()
        .map(|k| k.inner().unparse(OperatingSystem::Mac))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Keystrokes as a Mac menu writes them, a space between the keys of a chord: `⌘K E`, `⇧⌘↩`.
pub fn display(keystrokes: &[KeybindingKeystroke]) -> String {
    keystrokes
        .iter()
        .map(|k| display_one(k.modifiers(), k.key()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`display`] for keystrokes that aren't a binding's.
pub fn display_typed(keystrokes: &[Keystroke]) -> String {
    keystrokes
        .iter()
        .map(|k| display_one(&k.modifiers, &k.key))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One keystroke as a Mac menu writes it: the modifiers in the order ⌃⌥⇧⌘, then the key, a
/// named one as its symbol (`↩`, `⌫`, `→`) and a letter in upper case.
fn display_one(modifiers: &Modifiers, key: &str) -> String {
    let mut out = String::new();
    for (on, symbol) in [
        (modifiers.function, "fn "),
        (modifiers.control, "⌃"),
        (modifiers.alt, "⌥"),
        (modifiers.shift, "⇧"),
        (modifiers.platform, "⌘"),
    ] {
        if on {
            out.push_str(symbol);
        }
    }
    let named = match key {
        "enter" => Some("↩"),
        "backspace" => Some("⌫"),
        "delete" => Some("⌦"),
        "escape" => Some("⎋"),
        "tab" => Some("⇥"),
        "space" => Some("Space"),
        "up" => Some("↑"),
        "down" => Some("↓"),
        "left" => Some("←"),
        "right" => Some("→"),
        "home" => Some("↖"),
        "end" => Some("↘"),
        "pageup" => Some("⇞"),
        "pagedown" => Some("⇟"),
        _ => None,
    };
    match named {
        Some(symbol) => out.push_str(symbol),
        None => out.push_str(&key.to_uppercase()),
    }
    out
}

/// `text` (keystrokes as the keymap file writes them, in any order of modifiers) as GPUI writes
/// them back, so that two spellings of the same keys compare equal. `None` when `text` is no
/// keystrokes.
pub fn normalize_keystrokes(text: &str) -> Option<String> {
    let keys: Option<Vec<String>> = text
        .split_whitespace()
        .map(|source| {
            Keystroke::parse_with_os(source, OperatingSystem::Mac)
                .ok()
                .map(|k| k.unparse(OperatingSystem::Mac))
        })
        .collect();
    keys.filter(|keys| !keys.is_empty())
        .map(|keys| keys.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanizes_like_zed() {
        assert_eq!(
            humanize_action("workspace::CloseActiveTab"),
            "workspace: close active tab"
        );
        assert_eq!(humanize_action("file_editor::Save"), "file editor: save");
        assert_eq!(
            humanize_action("input::MoveToNextWord"),
            "input: move to next word"
        );
        assert_eq!(humanize_action("Quit"), "quit");
    }

    #[test]
    fn displays_like_a_mac_menu() {
        let keys = |text: &str| -> Vec<Keystroke> {
            text.split(' ')
                .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
                .collect()
        };
        assert_eq!(display_typed(&keys("cmd-shift-enter")), "⇧⌘↩");
        assert_eq!(display_typed(&keys("cmd-k e")), "⌘K E");
        assert_eq!(display_typed(&keys("shift-f10")), "⇧F10");
        assert_eq!(display_typed(&keys("ctrl-alt-cmd-backspace")), "⌃⌥⌘⌫");
    }

    #[test]
    fn normalizes_modifier_order() {
        assert_eq!(
            normalize_keystrokes("shift-cmd-k  e"),
            normalize_keystrokes("cmd-shift-k e")
        );
        assert_eq!(normalize_keystrokes(""), None);
        assert_eq!(normalize_keystrokes("cmd-nope-x"), None);
    }
}
