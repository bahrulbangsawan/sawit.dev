//! The canvas's keys in the app's keymap: what each key runs on the canvas, the keys Sawit keeps,
//! and the keymap editor's rows for the canvas's commands.

use super::*;

/// The key contexts from the window's root to the focused view.
fn contexts(names: &[&str]) -> Vec<KeyContext> {
    names
        .iter()
        .map(|name| KeyContext::parse(name).unwrap())
        .collect()
}

/// The key contexts of the canvas in the workspace: gpui-kit's root, the workspace, then the
/// canvas, which is `Board BoardText` while it edits text.
fn canvas_contexts(editing: bool) -> Vec<KeyContext> {
    let canvas = if editing { "Board BoardText" } else { "Board" };
    contexts(&["Root", "Workspace", canvas])
}

/// What pressing `keystroke` (written as `gpui_keystroke` writes it) does with the focus in
/// `contexts`: the actions its bindings run, by name, and whether it waits for the next key of
/// a chord. A key no action takes goes on to the focused view's `on_key_down`.
fn resolve(cx: &App, keystroke: &str, contexts: &[KeyContext]) -> (Vec<&'static str>, bool) {
    let typed = Keystroke::parse_with_os(keystroke, OperatingSystem::Mac).unwrap();
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (bindings, pending) = keymap.bindings_for_input(&[typed], contexts);
    let actions = bindings.iter().map(|b| b.action().name()).collect();
    (actions, pending)
}

/// The action names that `keystroke` runs with the focus in `contexts`, each canvas command
/// with its id: `board::Command tool.move`.
fn runs(cx: &App, keystroke: &str, contexts: &[KeyContext]) -> (Vec<String>, bool) {
    let typed = Keystroke::parse_with_os(keystroke, OperatingSystem::Mac).unwrap();
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let (bindings, pending) = keymap.bindings_for_input(&[typed], contexts);
    let names = bindings
        .iter()
        .map(|b| match b.action().as_any().downcast_ref::<Command>() {
            Some(command) => format!("board::Command {}", command.id),
            None => b.action().name().to_string(),
        })
        .collect();
    (names, pending)
}

#[gpui_kit::test]
fn board_keys_reach_the_canvas(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);
    let wrong = cx.update(|cx| {
        let mut wrong = Vec::new();
        for key in shortcuts::key_bindings() {
            let editing = key.context == shortcuts::TEXT_CONTEXT;
            let (actions, pending) = runs(cx, &key.keystroke, &canvas_contexts(editing));
            // The canvas's command runs first, and nothing waits for a chord.
            let expected = format!("board::Command {}", key.id);
            if actions.first() != Some(&expected) || pending {
                wrong.push(format!(
                    "{} ({}, editing {editing}): {actions:?}{}",
                    key.keystroke,
                    key.id,
                    if pending { ", waits for a chord" } else { "" },
                ));
            }
        }
        wrong
    });
    assert!(wrong.is_empty(), "not the canvas's:\n{}", wrong.join("\n"));
}

/// The keys Sawit gives the canvas's commands (`shortcuts::SAWIT_KEYS`) are no keys of the
/// app's: none runs anything outside the canvas, so on the canvas none shadows a key of the
/// workspace's, the app's or gpui-kit's root. (The canvas test above proves each runs its
/// command there.)
#[gpui_kit::test]
fn sawits_canvas_keys_take_no_key_of_the_apps(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);
    let workspace = contexts(&["Root", "Workspace"]);
    cx.update(|cx| {
        for key in shortcuts::SAWIT_KEYS {
            for spec in key.keys {
                let keystroke = shortcuts::binding_keystroke(spec).unwrap();
                let (actions, pending) = resolve(cx, &keystroke, &workspace);
                assert!(
                    actions.is_empty() && !pending,
                    "{spec} ({}) is the app's: {actions:?}",
                    key.id
                );
            }
        }
    });
}

