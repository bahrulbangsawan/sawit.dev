//! One terminal tab: its panes, each a `TerminalView`, arranged by a
//! `TabState` split layout. A tab with one pane shows it full size.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::*;
use sawit_core::terminal::{PaneId, SplitDir, TabState, new_pane_id};

use super::view::{TerminalEvent, TerminalView};

/// What a tab tells the panel.
pub enum TabEvent {
    /// The user asked to split the focused pane.
    #[expect(dead_code, reason = "emitted once panes can be split")]
    Split(SplitDir),
    /// A pane's title or state changed, so the tab strip needs a redraw.
    Changed,
    /// The tab's last pane closed.
    #[expect(dead_code, reason = "emitted once single panes can be closed")]
    Closed,
}

/// One tab of the terminal panel and the panes in it.
pub struct TerminalTab {
    /// Layout, focus, zoom and per-pane settings.
    pub state: TabState,
    /// Every pane's terminal; always holds exactly the layout's panes.
    views: HashMap<PaneId, Entity<TerminalView>>,
    /// The project whose group the tab is in (none for SSH or no project).
    pub group: Option<PathBuf>,
    /// The tab's area from the last paint, for laying out panes.
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
}

impl EventEmitter<TabEvent> for TerminalTab {}

impl TerminalTab {
    /// A tab holding `view` as its only pane.
    pub fn new(view: Entity<TerminalView>, group: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let pane = new_pane_id();
        cx.subscribe(&view, |_, _, _: &TerminalEvent, cx| {
            cx.emit(TabEvent::Changed)
        })
        .detach();
        Self {
            state: TabState::new(pane),
            views: HashMap::from([(pane, view)]),
            group,
            bounds: Rc::new(Cell::new(None)),
        }
    }

    /// The terminal of the pane that has focus.
    pub fn focused_view(&self) -> &Entity<TerminalView> {
        // The focused pane is always one of the layout's panes.
        &self.views[&self.state.focused]
    }

    /// Every pane and its terminal, in layout order.
    pub fn views(&self) -> impl Iterator<Item = (PaneId, &Entity<TerminalView>)> {
        self.state
            .layout
            .panes()
            .into_iter()
            .filter_map(|pane| self.views.get(&pane).map(|view| (pane, view)))
    }

    /// The focused pane's name, else its program's title.
    pub fn title(&self, cx: &App) -> SharedString {
        match self
            .state
            .panes
            .get(&self.state.focused)
            .and_then(|meta| meta.name.clone())
        {
            Some(name) => name.into(),
            None => self.focused_view().read(cx).title.clone(),
        }
    }

    /// Gives the keyboard to the focused pane.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        let handle = self.focused_view().focus_handle(cx);
        window.focus(&handle, cx);
    }

    /// One of the tab's panes has the keyboard.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        self.views()
            .any(|(_, view)| view.focus_handle(cx).contains_focused(window, cx))
    }
}

impl Render for TerminalTab {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let bounds = self.bounds.clone();
        div()
            .relative()
            .size_full()
            .child(
                canvas(move |area, _, _| bounds.set(Some(area)), |_, _, _, _| {})
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
            )
            .child(self.focused_view().clone())
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;

    use super::super::testing::{init, open_panel};

    #[gpui_kit::test]
    fn a_new_panel_has_one_tab_of_one_pane(cx: &mut TestAppContext) {
        cx.update(init);
        let (_w, panel) = open_panel(cx);
        cx.update(|cx| {
            let p = panel.read(cx);
            assert_eq!(p.tab_count(), 1);
            let tab = p.tab(0).read(cx);
            assert_eq!(tab.views().count(), 1);
            assert!(!tab.state.is_split());
        });
    }
}
