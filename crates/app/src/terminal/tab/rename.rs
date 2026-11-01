//! Renaming a pane in its header, or the tab in the tab strip.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::*;
use sawit_core::terminal::PaneId;

use super::{Rename, Renamed, TabEvent, TerminalTab};

/// Width of the field that renames a pane or the tab.
const RENAME_W: Pixels = px(160.);

impl TerminalTab {
    /// Puts a field for `pane`'s name in its header. A tab of one pane has
    /// no header, and its pane is all there is to see of it, so the tab is
    /// renamed instead: the name then stays on the tab when it is split.
    pub(super) fn rename_pane(
        &mut self,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.is_split() {
            self.start_rename(Renamed::Pane(pane), window, cx);
        } else {
            self.start_rename(Renamed::Tab, window, cx);
        }
    }

    /// Puts a field for the tab's name in the tab strip.
    pub fn rename_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_rename(Renamed::Tab, window, cx);
    }

    /// Puts a field for a pane's name in its header, or for the tab's in the
    /// tab strip, filled with the name shown there and selected. Enter names
    /// the pane or the tab (an empty name clears it); Escape or leaving the
    /// field keeps the old name.
    fn start_rename(&mut self, what: Renamed, window: &mut Window, cx: &mut Context<Self>) {
        let shown = match what {
            Renamed::Pane(pane) => match self.views.get(&pane) {
                Some(view) => self.pane_label(pane, view.read(cx)),
                None => return,
            },
            Renamed::Tab => self.label(cx),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(shown));
        input.update(cx, |s, cx| {
            s.focus(window, cx);
            s.select_all(window, cx);
        });
        let events = cx.subscribe_in(&input, window, move |this, input, e, window, cx| match e {
            InputEvent::PressEnter { .. } => {
                let name = input.read(cx).value();
                match what {
                    Renamed::Pane(pane) => this.state.rename(pane, &name),
                    Renamed::Tab => this.state.rename_tab(&name),
                }
                this.end_rename(window, cx);
            }
            InputEvent::Blur => {
                this.renaming = None;
                cx.emit(TabEvent::Changed);
                cx.notify();
            }
            InputEvent::Change | InputEvent::Focus => {}
        });
        self.renaming = Some(Rename {
            what,
            input,
            _events: events,
        });
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// Closes the rename field and gives the keyboard back to the pane.
    fn end_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        self.focus(window, cx);
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// The field renaming `pane`, while it is open.
    pub fn rename_field(&self, tab: WeakEntity<Self>, pane: PaneId) -> Option<AnyElement> {
        self.field(tab, Renamed::Pane(pane))
    }

    /// The field renaming the tab, while it is open.
    pub fn tab_rename_field(&self, tab: WeakEntity<Self>) -> Option<AnyElement> {
        self.field(tab, Renamed::Tab)
    }

    /// The field renaming `what`, while it is open.
    fn field(&self, tab: WeakEntity<Self>, what: Renamed) -> Option<AnyElement> {
        let rename = self.renaming.as_ref().filter(|r| r.what == what)?;
        Some(
            div()
                .w(RENAME_W)
                .flex_none()
                .on_action(move |_: &Escape, window, cx| {
                    let _ = tab.update(cx, |t, cx| t.end_rename(window, cx));
                })
                // A click in the field must not reach the pane or tab under
                // it, which would take the keyboard and close the field.
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(Input::new(&rename.input).xsmall().aria_label(match what {
                    Renamed::Tab => "Tab Name",
                    Renamed::Pane(_) => "Pane Name",
                }))
                .into_any_element(),
        )
    }
}