#[gpui_kit::test]
fn sawit_keeps_its_shortcuts(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);
    let workspace = contexts(&["Root", "Workspace"]);
    cx.update(|cx| {
        for keystroke in SAWIT_WINS {
            let (in_workspace, _) = resolve(cx, keystroke, &workspace);
            assert!(!in_workspace.is_empty(), "{keystroke} is no Sawit shortcut");
            // On the canvas, and while it edits text.
            for editing in [false, true] {
                let (on_canvas, pending) = resolve(cx, keystroke, &canvas_contexts(editing));
                assert_eq!(on_canvas, in_workspace, "{keystroke}, editing {editing}");
                assert!(!pending, "{keystroke} waits for a chord, editing {editing}");
            }
        }
    });
}

#[gpui_kit::test]
fn text_editing_takes_text_keys(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);
    cx.update(|cx| {
        for (keystroke, sawits, command) in [
            ("cmd-b", "workspace::ToggleSidebar", "text.bold"),
            ("alt-cmd-t", "workspace::CloseOtherTabs", "text.alignCenter"),
            (
                "shift-cmd-enter",
                "workspace::ToggleMaximizePane",
                "board.nextSticky",
            ),
        ] {
            // While text is edited they are bold, centre and the next sticky: the canvas's.
            let (editing, pending) = runs(cx, keystroke, &canvas_contexts(true));
            let expected = format!("board::Command {command}");
            assert_eq!(editing.first(), Some(&expected), "{keystroke}");
            assert!(!pending, "{keystroke}");
            // Otherwise they stay Sawit's.
            let (on_canvas, _) = resolve(cx, keystroke, &canvas_contexts(false));
            assert_eq!(on_canvas.first(), Some(&sawits), "{keystroke}");
        }
    });
}

#[gpui_kit::test]
fn a_shortcut_taken_out_of_the_keymap_stays_off_the_app(cx: &mut TestAppContext) {
    cx.update(bind_app_keys);
    cx.update(|cx| {
        // ⌘C on the canvas is FigCraft's copy, not root's input::Copy, also once the user
        // unbinds it.
        cx.bind_keys([KeyBinding::load(
            "cmd-c",
            Box::new(Unbind("board::Command".into())),
            KeyBindingContextPredicate::parse(shortcuts::CANVAS_CONTEXT)
                .ok()
                .map(std::rc::Rc::new),
            false,
            None,
            cx,
        )
        .unwrap()
        .with_meta(crate::keymap::USER)]);
        let (on_canvas, pending) = resolve(cx, "cmd-c", &canvas_contexts(false));
        assert!(on_canvas.is_empty() && !pending, "{on_canvas:?}");
    });
}

/// The canvas commands the keymap editor offers to bind when no row binds any: the ids of
/// [`super::super::unbound_rows`].
fn offered_to_bind() -> Vec<String> {
    super::super::unbound_rows(&[])
        .iter()
        .filter_map(|row| {
            let args: Value = serde_json::from_str(row.args.as_deref()?).ok()?;
            Some(args["id"].as_str()?.to_string())
        })
        .collect()
}

/// The keymap editor offers the canvas's commands that have no key, those Sawit carries out
/// itself among them (export as SVG, copy as text), and a key the user gives one carries it
/// out as its menu item does.
#[gpui_kit::test]
fn a_key_the_user_gives_a_command_sawit_carries_out_runs_it(cx: &mut TestAppContext) {
    let offered = offered_to_bind();
    for id in [
        "file.exportSvg",
        "file.exportFramesPdf",
        "edit.copyAsText",
        "edit.copyAsCss",
        "edit.copyAsSvg",
    ] {
        assert!(offered.iter().any(|o| o == id), "{id} isn't offered");
    }
    let tmp = Scratch::new("bound-copy-as");
    let (view, mut cx) = chrome_canvas(cx, &tmp);
    cx.update(|_, cx| {
        let copy_as_text = Box::new(Command {
            id: "edit.copyAsText".into(),
        });
        let context = KeyBindingContextPredicate::parse(shortcuts::CANVAS_CONTEXT)
            .ok()
            .map(std::rc::Rc::new);
        let binding = KeyBinding::load("ctrl-alt-c", copy_as_text, context, false, None, cx);
        cx.bind_keys([binding.unwrap().with_meta(crate::keymap::USER)]);
    });
    let note = json!({"x": 0, "y": 0, "text": "Ship it"});
    let sticky = json!({"op": "command", "command": "board.sticky", "params": note});
    run_op(&view, sticky, &mut cx);
    focus_canvas(&view, &mut cx);
    cx.simulate_keystrokes("ctrl-alt-c");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some("Ship it"));
    let toast = view.read_with(&cx, |view, _| view.toast.clone());
    assert_eq!(toast.as_deref(), Some("Copied as text"));
}

