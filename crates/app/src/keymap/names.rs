//! How the keymap editor writes actions and keystrokes for people.

use std::sync::atomic::{AtomicBool, Ordering};

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
/// named one as its symbol (`↩`, `⌫`, `→`) and a letter in upper case. A shifted symbol reads as
/// the keys pressed for it, on a US keyboard as the canvas's bindings assume: GPUI reports ⇧⌘. as
/// `cmd->` with Shift off, which is how it is bound, and `cmd->` reads `⇧⌘.`. On a layout that
/// moves keys for shortcuts the key is the character it types, and reads as it is.
fn display_one(modifiers: &Modifiers, key: &str) -> String {
    display_key(modifiers, key, !LAYOUT_MOVES_KEYS.load(Ordering::Relaxed))
}

/// [`display_one`], with a shifted symbol read as the keys pressed for it on a US keyboard when
/// `us_keys`.
fn display_key(modifiers: &Modifiers, key: &str, us_keys: bool) -> String {
    let mut chars = key.chars();
    let unshifted = match (chars.next(), chars.next()) {
        (Some(c), None) if us_keys && !modifiers.shift => {
            sawit_board::shortcuts::unshifted(c).map(String::from)
        }
        _ => None,
    };
    let key = unshifted.as_deref().unwrap_or(key);
    let mut out = String::new();
    for (on, symbol) in [
        (modifiers.function, "fn "),
        (modifiers.control, "⌃"),
        (modifiers.alt, "⌥"),
        (modifiers.shift || unshifted.is_some(), "⇧"),
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

/// `keystrokes` (as the keymap file writes them) with each key that `moves` moves on the keyboard
/// layout put where it is typed there (Apple's key equivalents: on a German layout `>` is typed
/// as `:`, so `cmd->` becomes `cmd-:`). `None` when no key moves.
pub fn relocate(keystrokes: &str, moves: impl Fn(char) -> Option<char>) -> Option<String> {
    let mut moved = false;
    let keys: Vec<String> = keystrokes
        .split_whitespace()
        .map(
            |source| match Keystroke::parse_with_os(source, OperatingSystem::Mac) {
                Ok(mut keystroke) => {
                    let mut chars = keystroke.key.chars();
                    if let (Some(c), None) = (chars.next(), chars.next())
                        && let Some(to) = moves(c)
                    {
                        keystroke.key = to.to_string();
                        moved = true;
                    }
                    keystroke.unparse(OperatingSystem::Mac)
                }
                Err(_) => source.to_string(),
            },
        )
        .collect();
    moved.then(|| keys.join(" "))
}

/// Whether the keyboard layout moves keys for shortcuts ([`relocate`]): its bindings' keys are
/// then the characters it types, shown as they are. On a layout that moves none (US) a shifted
/// symbol is shown as the keys pressed for it (`cmd->` as ⇧⌘.).
static LAYOUT_MOVES_KEYS: AtomicBool = AtomicBool::new(false);

/// Says whether the keyboard layout moves keys for shortcuts ([`LAYOUT_MOVES_KEYS`]).
pub fn set_layout_moves_keys(moves: bool) {
    LAYOUT_MOVES_KEYS.store(moves, Ordering::Relaxed);
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

    /// GPUI reports a shifted symbol with Shift off (⇧⌘. is `cmd->`), and bindings are written
    /// that way; they read as the keys pressed.
    #[test]
    fn displays_shifted_symbols_as_the_keys_pressed() {
        let keys = |text: &str| -> Vec<Keystroke> {
            text.split(' ')
                .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
                .collect()
        };
        assert_eq!(display_typed(&keys("cmd->")), "⇧⌘.");
        assert_eq!(display_typed(&keys("ctrl-?")), "⌃⇧/");
        assert_eq!(display_typed(&keys("alt-<")), "⌥⇧,");
        assert_eq!(display_typed(&keys("cmd-|")), "⇧⌘\\");
        assert_eq!(display_typed(&keys("!")), "⇧1");
        assert_eq!(display_typed(&keys("cmd-k cmd-+")), "⌘K ⇧⌘=");
        // Unshifted keys stay as they are, and so does a symbol written with Shift.
        assert_eq!(display_typed(&keys("cmd-.")), "⌘.");
        assert_eq!(display_typed(&keys("cmd--")), "⌘-");
        assert_eq!(display_typed(&keys("cmd-shift->")), "⇧⌘>");
    }

    /// Part of Apple's German layout table (gpui-pre-macos `keyboard.rs`).
    fn german(c: char) -> Option<char> {
        [
            ('<', ';'),
            ('>', ':'),
            ('[', 'ö'),
            (']', 'ä'),
            ('\\', '#'),
            ('|', '\''),
            ('`', '<'),
            ('~', '>'),
        ]
        .into_iter()
        .find(|(from, _)| *from == c)
        .map(|(_, to)| to)
    }

    /// Keys a German layout can't type as a US one does move where macOS puts them: ⇧⌘. (bound
    /// `cmd->`) is typed `cmd-:` there, and the window cycling keys ⌘` and ⌘~ are `cmd-<` and
    /// `cmd->`.
    #[test]
    fn relocates_keys_as_the_layout_types_them() {
        assert_eq!(relocate("cmd->", german).as_deref(), Some("cmd-:"));
        assert_eq!(relocate("cmd-<", german).as_deref(), Some("cmd-;"));
        assert_eq!(
            relocate("ctrl-cmd-[", german).as_deref(),
            Some("ctrl-cmd-ö")
        );
        assert_eq!(
            relocate("cmd-k cmd-]", german).as_deref(),
            Some("cmd-k cmd-ä")
        );
        assert_eq!(relocate("cmd-\\", german).as_deref(), Some("cmd-#"));
        // Letters, named keys and keys the layout types as a US one does stay.
        assert_eq!(relocate("cmd-j", german), None);
        assert_eq!(relocate("cmd-k e", german), None);
        assert_eq!(relocate("shift-cmd-enter", german), None);
        assert_eq!(relocate("cmd--", german), None);
    }

    /// On a layout that moves keys for shortcuts, a key reads as the character the layout types.
    #[test]
    fn displays_a_moved_key_as_typed() {
        let keys = |text: &str| Keystroke::parse_with_os(text, OperatingSystem::Mac).unwrap();
        let shown = |text: &str, us_keys| {
            let key = keys(text);
            display_key(&key.modifiers, &key.key, us_keys)
        };
        assert_eq!(shown("cmd-:", false), "⌘:");
        assert_eq!(shown("cmd->", false), "⌘>");
        assert_eq!(shown("cmd->", true), "⇧⌘.");
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
