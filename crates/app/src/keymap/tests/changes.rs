//! Changes made through the store, as the editor makes them: a default moved to other keys,
//! removed and restored, and the user's own bindings of the same keys kept beside it.

use gpui_kit::{App, Entity, TestAppContext};

use super::{Scratch, default_row, runs, start};
use crate::keymap::{KeymapStore, Source};

#[gpui_kit::test]
fn changing_a_default_moves_it_to_the_new_keys(cx: &mut TestAppContext) {
    let scratch = Scratch::new("change");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        let source = default_row(&store, cx, "keymap_test::Close");
        store
            .update(cx, |store, cx| {
                store.change(
                    source,
                    Some("Workspace"),
                    "cmd-e",
                    "keymap_test::Close",
                    None,
                    cx,
                )
            })
            .unwrap();
        assert_eq!(runs(cx, "cmd-e"), ["keymap_test::Close"]);
        assert!(runs(cx, "cmd-w").is_empty(), "{:?}", runs(cx, "cmd-w"));
        // The default shows as removed, beside the user's binding.
        let rows = store.read(cx).rows(false, cx);
        let close: Vec<_> = rows
            .iter()
            .filter(|r| r.action.as_ref() == "keymap_test::Close")
            .map(|r| (r.keystrokes.to_string(), r.source))
            .collect();
        assert_eq!(close.len(), 2, "{close:?}");
        assert!(
            close
                .iter()
                .any(|(k, s)| k == "cmd-w" && matches!(s, Source::Removed { .. }))
        );
        assert!(
            close
                .iter()
                .any(|(k, s)| k == "cmd-e" && matches!(s, Source::User(_)))
        );
    });
    let text = std::fs::read_to_string(scratch.file()).unwrap();
    assert!(text.contains("zed::Unbind"), "{text}");
}

#[gpui_kit::test]
fn a_removed_default_comes_back(cx: &mut TestAppContext) {
    let scratch = Scratch::new("restore");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        let source = default_row(&store, cx, "keymap_test::CloseAll");
        store
            .update(cx, |store, cx| store.remove(source, cx))
            .unwrap();
        assert!(runs(cx, "cmd-k w").is_empty());
        let removed = default_row(&store, cx, "keymap_test::CloseAll");
        assert!(matches!(removed, Source::Removed { .. }), "{removed:?}");
        store
            .update(cx, |store, cx| store.remove(removed, cx))
            .unwrap();
        assert_eq!(runs(cx, "cmd-k w"), ["keymap_test::CloseAll"]);
        assert!(matches!(
            default_row(&store, cx, "keymap_test::CloseAll"),
            Source::Default(_)
        ));
    });
}

/// The rows on `keystrokes`: each action's name and where it comes from.
fn rows_on(store: &Entity<KeymapStore>, cx: &App, keystrokes: &str) -> Vec<(String, Source)> {
    store
        .read(cx)
        .rows(false, cx)
        .into_iter()
        .filter(|row| row.keystrokes.as_ref() == keystrokes)
        .map(|row| (row.action.to_string(), row.source))
        .collect()
}

fn user_row(store: &Entity<KeymapStore>, cx: &App, keystrokes: &str, action: &str) -> Source {
    rows_on(store, cx, keystrokes)
        .into_iter()
        .find(|(name, source)| name == action && matches!(source, Source::User(_)))
        .map(|(_, source)| source)
        .unwrap_or_else(|| panic!("a user row {keystrokes} {action}"))
}

/// Hand check PR2-1: a second binding of the same keys in the same context is added beside
/// the first, which the form's clash warning promises: both rows stay, and the one bound last
/// runs first.
#[gpui_kit::test]
fn two_bindings_of_the_same_keys_both_stay(cx: &mut TestAppContext) {
    let scratch = Scratch::new("same-keys");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        for action in ["keymap_test::CloseAll", "keymap_test::Other"] {
            store
                .update(cx, |s, cx| {
                    s.add(Some("Workspace"), "ctrl-alt-w", action, None, cx)
                })
                .unwrap();
        }
        let rows = rows_on(&store, cx, "ctrl-alt-w");
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(
            runs(cx, "ctrl-alt-w"),
            ["keymap_test::Other", "keymap_test::CloseAll"]
        );
    });
}

/// Removing a default (or changing its keys, `change`) turns off only the default: the
/// user's own binding of the same keys in the same context stays and still runs.
fn turn_off_a_default_the_user_rebound(cx: &mut TestAppContext, change: bool) {
    let scratch = Scratch::new(if change { "keep-change" } else { "keep-remove" });
    let store = start(cx, &scratch);
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(Some("Workspace"), "cmd-w", "keymap_test::Other", None, cx)
            })
            .unwrap();
        let close = default_row(&store, cx, "keymap_test::Close");
        store
            .update(cx, |s, cx| {
                if change {
                    s.change(
                        close,
                        Some("Workspace"),
                        "cmd-e",
                        "keymap_test::Close",
                        None,
                        cx,
                    )
                } else {
                    s.remove(close, cx)
                }
            })
            .unwrap();
        assert_eq!(runs(cx, "cmd-w"), ["keymap_test::Other"]);
        user_row(&store, cx, "cmd-w", "keymap_test::Other");
        let on_cmd_w = rows_on(&store, cx, "cmd-w");
        assert!(
            on_cmd_w
                .iter()
                .any(|(name, source)| name == "keymap_test::Close"
                    && matches!(source, Source::Removed { .. })),
            "{on_cmd_w:?}"
        );
        if change {
            assert_eq!(runs(cx, "cmd-e"), ["keymap_test::Close"]);
        }
    });
}

#[gpui_kit::test]
fn removing_a_default_keeps_the_users_binding_of_its_keys(cx: &mut TestAppContext) {
    turn_off_a_default_the_user_rebound(cx, false);
}

#[gpui_kit::test]
fn changing_a_default_keeps_the_users_binding_of_its_keys(cx: &mut TestAppContext) {
    turn_off_a_default_the_user_rebound(cx, true);
}

/// Changing a user's binding to keys another of the user's bindings has keeps the other one.
#[gpui_kit::test]
fn changing_a_binding_onto_anothers_keys_keeps_the_other(cx: &mut TestAppContext) {
    let scratch = Scratch::new("onto-another");
    let store = start(cx, &scratch);
    cx.update(|cx| {
        store
            .update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-a",
                    "keymap_test::CloseAll",
                    None,
                    cx,
                )
            })
            .unwrap();
        store
            .update(cx, |s, cx| {
                s.add(
                    Some("Workspace"),
                    "ctrl-alt-b",
                    "keymap_test::Other",
                    None,
                    cx,
                )
            })
            .unwrap();
        let first = user_row(&store, cx, "ctrl-alt-a", "keymap_test::CloseAll");
        store
            .update(cx, |s, cx| {
                s.change(
                    first,
                    Some("Workspace"),
                    "ctrl-alt-b",
                    "keymap_test::CloseAll",
                    None,
                    cx,
                )
            })
            .unwrap();
        assert!(rows_on(&store, cx, "ctrl-alt-a").is_empty());
        assert_eq!(
            runs(cx, "ctrl-alt-b"),
            ["keymap_test::CloseAll", "keymap_test::Other"]
        );
    });
}
