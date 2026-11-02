//! The form's warnings: another binding of the same keys in the context, which both stay, and a
//! chord beside its first keys bound alone.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use super::{open, save};
use crate::keymap::editor::KeymapEditor;
use crate::keymap::tests::{Scratch, runs};

/// Creates a binding through the form, as Create Keybinding does: the action typed, the keys
/// recorded, the context written.
fn create(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    editor: &Entity<KeymapEditor>,
    action: &str,
    keys: &str,
    context: &str,
) {
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.create(&super::CreateBinding, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let form = editor.read(cx).form.clone().expect("the form is open");
        let (action_field, context_field, recorder) = {
            let form = form.read(cx);
            (form.action.clone(), form.context.clone(), form.keys.clone())
        };
        action_field.update(cx, |a, cx| a.set_value(action.to_string(), window, cx));
        context_field.update(cx, |c, cx| c.set_value(context.to_string(), window, cx));
        let focus = recorder.read(cx).focus_handle().clone();
        window.focus(&focus, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window, keys);
    save(cx, window, editor);
}

/// Hand check PR2-1: a second binding on ⌃⌥W in the same context, for another action. The
/// form warns of the clash, saving keeps both, and both rows show ⚠.
#[gpui_kit::test]
fn a_second_binding_of_the_same_keys_keeps_the_first(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-same-keys");
    let (window, editor) = open(cx, &scratch);
    create(
        cx,
        window,
        &editor,
        "keymap_editor_test::CloseAll",
        "ctrl-alt-w",
        "Workspace",
    );
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.create(&super::CreateBinding, window, cx)
        });
        let form = editor.read(cx).form.clone().unwrap();
        let (context_field, recorder) = {
            let form = form.read(cx);
            (form.context.clone(), form.keys.clone())
        };
        context_field.update(cx, |c, cx| c.set_value("Workspace", window, cx));
        recorder.update(cx, |r, cx| r.set_keystrokes("ctrl-alt-w", cx));
        let (same, _) = form.read(cx).clashes(cx);
        assert_eq!(same, ["keymap editor test: close all"]);
        editor.update(cx, |editor, cx| editor.close_form(window, cx));
    })
    .unwrap();
    create(
        cx,
        window,
        &editor,
        "keymap_editor_test::Other",
        "ctrl-alt-w",
        "Workspace",
    );
    cx.update(|cx| {
        let editor = editor.read(cx);
        let rows: Vec<_> = editor
            .rows
            .iter()
            .filter(|row| row.keystrokes.as_ref() == "ctrl-alt-w")
            .collect();
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows.iter().all(|row| editor.row_conflicts(row)));
        assert_eq!(
            runs(cx, "ctrl-alt-w"),
            ["keymap_editor_test::Other", "keymap_editor_test::CloseAll"]
        );
    });
}

/// The form warns when the keys and another binding in the context are a chord and its first
/// keys: ⌘K bound alone beside ⌘K W, or ⌘J X beside ⌘J.
#[gpui_kit::test]
fn the_form_warns_of_a_chord_on_bound_keys(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-chords");
    let (window, editor) = open(cx, &scratch);
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.create(&super::CreateBinding, window, cx)
        });
        let form = editor.read(cx).form.clone().unwrap();
        let (context, recorder) = {
            let form = form.read(cx);
            (form.context.clone(), form.keys.clone())
        };
        context.update(cx, |c, cx| c.set_value("Workspace", window, cx));
        recorder.update(cx, |r, cx| r.set_keystrokes("cmd-k", cx));
        let (longer, shorter) = form.read(cx).chord_clashes(cx);
        assert_eq!(longer, ["⌘K W (keymap editor test: close all)"]);
        assert!(shorter.is_empty(), "{shorter:?}");
        recorder.update(cx, |r, cx| r.set_keystrokes("cmd-j x", cx));
        let (longer, shorter) = form.read(cx).chord_clashes(cx);
        assert!(longer.is_empty(), "{longer:?}");
        assert_eq!(shorter, ["⌘J (keymap editor test: toggle dock)"]);
        // Another context doesn't count.
        context.update(cx, |c, cx| c.set_value("Editor", window, cx));
        assert_eq!(form.read(cx).chord_clashes(cx), (Vec::new(), Vec::new()));
    })
    .unwrap();
}

/// The notes under the form's fields follow the keys, the context (which a context button sets
/// without telling the form) and the keymap, though the form works them out once for each rather
/// than at every frame.
#[gpui_kit::test]
fn the_forms_notes_follow_its_keys_context_and_the_keymap(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-notes");
    let (window, editor) = open(cx, &scratch);
    let (context, recorder) = cx
        .update_window(window, |_, window, cx| {
            editor.update(cx, |editor, cx| {
                editor.create(&super::CreateBinding, window, cx)
            });
            let form = editor.read(cx).form.clone().unwrap();
            let form = form.read(cx);
            (form.context.clone(), form.keys.clone())
        })
        .unwrap();
    let notes = |cx: &mut TestAppContext| -> Vec<&'static str> {
        cx.update(|cx| {
            let form = editor.read(cx).form.clone().unwrap();
            let notes = form.update(cx, |form, cx| form.clash_notes(cx));
            notes.into_iter().map(|(id, _)| id).collect()
        })
    };
    cx.update_window(window, |_, window, cx| {
        context.update(cx, |c, cx| c.set_value("Workspace", window, cx));
        recorder.update(cx, |r, cx| r.set_keystrokes("cmd-k", cx));
    })
    .unwrap();
    assert_eq!(notes(cx), ["binding-starts-chords"]);
    cx.update(|cx| recorder.update(cx, |r, cx| r.set_keystrokes("cmd-j", cx)));
    assert_eq!(notes(cx), ["binding-clash"]);
    cx.update_window(window, |_, window, cx| {
        context.update(cx, |c, cx| c.set_value("Editor", window, cx))
    })
    .unwrap();
    assert_eq!(notes(cx), ["binding-elsewhere"]);
    // Another binding of the keys there, saved meanwhile.
    cx.update(|cx| {
        crate::keymap::store(cx).update(cx, |store, cx| {
            store.add(
                Some("Editor"),
                "cmd-j",
                "keymap_editor_test::Other",
                None,
                cx,
            )
        })
    })
    .unwrap();
    assert_eq!(notes(cx), ["binding-clash"]);
}
