//! Where the keyboard goes when what has it goes off screen, and back once it is shown again.

use std::cell::RefCell;
use std::rc::Rc;

use gpui_kit::{App, Context, FocusHandle, Subscription, WeakFocusHandle, Window};

/// Keeps GPUI's keyboard on screen. Whatever had it and is no longer drawn gives it to `home`
/// (the workspace): the browser's page after a click in it, a file editor, a terminal pane or a
/// sidebar panel's field, once their tab, the tab area, the dock or the panel is off screen
/// (⌃Tab, ⇧⌘J, ⌘J, ⌘1 to ⌘0, a page an agent puts in front, an agent's `app_activate_tab`).
/// Otherwise the keyboard stays on what isn't drawn: GPUI then sends the keys from the window's
/// root, gpui-kit's `Root`, past every workspace shortcut (⇧⌘J, ⌘W, ⌘T, ⌘1), and typing goes
/// nowhere, until a click. Zed's workspace does the same.
///
/// What it took is given back ([`Self::give_back`]) once what the workspace shows changes while
/// the workspace still has the keyboard: a terminal pane hidden and shown again (⌘J twice, the
/// title bar's button twice) or a file's text whose tab is left and come back to has it again,
/// and typing goes on. It is forgotten as soon as anything else has the keyboard. Given back
/// while still off screen (⌃Tab on to a third tab, ⌘J then ⌃Tab), it is parked again and the
/// keyboard comes back here once the frame is drawn.
pub(crate) struct KeyboardHome<S> {
    home: FocusHandle,
    /// What the keyboard was last taken from.
    parked: Rc<RefCell<Option<WeakFocusHandle>>>,
    /// What the workspace showed at its last render.
    shown: Option<S>,
    _lost: Subscription,
}

impl<S: PartialEq> KeyboardHome<S> {
    pub(crate) fn new<V: 'static>(
        home: FocusHandle,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let parked = Rc::new(RefCell::new(None));
        let _lost = cx.on_focus_lost(window, {
            let (home, parked) = (home.clone(), parked.clone());
            move |_, window, cx| {
                // Still the handle that is no longer drawn.
                *parked.borrow_mut() = window.focused(cx).map(|h| h.downgrade());
                window.focus(&home, cx);
            }
        });
        Self {
            home,
            parked,
            shown: None,
            _lost,
        }
    }

    /// From the workspace's render, once the canvases have taken or given their keyboard:
    /// `shown` is what the workspace shows now. `keep` says what may have the keyboard given
    /// back: not the browser's page, which gave AppKit's keyboard back when hidden, so that
    /// GPUI's alone would type nowhere, nor a keystroke recorder, which takes every key; they
    /// wait for a click, as before.
    pub(crate) fn give_back(
        &mut self,
        shown: S,
        keep: impl Fn(&FocusHandle, &App) -> bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let changed = self.shown.as_ref() != Some(&shown);
        self.shown = Some(shown);
        if !self.home.is_focused(window) {
            // Something else has the keyboard: the user went on from what was hidden.
            self.parked.borrow_mut().take();
            return;
        }
        if !changed {
            // Nothing changed on screen (a store's redraw, a chord's first key): it waits.
            return;
        }
        let back = self.parked.borrow_mut().take().and_then(|h| h.upgrade());
        let Some(back) = back.filter(|back| keep(back, cx)) else {
            return;
        };
        window.focus(&back, cx);
        // GPUI's focus-lost fallback runs only for a frame after one with a keyboard path, and
        // the frame that hid `back` had none: if `back` isn't drawn in this one either, nothing
        // would take the keyboard from it. Check once the frame is drawn (review I5).
        let (home, parked) = (self.home.clone(), self.parked.clone());
        let handle = window.window_handle();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| {
                if back.is_focused(window) && !home.contains_focused(window, cx) {
                    *parked.borrow_mut() = Some(back.downgrade());
                    window.focus(&home, cx);
                }
            });
        });
    }
}
