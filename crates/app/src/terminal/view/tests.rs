//! The terminal's key bindings, and where its right-clicks go.

use gpui_kit::{
    App, AppContext as _, Context, FocusHandle, IntoElement, KeyContext, KeyDownEvent, Keystroke,
    ParentElement as _, Render, Styled as _, TestAppContext, Window, div, prelude::*,
};

/// Keys a shell or TUI (Claude Code, vim, fzf, readline) needs.
const SHELL_KEYS: &[&str] = &[
    "tab",
    "shift-tab",
    "enter",
    "shift-enter",
    "alt-enter",
    "escape",
    "backspace",
    "alt-backspace",
    "delete",
    "space",
    "ctrl-space",
    "up",
    "down",
    "left",
    "right",
    "shift-up",
    "alt-left",
    "alt-right",
    "ctrl-left",
    "ctrl-right",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f1",
    "f5",
    "f12",
    "a",
    "shift-a",
    "/",
    "alt-f",
    "alt-b",
    "alt-d",
    "alt-.",
    "ctrl-a",
    "ctrl-b",
    "ctrl-c",
    "ctrl-d",
    "ctrl-e",
    "ctrl-f",
    "ctrl-g",
    "ctrl-h",
    "ctrl-j",
    "ctrl-k",
    "ctrl-l",
    "ctrl-n",
    "ctrl-o",
    "ctrl-p",
    "ctrl-r",
    "ctrl-s",
    "ctrl-t",
    "ctrl-u",
    "ctrl-v",
    "ctrl-w",
    "ctrl-x",
    "ctrl-y",
    "ctrl-z",
    "ctrl-\\",
    "ctrl-]",
    "ctrl-[",
    "ctrl-_",
    "cmd-c",
];

/// Non-⌘ keys the app deliberately takes over a focused terminal.
const APP_KEYS: &[&str] = &["ctrl-tab", "ctrl-shift-tab"];

/// Shell keys the terminal takes only at a shell prompt, and only in a split tab: anywhere
/// else their action hands the key on to the program (see the tab's tests).
const PROMPT_KEYS: &[(&str, &str)] = &[("shift-tab", "terminal::FocusNextPaneAtPrompt")];

/// Whether `names`, the actions `key` runs in a terminal, are only a [`PROMPT_KEYS`] one.
fn only_at_a_prompt(key: &str, names: &[&str]) -> bool {
    PROMPT_KEYS
        .iter()
        .any(|(k, action)| *k == key && names == [*action])
}

fn init(cx: &mut App) {
    gpui_kit::init(cx);
    crate::workspace::init(cx);
    super::super::init(cx);
}

fn stack(contexts: &[&str]) -> Vec<KeyContext> {
    contexts
        .iter()
        .map(|c| KeyContext::parse(c).unwrap())
        .collect()
}

fn name(k: &Keystroke) -> String {
    let m = &k.modifiers;
    let mut s = String::new();
    for (on, p) in [
        (m.control, "ctrl-"),
        (m.alt, "alt-"),
        (m.shift, "shift-"),
        (m.platform, "cmd-"),
    ] {
        if on {
            s.push_str(p);
        }
    }
    s + &k.key
}

#[gpui_kit::test]
fn no_binding_takes_a_shell_key_from_the_terminal(cx: &mut TestAppContext) {
    cx.update(init);
    let terminal = stack(&["Root", "Workspace", "Terminal"]);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for key in SHELL_KEYS {
            let (bindings, pending) =
                keymap.bindings_for_input(&[Keystroke::parse(key).unwrap()], &terminal);
            let names: Vec<_> = bindings.iter().map(|b| b.action().name()).collect();
            assert!(
                (names.is_empty() || only_at_a_prompt(key, &names)) && !pending,
                "{key} is taken by {names:?}"
            );
        }
        // Whatever gets bound later: only ⌘ shortcuts and APP_KEYS may
        // beat the shell.
        for binding in keymap.bindings() {
            let keys: Vec<Keystroke> = binding
                .keystrokes()
                .iter()
                .map(|k| Keystroke {
                    modifiers: *k.modifiers(),
                    key: k.key().to_string(),
                    key_char: None,
                })
                .collect();
            let first = &keys[0];
            if first.modifiers.platform || APP_KEYS.contains(&name(first).as_str()) {
                continue;
            }
            let (bindings, pending) = keymap.bindings_for_input(&keys[..1], &terminal);
            let names: Vec<_> = bindings.iter().map(|b| b.action().name()).collect();
            assert!(
                (names.is_empty() || only_at_a_prompt(&name(first), &names)) && !pending,
                "{} is taken from the terminal by {names:?}",
                name(first)
            );
        }
    });
}

