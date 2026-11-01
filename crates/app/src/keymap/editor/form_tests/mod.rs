//! What the form tests share: the editor in an active window over defaults of their own (⌘J,
//! ⌘K W and the canvas's R), and the form's Save clicked.

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Entity, KeyBinding, KeyBindingContextPredicate,
    TestAppContext,
};
use sawit_board::shortcuts::CANVAS_CONTEXT;

use super::super::tests::Scratch;
use super::KeymapEditor;
// What the tests below name as `super::CreateBinding`, `super::Source::Unbound`, ….
use super::{CreateBinding, ToggleKeystrokeSearch};
use crate::keymap::Source;

mod clashes;
mod edits;
mod tabs;

gpui_kit::actions!(keymap_editor_test, [ToggleDock, CloseAll, Other]);

/// A keymap editor in an active window, over the defaults ⌘J (toggle the dock) and ⌘K W
/// (close all) in `Workspace`, and the canvas's R (the rectangle tool), with its keymap file
/// in `scratch`.
fn open(cx: &mut TestAppContext, scratch: &Scratch) -> (AnyWindowHandle, Entity<KeymapEditor>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([
            KeyBinding::new("cmd-j", ToggleDock, Some("Workspace")),
            KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
        ]);
        cx.bind_keys([canvas_key("r", "tool.rectangle", cx)]);
        crate::keymap::init_at(scratch.file(), cx);
    });
    let (window, editor) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| KeymapEditor::new(window, cx))
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    (window, editor)
}

/// The canvas's binding of `keys` to its command `id`, as `board::bindings` makes it.
fn canvas_key(keys: &str, id: &str, cx: &App) -> KeyBinding {
    let context = KeyBindingContextPredicate::parse(CANVAS_CONTEXT).unwrap();
    KeyBinding::load(
        keys,
        Box::new(crate::board::Command { id: id.into() }),
        Some(std::rc::Rc::new(context)),
        false,
        Some(serde_json::json!({ "id": id }).to_string().into()),
        cx,
    )
    .unwrap()
}

/// Clicks the open form's Save.
fn save(cx: &mut TestAppContext, window: AnyWindowHandle, editor: &Entity<KeymapEditor>) {
    cx.update_window(window, |_, window, cx| {
        let form = editor.read(cx).form.clone().expect("the form is open");
        form.update(cx, |form, cx| form.save(window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}