/// The keymap editor doesn't offer to bind the commands the canvas has no use for, which get
/// no key and stay off the shortcuts sheet: Dev Mode, the switch to prototyping, cursor chat,
/// F6's regions, the keyboard box selection and the prototype player's Present and Preview
/// (CANVAS-UI-6). A key given to `select.area` would bring back a box selection whose box the
/// canvas never draws, and one given to Present or Preview a player it never shows.
#[test]
fn the_keymap_editor_offers_no_command_the_canvas_has_no_use_for() {
    let offered = offered_to_bind();
    assert!(
        offered.iter().any(|id| id == "file.exportSvg"),
        "{offered:?}"
    );
    for id in [
        "view.devMode",
        "view.designPrototype",
        "collab.cursorChat",
        "view.nextRegion",
        "view.previousRegion",
        "select.area",
        "proto.present",
        "proto.preview",
    ] {
        assert!(!offered.iter().any(|o| o == id), "{id} is offered");
    }
}

/// The canvas chrome's keys are rows of the keymap editor, bound as the app binds them: the
/// panels' toggles, Hide UI, the Actions palette and the shortcuts sheet are `board::Command`
/// bindings in the canvas's context (the sheet's while text is edited too), and ⇧⌘↩, which
/// Maximize says, is the workspace's.
#[gpui_kit::test]
fn the_canvas_chromes_keys_are_rows_of_the_keymap(cx: &mut TestAppContext) {
    let tmp = Scratch::new("keymap-rows");
    cx.update(bind_app_keys);
    let rows = cx.update(|cx| {
        crate::keymap::init_at(tmp.0.join("keymap.json"), cx);
        crate::keymap::store(cx).read(cx).rows(false, cx)
    });
    let has = |keys: &str, action: &str, id: Option<&str>, context: &str| {
        let keys = crate::keymap::names::normalize_keystrokes(keys).unwrap();
        let context = KeyBindingContextPredicate::parse(context)
            .unwrap()
            .to_string();
        rows.iter().any(|row| {
            let args = row
                .args
                .as_deref()
                .and_then(|a| serde_json::from_str(a).ok());
            let same_id = match id {
                Some(id) => args.is_some_and(|args: Value| args["id"] == id),
                None => true,
            };
            row.keystrokes.as_ref() == keys
                && row.action.as_ref() == action
                && same_id
                && row.context.as_deref() == Some(context.as_str())
        })
    };
    for (keys, id) in [
        ("alt-1", "view.layers"),
        ("alt-8", "view.design"),
        ("cmd-|", "view.leftSidebar"),
        ("cmd-\\", "view.toggleUi"),
        ("cmd-k", "app.actions"),
        ("cmd-/", "app.actions"),
        ("ctrl-?", "help.shortcuts"),
    ] {
        let command = Some(id);
        assert!(
            has(keys, "board::Command", command, shortcuts::CANVAS_CONTEXT),
            "{keys} {id}"
        );
    }
    let sheet = Some("help.shortcuts");
    assert!(has(
        "ctrl-?",
        "board::Command",
        sheet,
        shortcuts::TEXT_CONTEXT
    ));
    let maximize = "workspace::ToggleMaximizePane";
    assert!(has("shift-cmd-enter", maximize, None, "Workspace"));
}
