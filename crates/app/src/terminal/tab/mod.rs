//! One terminal tab: its panes, each a `TerminalView`, arranged by a
//! `TabState` split layout. A tab with one pane shows it full size; with
//! more, each pane gets a header and a border, and the dividers between
//! them can be dragged. Right-clicking a pane opens its menu (rename,
//! split, zoom, right-click routing, close), each item with its key. A tab
//! can have a name of its own, which the tab strip shows whichever pane has
//! focus. When a pane's process ends, a
//! plain shell in a split closes its pane; any other pane says so in a
//! banner, with what can be done next.

mod header;
mod look;
mod menu;
mod panes;
mod rename;
mod render;

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::InputState;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::*;
use sawit_core::terminal::{AgentKind, PaneId, Rect, SplitDir, SplitPath, TabState, new_pane_id};

use super::view::{TerminalEvent, TerminalView};

/// Height of a pane's header, its bottom border included (boxes are
/// border-box, so the border takes from this height).
pub const HEADER_H: Pixels = px(24.);
/// The border around each pane of a split tab.
const PANE_BORDER: Pixels = px(1.);

/// Why a pane wasn't split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsplit {
    /// The tab hasn't been drawn, and no area was given for it: how big its panes are isn't known.
    Unmeasured,
    /// A half would be under 20 columns or 4 rows.
    TooSmall,
}

/// What a tab tells the panel.
pub enum TabEvent {
    /// A pane's title or state changed, so the tab strip needs a redraw.
    Changed,
    /// The tab's last pane closed. `refocus` is set when the keyboard was
    /// in the tab (or the user closed the pane there), so the panel's next
    /// tab should take it.
    Closed { refocus: bool },
    /// The user asked to connect the ended SSH session in this pane again.
    Reconnect(PaneId),
    /// The user asked for a new shell tab.
    NewTerminal,
}

/// A divider being dragged.
struct Drag {
    /// The split it belongs to.
    path: SplitPath,
    dir: SplitDir,
    /// The split's whole area, in the tab's coordinates.
    span: Rect,
    /// The smallest (width, height) any pane may get.
    pane_min: (f32, f32),
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
    /// The divider under the mouse while it is dragged.
    drag: Option<Drag>,
    /// The pane menu while it is open.
    menu: Option<Menu>,
    /// The pane or the tab being renamed, if any.
    renaming: Option<Rename>,
    /// The window the tab last drew in, where its pane menu opens.
    window: Option<AnyWindowHandle>,
    /// How many times the tab has drawn (for tests).
    #[cfg(test)]
    renders: usize,
}

/// An open pane menu.
struct Menu {
    view: Entity<PopupMenu>,
    /// Where it opens, in window coordinates.
    at: Point<Pixels>,
    _dismiss: Subscription,
}

/// What a rename field names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Renamed {
    /// A pane, in its header.
    Pane(PaneId),
    /// The tab, in the tab strip.
    Tab,
}

/// A pane or the tab being renamed.
struct Rename {
    what: Renamed,
    input: Entity<InputState>,
    _events: Subscription,
}

impl EventEmitter<TabEvent> for TerminalTab {}

impl TerminalTab {
    /// A tab holding `view` as its only pane.
    pub fn new(view: Entity<TerminalView>, group: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let pane = new_pane_id();
        Self::watch(pane, &view, cx);
        Self {
            state: TabState::new(pane),
            views: HashMap::from([(pane, view)]),
            group,
            bounds: Rc::new(Cell::new(None)),
            drag: None,
            menu: None,
            renaming: None,
            window: None,
            #[cfg(test)]
            renders: 0,
        }
    }

    /// Opens the pane menu when asked, handles the pane's process ending,
    /// and redraws the tab strip when the pane's title or agent changes (and
    /// the pane's header for its agent). Events from a terminal the pane no
    /// longer holds are ignored.
    fn watch(pane: PaneId, view: &Entity<TerminalView>, cx: &mut Context<Self>) {
        cx.subscribe(view, move |this, view, event: &TerminalEvent, cx| {
            if this.views.get(&pane) != Some(&view) {
                return;
            }
            match event {
                TerminalEvent::MenuRequested(at) => this.request_menu(pane, *at, cx),
                TerminalEvent::TitleChanged => cx.emit(TabEvent::Changed),
                TerminalEvent::AgentChanged => {
                    cx.emit(TabEvent::Changed);
                    cx.notify();
                }
                TerminalEvent::Exited => this.pane_exited(pane, cx),
                TerminalEvent::Focused => this.pane_focused(pane, cx),
            }
        })
        .detach();
    }

