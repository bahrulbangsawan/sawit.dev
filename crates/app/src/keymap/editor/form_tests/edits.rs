//! A binding edited through the form: new keys recorded over the row's, a canvas key pointed at
//! another command, a form opened before the file changed, an action bound from its own row, and
//! ⎋ and ↩ in the keys field.

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, KeyContext, Keystroke, OperatingSystem,
    TestAppContext,
};

use super::{open, save};
use crate::keymap::editor::KeymapEditor;
use crate::keymap::tests::{Scratch, runs};

/// The canvas commands `keys` runs on the canvas, by id.
fn canvas_runs(cx: &App, keys: &str) -> Vec<String> {
    let typed = [Keystroke::parse_with_os(keys, OperatingSystem::Mac).unwrap()];
    let contexts = ["Workspace", "Board"].map(|c| KeyContext::parse(c).unwrap());
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (bindings, _) = keymap.bindings_for_input(&typed, &contexts);
    bindings
        .iter()
        .filter_map(|b| b.action().as_any().downcast_ref::<crate::board::Command>())
        .map(|command| command.id.clone())
        .collect()
}

/// Opens the form on the first row of `action`, as its Edit button does.
fn edit(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    editor: &Entity<KeymapEditor>,
    action: &str,
) {
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            let ix = editor
                .rows
                .iter()
                .position(|r| r.action.as_ref() == action)
                .unwrap_or_else(|| panic!("a row for {action}"));
            editor.edit(ix, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
}

/// The keys the open form's recorder holds, as the keymap file writes them.
fn recorded(cx: &mut TestAppContext, editor: &Entity<KeymapEditor>) -> String {
    cx.update(|cx| {
        let form = editor.read(cx).form.clone().expect("the form is open");
        form.read(cx).keys.read(cx).keystrokes()
    })
}

/// Hand check PR2-1: Edit a default (⌘J), press ⌃⌥J, Save. The new keys replace the row's,
/// which the form shows until the first key: ⌃⌥J alone toggles the dock, and ⌘J is unbound.
#[gpui_kit::test]
fn editing_a_binding_records_only_the_new_keys(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-new-keys");
    let (window, editor) = open(cx, &scratch);
    edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
    assert_eq!(
        recorded(cx, &editor),
        "cmd-j",
        "the form starts with the row's keys"
    );
    cx.simulate_keystrokes(window, "ctrl-alt-j");
    assert_eq!(recorded(cx, &editor), "ctrl-alt-j");
    // The next key makes a chord of them.
    cx.simulate_keystrokes(window, "ctrl-alt-k");
    assert_eq!(recorded(cx, &editor), "ctrl-alt-j ctrl-alt-k");
    // Back to the field (a click gives it the keyboard), the next key starts again.
    cx.update_window(window, |_, window, cx| {
        let form = editor.read(cx).form.clone().unwrap();
        let context = form.read(cx).context.clone();
        context.update(cx, |context, cx| context.focus(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        let form = editor.read(cx).form.clone().unwrap();
        let keys = form.read(cx).keys.read(cx).focus_handle().clone();
        window.focus(&keys, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "ctrl-alt-j");
    assert_eq!(recorded(cx, &editor), "ctrl-alt-j");
    save(cx, window, &editor);
    cx.update(|cx| {
        assert!(
            editor.read(cx).form.is_none(),
            "{:?}",
            editor.read(cx).error
        );
        assert_eq!(runs(cx, "ctrl-alt-j"), ["keymap_editor_test::ToggleDock"]);
        assert!(runs(cx, "cmd-j").is_empty(), "{:?}", runs(cx, "cmd-j"));
    });
    let text = std::fs::read_to_string(scratch.file()).unwrap();
    assert!(!text.contains("cmd-j ctrl-alt-j"), "{text}");
}

/// A canvas key can be pointed at another command: its row shows the command's id as its
/// arguments, and a new id is saved as the user's binding of the key in place of the default.
#[gpui_kit::test]
fn a_canvas_key_can_run_another_command(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-canvas-id");
    let (window, editor) = open(cx, &scratch);
    edit(cx, window, &editor, "board::Command");
    cx.update_window(window, |_, window, cx| {
        let form = editor.read(cx).form.clone().unwrap();
        let args = form.read(cx).args.clone();
        assert_eq!(args.read(cx).value().as_ref(), r#"{"id":"tool.rectangle"}"#);
        args.update(cx, |a, cx| {
            a.set_value(r#"{"id": "tool.ellipse"}"#, window, cx)
        });
    })
    .unwrap();
    save(cx, window, &editor);
    cx.update(|cx| {
        let editor = editor.read(cx);
        let refused = editor.form.as_ref().map(|form| form.read(cx).error.clone());
        assert_eq!(refused, None, "the form refused");
        assert_eq!(canvas_runs(cx, "r"), ["tool.ellipse"]);
        let sources: Vec<_> = editor
            .rows
            .iter()
            .filter(|row| row.keystrokes.as_ref() == "r")
            .map(|row| (row.args.clone().unwrap_or_default(), row.source))
            .collect();
        assert!(
            sources
                .iter()
                .any(|(args, source)| args.contains("tool.rectangle")
                    && matches!(source, super::Source::Removed { .. })),
            "{sources:?}"
        );
        assert!(
            !sources
                .iter()
                .any(|(_, source)| matches!(source, super::Source::Default(_)))
        );
    });
}

/// A form opened before keymap.json changed on disk never changes another binding: the
/// place in the file its row came from may hold another binding now.
#[gpui_kit::test]
fn a_form_opened_before_the_file_changed_changes_nothing_else(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-stale");
    std::fs::create_dir_all(&scratch.0).unwrap();
    std::fs::write(
        scratch.file(),
        r#"[{"context": "Workspace", "bindings": {"ctrl-alt-a": "keymap_editor_test::Other"}}]"#,
    )
    .unwrap();
    let (window, editor) = open(cx, &scratch);
    edit(cx, window, &editor, "keymap_editor_test::Other");
    // Another program writes the file, and the editor reads it (its look once a second).
    let by_hand = r#"[{"context": "Workspace", "bindings": {"ctrl-alt-z": "keymap_editor_test::ToggleDock"}}]"#;
    std::fs::write(scratch.file(), by_hand).unwrap();
    cx.update(|cx| crate::keymap::store(cx).update(cx, |s, cx| s.reload(cx)));
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "ctrl-alt-q");
    save(cx, window, &editor);
    cx.update(|cx| {
        let form = editor.read(cx).form.clone().expect("the form stays open");
        assert!(form.read(cx).error.is_some());
        assert_eq!(runs(cx, "ctrl-alt-z"), ["keymap_editor_test::ToggleDock"]);
        assert!(runs(cx, "ctrl-alt-q").is_empty());
    });
    assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
}

/// An action with no binding gets one from its own row (Show Actions Without a Binding, then
/// the row's Edit: "Add a Keybinding for …"): such a row has no place in the file to go stale.
/// For a canvas command without a default key, that row is the editor's way to bind it.
#[gpui_kit::test]
fn an_unbound_row_gets_a_binding(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-unbound");
    let (window, editor) = open(cx, &scratch);
    cx.update(|cx| editor.update(cx, |e, cx| e.toggle_unbound(cx)));
    cx.run_until_parked();
    let source = cx.update(|cx| {
        editor
            .read(cx)
            .rows
            .iter()
            .find(|r| r.action.as_ref() == "keymap_editor_test::Other")
            .map(|r| r.source)
    });
    assert_eq!(source, Some(super::Source::Unbound));
    edit(cx, window, &editor, "keymap_editor_test::Other");
    cx.simulate_keystrokes(window, "ctrl-alt-o");
    assert_eq!(recorded(cx, &editor), "ctrl-alt-o");
    save(cx, window, &editor);
    cx.update(|cx| {
        let e = editor.read(cx);
        let refused = e.form.as_ref().and_then(|f| f.read(cx).error.clone());
        assert!(e.form.is_none(), "the form refused: {refused:?}");
        assert_eq!(runs(cx, "ctrl-alt-o"), ["keymap_editor_test::Other"]);
    });

    // A canvas command with no key: the ellipse tool (only R, the rectangle's, is bound).
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            let ix = editor
                .rows
                .iter()
                .position(|r| {
                    r.source == super::Source::Unbound
                        && r.args.as_deref() == Some(r#"{"id":"tool.ellipse"}"#)
                })
                .expect("the ellipse tool's row");
            editor.edit(ix, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "ctrl-alt-e");
    save(cx, window, &editor);
    cx.update(|cx| {
        let e = editor.read(cx);
        let refused = e.form.as_ref().and_then(|f| f.read(cx).error.clone());
        assert!(e.form.is_none(), "the form refused: {refused:?}");
        assert_eq!(canvas_runs(cx, "ctrl-alt-e"), ["tool.ellipse"]);
    });
}

/// The form's keys field records every key but plain ⎋ and ↩: ⎋ stops recording, and a
/// second ⎋ cancels the form; ↩ saves it.
#[gpui_kit::test]
fn escape_and_enter_end_the_recording(cx: &mut TestAppContext) {
    let scratch = Scratch::new("form-escape-enter");
    let (window, editor) = open(cx, &scratch);
    edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
    cx.simulate_keystrokes(window, "escape");
    assert_eq!(recorded(cx, &editor), "cmd-j", "⎋ is not recorded");
    cx.simulate_keystrokes(window, "escape");
    cx.update(|cx| assert!(editor.read(cx).form.is_none(), "a second ⎋ cancels"));
    assert!(!scratch.file().exists(), "nothing is written");

    edit(cx, window, &editor, "keymap_editor_test::ToggleDock");
    cx.simulate_keystrokes(window, "ctrl-alt-j enter");
    cx.update(|cx| {
        assert!(editor.read(cx).form.is_none(), "↩ saves");
        assert_eq!(runs(cx, "ctrl-alt-j"), ["keymap_editor_test::ToggleDock"]);
    });
}
