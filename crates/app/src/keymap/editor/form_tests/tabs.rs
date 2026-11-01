//! The editor in a stand-in for the workspace's tabs: ⌘E opens keymap.json, and a keys field
//! hidden with its tab lets go of the keyboard.

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, KeyBinding, TestAppContext};

use crate::keymap::editor::KeymapEditor;
use crate::keymap::tests::Scratch;

gpui_kit::actions!(keymap_editor_test, [CloseTab]);

/// A stand-in for the workspace: the editor in a tab that can be switched away, ⌘W bound in
/// `Workspace`, and the keyboard taken back when nothing holds it, as the workspace does.
struct Tabs {
    focus: gpui_kit::FocusHandle,
    editor: Entity<KeymapEditor>,
    shown: bool,
    closed: usize,
    /// How many times keymap.json was asked for.
    opened: usize,
}

impl gpui_kit::Render for Tabs {
    fn render(
        &mut self,
        window: &mut gpui_kit::Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        use gpui_kit::{InteractiveElement as _, ParentElement as _, Styled as _};
        if window.focused(cx).is_none() {
            self.focus.focus(window, cx);
        }
        gpui_kit::div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &CloseTab, _, _| this.closed += 1))
            .on_action(
                cx.listener(|this, _: &crate::keymap::OpenKeymapFile, _, _| this.opened += 1),
            )
            .children(self.shown.then(|| self.editor.clone()))
    }
}

/// The keymap editor in [`Tabs`], in an active window, with the app's ⌘W.
fn open_in_tabs(
    cx: &mut TestAppContext,
    scratch: &Scratch,
) -> (AnyWindowHandle, Entity<Tabs>, Entity<KeymapEditor>) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.bind_keys([KeyBinding::new("cmd-w", CloseTab, Some("Workspace"))]);
        crate::keymap::init_at(scratch.file(), cx);
    });
    let (window, tabs) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            let editor = cx.new(|cx| KeymapEditor::new(window, cx));
            cx.new(|cx| Tabs {
                focus: cx.focus_handle(),
                editor,
                shown: true,
                closed: 0,
                opened: 0,
            })
        })
        .unwrap()
    });
    cx.update_window(window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    let editor = cx.update(|cx| tabs.read(cx).editor.clone());
    (window, tabs, editor)
}

/// ⌘E in the keymap editor opens keymap.json, through the editor's own action: the Sawit
/// menu's Open Keymap File has no key.
#[gpui_kit::test]
fn cmd_e_in_the_editor_opens_the_keymap_file(cx: &mut TestAppContext) {
    let scratch = Scratch::new("cmd-e");
    let (window, tabs, editor) = open_in_tabs(cx, &scratch);
    cx.update_window(window, |_, window, cx| {
        let focus = editor.read(cx).focus.clone();
        window.focus(&focus, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "cmd-e");
    cx.update(|cx| assert_eq!(tabs.read(cx).opened, 1));
}

/// A recorder whose tab is switched away while it has the keyboard lets go of it: the
/// window's keys (⌘W) work again instead of going into the hidden field.
#[gpui_kit::test]
fn a_hidden_recorder_lets_go_of_the_keyboard(cx: &mut TestAppContext) {
    let scratch = Scratch::new("hidden-recorder");
    let (window, tabs, editor) = open_in_tabs(cx, &scratch);
    cx.update_window(window, |_, window, cx| {
        editor.update(cx, |editor, cx| {
            editor.toggle_keystroke_search(&super::ToggleKeystrokeSearch, window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "cmd-k");
    let searched =
        |cx: &mut TestAppContext| cx.update(|cx| editor.read(cx).keys_filter.read(cx).keystrokes());
    assert_eq!(searched(cx), "cmd-k");
    // Another tab shows.
    cx.update(|cx| {
        tabs.update(cx, |tabs, cx| {
            tabs.shown = false;
            cx.notify();
        })
    });
    cx.run_until_parked();
    cx.simulate_keystrokes(window, "cmd-w");
    assert_eq!(searched(cx), "cmd-k", "the hidden field took ⌘W");
    cx.update(|cx| assert_eq!(tabs.read(cx).closed, 1));
}

/// The keystroke search field is a recorder ([`crate::keymap::is_recorder`]), so the
/// workspace never gives it the keyboard back when its tab shows again: it takes every key,
/// and a ⌃Tab round would stop there. Only a click arms it again.
#[gpui_kit::test]
fn the_keystroke_search_is_known_as_a_recorder(cx: &mut TestAppContext) {
    let scratch = Scratch::new("recorder-known");
    let (_, _, editor) = open_in_tabs(cx, &scratch);
    cx.update(|cx| {
        let editor = editor.read(cx);
        let field = editor.keys_filter.read(cx).focus_handle().clone();
        assert!(crate::keymap::is_recorder(&field, cx));
        assert!(!crate::keymap::is_recorder(&editor.focus, cx));
    });
}