    /// `pane`'s process ended. A plain shell (not SSH, not a script) in a
    /// split closes its pane; any other pane stays and shows its banner.
    fn pane_exited(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        let plain_shell = self.views.get(&pane).is_some_and(|view| {
            let v = view.read(cx);
            v.shell && v.remote.is_none() && v.job.is_none()
        });
        if plain_shell && self.state.is_split() {
            self.in_window(cx, move |this, window, cx| {
                if this.state.is_split() {
                    // Nobody asked for this close, so the keyboard stays
                    // where it is unless it was in this tab.
                    let refocus = this.has_focus(window, cx);
                    this.remove_pane(pane, refocus, window, cx);
                }
            });
        }
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// `pane` took the keyboard by itself: the tab's focus (its ring, where
    /// a split starts, what zoom and the agents' default pane mean) follows.
    fn pane_focused(&mut self, pane: PaneId, cx: &mut Context<Self>) {
        if self.state.focused != pane {
            self.state.focus(pane);
            cx.emit(TabEvent::Changed);
        }
        cx.notify();
    }

    /// Runs `f` on the tab with the window it last drew in, once that can
    /// be reached (events from a pane come without it).
    fn in_window(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) {
        let Some(handle) = self.window else {
            return;
        };
        let tab = cx.entity().downgrade();
        cx.defer(move |cx| {
            let _ = handle.update(cx, |_, window, cx| tab.update(cx, |t, cx| f(t, window, cx)));
        });
    }

    /// The terminal in `pane`, if the tab has that pane.
    pub fn view(&self, pane: PaneId) -> Option<&Entity<TerminalView>> {
        self.views.get(&pane)
    }

    /// Puts `view` in `pane` in place of its terminal (whose process is
    /// dropped) and focuses the pane. Does nothing for an unknown pane.
    pub fn replace_view(
        &mut self,
        pane: PaneId,
        view: Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) {
        if !self.views.contains_key(&pane) {
            return;
        }
        let send = self
            .state
            .panes
            .get(&pane)
            .is_some_and(|meta| meta.send_right_clicks);
        view.update(cx, |v, _| v.send_right_clicks = send);
        Self::watch(pane, &view, cx);
        self.views.insert(pane, view);
        self.state.focus(pane);
        cx.emit(TabEvent::Changed);
        cx.notify();
    }

    /// The terminal of the pane that has focus.
    pub fn focused_view(&self) -> &Entity<TerminalView> {
        // The focused pane is always one of the layout's panes.
        &self.views[&self.state.focused]
    }

    /// The coding agent running in each pane that runs one, in reading order (left to right,
    /// then down), as the tab strip shows them.
    pub fn agents(&self, cx: &App) -> Vec<(PaneId, AgentKind)> {
        self.state
            .reading_order(self.area())
            .into_iter()
            .filter_map(|pane| {
                let agent = self.views.get(&pane)?.read(cx).agent()?;
                Some((pane, agent))
            })
            .collect()
    }

    /// Every pane and its terminal, in layout order.
    pub fn views(&self) -> impl Iterator<Item = (PaneId, &Entity<TerminalView>)> {
        self.state
            .layout
            .panes()
            .into_iter()
            .filter_map(|pane| self.views.get(&pane).map(|view| (pane, view)))
    }

    /// A pane's name, else its program's title.
    pub(super) fn pane_title(&self, pane: PaneId, view: &TerminalView) -> SharedString {
        match self
            .state
            .panes
            .get(&pane)
            .and_then(|meta| meta.name.clone())
        {
            Some(name) => name.into(),
            None => view.title.clone(),
        }
    }

    /// What a pane's header calls it: the name the user gave it, else, as
    /// in the tab strip, the server of an SSH session or the script it
    /// runs, else its program's title.
    pub(super) fn pane_label(&self, pane: PaneId, view: &TerminalView) -> SharedString {
        let named = self
            .state
            .panes
            .get(&pane)
            .and_then(|meta| meta.name.clone());
        let fixed = view
            .remote
            .as_ref()
            .map(|r| r.name.clone())
            .or_else(|| view.job.as_ref().map(|j| j.name.clone()));
        match named.or(fixed) {
            Some(name) => name.into(),
            None => view.title.clone(),
        }
    }

    /// The name the user gave the focused pane, if any.
    pub fn pane_name(&self) -> Option<&str> {
        self.state
            .panes
            .get(&self.state.focused)
            .and_then(|meta| meta.name.as_deref())
    }

    /// The tab's name, else the focused pane's name, else its program's
    /// title.
    pub fn title(&self, cx: &App) -> SharedString {
        match &self.state.name {
            Some(name) => name.clone().into(),
            None => self.pane_title(self.state.focused, self.focused_view().read(cx)),
        }
    }

    /// What the tab strip calls the tab, before the state it adds (`(exited)`):
    /// the tab's name, else what its focused pane's header would.
    pub fn label(&self, cx: &App) -> SharedString {
        match &self.state.name {
            Some(name) => name.clone().into(),
            None => self.pane_label(self.state.focused, self.focused_view().read(cx)),
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

    /// The tab's area from the last paint, in its own coordinates.
    pub(super) fn area(&self) -> Option<Rect> {
        self.bounds.get().map(|b| Rect {
            x: 0.,
            y: 0.,
            w: f32::from(b.size.width),
            h: f32::from(b.size.height),
        })
    }
}

#[cfg(test)]
mod tests;
