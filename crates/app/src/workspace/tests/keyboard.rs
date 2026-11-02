//! A stand-in for the workspace in a window as `cli::open_window` makes one (under gpui-kit's
//! `Root`): the workspace's keys, key context and keyboard, `render`'s refocus when nothing
//! has the keyboard, and its [`super::super::KeyboardHome`], given back to as `render` does. It shows
//! a terminal pane while the dock shows (⌘J), which records what is typed into it, and a
//! file's text while its tab is in front (⌃Tab goes round three tabs), drawn with a page,
//! which, as the browser's, never has the keyboard given back.

use super::super::{KeyboardHome, NextTab, ToggleTerminal};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Entity, FocusHandle, KeyDownEvent, TestAppContext, Window, div,
};

pub(super) struct StandIn {
    pub focus: FocusHandle,
    /// A terminal pane's keyboard, drawn while the dock shows (⌘J).
    pub pane: FocusHandle,
    pub dock: bool,
    /// A file editor's text, drawn while its tab is the one in front (⌃Tab).
    pub text: Entity<InputState>,
    /// A web page's keyboard, drawn with the text.
    pub page: FocusHandle,
    pub tab: usize,
    pub presses: usize,
    /// What reached the terminal pane.
    pub typed: Vec<String>,
    renders: usize,
    keyboard: KeyboardHome<(bool, usize)>,
}

impl Render for StandIn {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A give-back that asked for frames without end would hang the test instead.
        self.renders += 1;
        assert!(self.renders < 500, "the stand-in redraws without end");
        if window.focused(cx).is_none() {
            self.focus.focus(window, cx);
        }
        let page = self.page.clone();
        self.keyboard
            .give_back((self.dock, self.tab), |back, _| *back != page, window, cx);
        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .size_full()
            .on_action(cx.listener(|this, _: &ToggleTerminal, _, cx| {
                this.dock = !this.dock;
                this.presses += 1;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &NextTab, _, cx| {
                this.tab = (this.tab + 1) % 3;
                this.presses += 1;
                cx.notify();
            }))
            .when(self.tab == 0, |el| {
                el.child(Input::new(&self.text))
                    .child(div().track_focus(&self.page))
            })
            .when(self.dock, |el| {
                el.child(
                    div()
                        .key_context("Terminal")
                        .track_focus(&self.pane)
                        .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                            if !e.keystroke.modifiers.modified() {
                                this.typed.push(e.keystroke.key.clone());
                                cx.stop_propagation();
                            }
                        })),
                )
            })
    }
}

/// What has the keyboard to start with.
pub(super) type Keyboard = fn(&StandIn, &App) -> FocusHandle;

/// The stand-in, with the keyboard given to what `keyboard` picks.
pub(super) fn open(
    keyboard: Keyboard,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<StandIn>) {
    let (window, workspace) = cx.update(|cx| {
        gpui_kit::open_window(Default::default(), cx, |window, cx| {
            let text = cx.new(|cx| InputState::new(window, cx));
            cx.new(|cx| {
                let focus = cx.focus_handle();
                StandIn {
                    keyboard: KeyboardHome::new(focus.clone(), window, cx),
                    focus,
                    pane: cx.focus_handle(),
                    dock: true,
                    text,
                    page: cx.focus_handle(),
                    tab: 0,
                    presses: 0,
                    typed: Vec::new(),
                    renders: 0,
                }
            })
        })
        .unwrap()
    });
    let to = cx.update(|cx| keyboard(workspace.read(cx), cx));
    cx.update_window(window, |_, window, cx| {
        window.activate_window();
        window.focus(&to, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let placed = cx
        .update_window(window, |_, window, _| to.is_focused(window))
        .unwrap();
    assert!(placed, "the keyboard isn't where the test put it");
    (window, workspace)
}

/// Changes what the stand-in shows as a click does: with no key.
pub(super) fn show(
    workspace: &Entity<StandIn>,
    cx: &mut TestAppContext,
    change: impl FnOnce(&mut StandIn),
) {
    workspace.update(cx, |workspace, cx| {
        change(workspace);
        cx.notify();
    });
    cx.run_until_parked();
}

/// Whether `handle`, picked as `keyboard` picks, has the keyboard.
pub(super) fn has(
    keyboard: Keyboard,
    window: AnyWindowHandle,
    workspace: &Entity<StandIn>,
    cx: &mut TestAppContext,
) -> bool {
    cx.update_window(window, |_, window, cx| {
        keyboard(workspace.read(cx), cx).is_focused(window)
    })
    .unwrap()
}
