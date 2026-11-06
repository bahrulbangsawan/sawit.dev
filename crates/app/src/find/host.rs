//! A view's side of the find bar: the bar while it is open, where the keyboard was in the view when
//! it opened, and the bar drawn above the view's content.

use gpui_kit::*;

use super::{FindBar, FindEvent, Supports};

/// The find bar a view has open, if any.
#[derive(Default)]
pub struct Finder {
    bar: Option<Entity<FindBar>>,
    /// Where the keyboard was when the bar opened: it goes back there when the bar closes.
    back: Option<FocusHandle>,
    _events: Option<Subscription>,
}

impl Finder {
    /// The open bar.
    pub fn bar(&self) -> Option<&Entity<FindBar>> {
        self.bar.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.bar.is_some()
    }

    /// ⌘F (⌥⌘F for `replace`): opens the bar, or takes the keyboard to the open one, its query
    /// `seed` when given (the view's selected text), as [`FindBar::open`] does. `on_event` is
    /// the view's answer to what the user asks the bar for. A view with no text opens it with
    /// `supports` `None`: the bar says search isn't available there.
    pub fn open<V: 'static>(
        &mut self,
        supports: Option<Supports>,
        seed: Option<&str>,
        replace: bool,
        on_event: impl Fn(&mut V, &FindEvent, &mut Window, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Entity<FindBar> {
        let bar = match &self.bar {
            Some(bar) => bar.clone(),
            None => {
                let bar = cx.new(|cx| match supports {
                    Some(supports) => FindBar::new(supports, window, cx),
                    None => FindBar::unavailable(window, cx),
                });
                self._events = Some(cx.subscribe_in(
                    &bar,
                    window,
                    move |view, _, event, window, cx| on_event(view, event, window, cx),
                ));
                self.bar = Some(bar.clone());
                bar
            }
        };
        if !bar.read(cx).has_focus(window, cx) {
            self.back = window.focused(cx);
        }
        bar.update(cx, |bar, cx| bar.open(seed, replace, window, cx));
        cx.notify();
        bar
    }

    /// Closes the bar, the keyboard going back where it was in the view, or to `fallback` when
    /// that is gone. False when no bar was open.
    pub fn close(&mut self, fallback: &FocusHandle, window: &mut Window, cx: &mut App) -> bool {
        let Some(bar) = self.bar.take() else {
            return false;
        };
        self._events = None;
        let had_keyboard = bar.read(cx).has_focus(window, cx);
        if had_keyboard {
            let back = self.back.take().unwrap_or_else(|| fallback.clone());
            window.focus(&back, cx);
        }
        true
    }

    /// The bar, to draw above the view's content.
    pub fn render(&self) -> Option<AnyElement> {
        self.bar.clone().map(|bar| bar.into_any_element())
    }
}