#[gpui_kit::test]
fn app_shortcuts_still_work_from_the_terminal(cx: &mut TestAppContext) {
    cx.update(init);
    let workspace = stack(&["Root", "Workspace"]);
    let terminal = stack(&["Root", "Workspace", "Terminal"]);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for binding in keymap.bindings() {
            let keys: Vec<Keystroke> = binding
                .keystrokes()
                .iter()
                .map(|k| Keystroke {
                    modifiers: *k.modifiers(),
                    key: k.key().to_string(),
                    key_char: None,
                })
                .collect();
            let reclaimed = super::bindings().iter().any(|b| {
                b.keystrokes().len() == 1
                    && b.keystrokes()[0].key() == keys[0].key
                    && *b.keystrokes()[0].modifiers() == keys[0].modifiers
            });
            if reclaimed || binding.predicate().is_none_or(|p| !p.eval(&workspace)) {
                continue;
            }
            let action = |stack| {
                keymap
                    .bindings_for_input(&keys, stack)
                    .0
                    .first()
                    .map(|b| b.action().name())
            };
            assert_eq!(
                action(&terminal),
                action(&workspace),
                "{:?} acts differently in the terminal",
                keys.iter().map(name).collect::<Vec<_>>()
            );
        }
    });
}

/// Stand-in for the workspace: a tab stop beside a focused element with
/// the terminal's key context and an `on_key_down` like `TerminalView`'s.
struct Host {
    terminal: FocusHandle,
    received: Vec<String>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("Workspace")
            .size_full()
            .child(
                div()
                    .id("button")
                    .track_focus(&cx.focus_handle().tab_stop(true))
                    .child("0/0 instances running"),
            )
            .child(
                div()
                    .id("terminal")
                    .key_context("Terminal")
                    .track_focus(&self.terminal)
                    .size_full()
                    .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                        this.received.push(name(&e.keystroke));
                        cx.stop_propagation();
                    })),
            )
    }
}

#[gpui_kit::test]
fn tab_stays_in_the_terminal(cx: &mut TestAppContext) {
    cx.update(init);
    let (window, host) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |_, cx| {
            cx.new(|cx| Host {
                terminal: cx.focus_handle(),
                received: Vec::new(),
            })
        })
        .unwrap()
    });
    let focus = cx.update(|cx| host.read(cx).terminal.clone());
    cx.update_window(window, |_, window, cx| window.focus(&focus, cx))
        .unwrap();
    cx.run_until_parked();

    let keys = "tab tab shift-tab cmd-c ctrl-r escape enter";
    cx.simulate_keystrokes(window, keys);

    let focused = cx
        .update_window(window, |_, window, _| focus.is_focused(window))
        .unwrap();
    assert!(focused, "focus left the terminal");
    let received = cx.update(|cx| host.read(cx).received.clone());
    assert_eq!(received, keys.split(' ').collect::<Vec<_>>());
}

#[test]
fn right_clicks_open_the_menu_unless_sent_to_the_pane() {
    use super::mouse::{RightClick::*, right_click_target};
    assert!(matches!(right_click_target(false, false, true), Menu));
    assert!(matches!(right_click_target(true, false, true), Program));
    assert!(matches!(right_click_target(true, false, false), Nothing));
    assert!(matches!(right_click_target(true, true, true), Menu));
}
