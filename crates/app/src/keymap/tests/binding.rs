//! How the keys are bound: the canvas lets through a user's chord on its keys, and the defaults
//! follow the keyboard layout.

use gpui_kit::{KeyBinding, KeyContext, Keystroke, OperatingSystem, TestAppContext};

use super::{Close, CloseAll, Other, Scratch, default_row, runs};
use crate::keymap::{Source, init_at, store};

/// A chord the user binds that starts with a key the canvas binds alone (⌘K, its Actions
/// palette) doesn't keep that key waiting on the canvas for the chord's second key: the
/// canvas lets it through, as it does the workspace's own ⌘K chords. Off the canvas the
/// chord works.
#[gpui_kit::test]
fn a_users_chord_doesnt_keep_the_canvas_waiting(cx: &mut TestAppContext) {
    let scratch = Scratch::new("chord-canvas");
    let store = cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("cmd-k", Other, Some("Board && !BoardText")),
            KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
            KeyBinding::new("cmd-k w", gpui_kit::NoAction, Some("Board")),
        ]);
        init_at(scratch.file(), cx);
        store(cx)
    });
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(Some("Workspace"), "cmd-k l", "keymap_test::Close", None, cx)
            })
            .unwrap();
        let typed = |keys: &str| -> Vec<Keystroke> {
            keys.split(' ')
                .map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).unwrap())
                .collect()
        };
        let canvas = ["Workspace", "Board"].map(|c| KeyContext::parse(c).unwrap());
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let (bindings, waits) = keymap.bindings_for_input(&typed("cmd-k"), &canvas);
        assert!(!waits, "⌘K waits on the canvas");
        assert_eq!(
            bindings.first().map(|b| b.action().name()),
            Some("keymap_test::Other")
        );
        let workspace = [KeyContext::parse("Workspace").unwrap()];
        let (bindings, _) = keymap.bindings_for_input(&typed("cmd-k l"), &workspace);
        assert_eq!(
            bindings.first().map(|b| b.action().name()),
            Some("keymap_test::Close")
        );
    });
}

/// On a layout that types some keys elsewhere (German), the defaults are bound where the
/// layout types them, as macOS menus have them: ⇧⌘. (`cmd->`) as `cmd-:`, and the window
/// cycling key `cmd->` (⇧⌘<) is left to macOS. A binding the user turns off there stays off.
#[gpui_kit::test]
fn the_defaults_follow_the_keyboard_layout(cx: &mut TestAppContext) {
    let scratch = Scratch::new("layout");
    let store = cx.update(|cx| {
        cx.bind_keys([
            KeyBinding::new("cmd->", Other, Some("Workspace")),
            KeyBinding::new("cmd-w", Close, Some("Workspace")),
        ]);
        init_at(scratch.file(), cx);
        store(cx)
    });
    let german = |c: char| match c {
        '>' => Some(':'),
        '<' => Some(';'),
        _ => None,
    };
    cx.update(|cx| {
        assert_eq!(runs(cx, "cmd->"), ["keymap_test::Other"]);
        store.update(cx, |s, cx| s.relayout_with(german, cx));
        assert_eq!(runs(cx, "cmd-:"), ["keymap_test::Other"]);
        assert!(runs(cx, "cmd->").is_empty());
        assert_eq!(runs(cx, "cmd-w"), ["keymap_test::Close"]);
        // Removed in the editor, it is unbound where it is bound.
        let other = default_row(&store, cx, "keymap_test::Other");
        store.update(cx, |s, cx| s.remove(other, cx)).unwrap();
        assert!(runs(cx, "cmd-:").is_empty());
        assert!(matches!(
            default_row(&store, cx, "keymap_test::Other"),
            Source::Removed { .. }
        ));
    });
}
