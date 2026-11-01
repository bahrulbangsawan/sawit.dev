//! Splitting, focusing, zooming and closing panes, dragging the dividers between them, and
//! the smallest a pane may get.

use gpui_kit::*;
use sawit_core::terminal::{
    Direction, MIN_COLS, MIN_ROWS, PaneId, Rect, Removed, SplitDir, new_pane_id,
};

use super::{HEADER_H, PANE_BORDER, Renamed, TabEvent, TerminalTab, Unsplit};
use crate::terminal::view::{PAD_X, TerminalView, cell_size};

impl TerminalTab {
    /// Splits the focused pane and puts `view` in the new half, focused.
    /// Refused (false) before the first paint or when either half would be
    /// under 20 columns or 4 rows; `view` is then dropped, ending its process.
    pub fn add_pane(
        &mut self,
        dir: SplitDir,
        view: Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let focused = self.state.focused;
        let Ok(pane) = self.split_pane(focused, dir, view, None, window, cx) else {
            return false;
        };
        self.state.focus(pane);
        self.focus(window, cx);
        true
    }

    /// Whether `pane` could be split `dir`: [`Self::split_pane`] without the split, so that a
    /// shell is started only for a split that will happen.
    pub fn can_split(
        &self,
        pane: PaneId,
        dir: SplitDir,
        area: Option<Rect>,
        window: &Window,
        cx: &App,
    ) -> Result<(), Unsplit> {
        let area = self.area().or(area).ok_or(Unsplit::Unmeasured)?;
        let mut state = self.state.clone();
        match state.split_pane(
            pane,
            dir,
            new_pane_id(),
            area,
            min_size(cell_size(window, cx)),
        ) {
            true => Ok(()),
            false => Err(Unsplit::TooSmall),
        }
    }

    /// Splits `pane` and puts `view` in the new half, leaving the tab's focus and the keyboard
    /// where they are: an agent's split, made while the user may type in another pane
    /// ([`TabState::split_pane`]). A tab not drawn yet is measured as `area`, the area the dock
    /// gives every tab. Returns the new pane, or why there is none; `view` is then dropped,
    /// ending its process.
    ///
    /// [`TabState::split_pane`]: sawit_core::terminal::TabState::split_pane
    pub fn split_pane(
        &mut self,
        pane: PaneId,
        dir: SplitDir,
        view: Entity<TerminalView>,
        area: Option<Rect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<PaneId, Unsplit> {
        let area = self.area().or(area).ok_or(Unsplit::Unmeasured)?;
        let new = new_pane_id();
        let min = min_size(cell_size(window, cx));
        if !self.state.split_pane(pane, dir, new, area, min) {
            return Err(Unsplit::TooSmall);
        }
        Self::watch(new, &view, cx);
        self.views.insert(new, view);
        cx.emit(TabEvent::Changed);
        cx.notify();
        Ok(new)
    }

    /// Focuses `pane` and gives it the keyboard.
    pub(super) fn focus_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.views.contains_key(&pane) {
            return;
        }
        let changed = self.state.focused != pane;
        self.state.focus(pane);
        self.focus(window, cx);
        if changed {
            cx.emit(TabEvent::Changed);
        }
        cx.notify();
    }

    /// Focuses the neighbouring pane. Without one the key goes on to the
    /// next handler.
    pub(super) fn move_focus(
        &mut self,
        dir: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = self
            .area()
            .and_then(|area| self.state.layout.neighbor(self.state.focused, dir, area));
        match next {
            Some(pane) => self.focus_pane(pane, window, cx),
            None => cx.propagate(),
        }
    }

    /// Focuses the next pane in reading order (`forward`), or the previous
    /// one, wrapping round. With one pane the key goes on to the next handler.
    pub(super) fn step_focus(
        &mut self,
        forward: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.state.next_pane(self.area(), forward) {
            Some(pane) => self.focus_pane(pane, window, cx),
            None => cx.propagate(),
        }
    }

    /// ⇧⇥: the next pane, when nothing runs in front in the focused one (its
    /// shell waits at the prompt) and the tab is split. Otherwise the key goes
    /// on, to the terminal's own key handling and the program.
    pub(super) fn step_focus_at_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let idle = self.focused_view().read(cx).nothing_in_front();
        if idle && self.state.is_split() {
            self.step_focus(true, window, cx);
        } else {
            cx.propagate();
        }
    }

    /// Zooms `pane`, or unzooms it.
    pub(super) fn toggle_zoom(
        &mut self,
        pane: PaneId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_pane(pane, window, cx);
        self.state.toggle_zoom();
        cx.notify();
    }

    /// Closes `pane`, ending its process. The last pane closes the tab: its
    /// terminal goes with the tab, which the panel drops on `Closed`.
    pub fn close_pane(&mut self, pane: PaneId, window: &mut Window, cx: &mut Context<Self>) {
        self.remove_pane(pane, true, window, cx);
    }

    /// Closes `pane` like `close_pane`. Only when `refocus` is set does the
    /// keyboard go to the tab's next focused pane (or, for the last pane,
    /// to the panel's next tab).
    pub(in crate::terminal) fn remove_pane(
        &mut self,
        pane: PaneId,
        refocus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.state.close(pane) {
            Removed::NotFound => {}
            Removed::Last => {
                self.menu = None;
                self.renaming = None;
                cx.emit(TabEvent::Closed { refocus });
            }
            Removed::Focus(_) => {
                self.views.remove(&pane);
                if self
                    .renaming
                    .as_ref()
                    .is_some_and(|r| r.what == Renamed::Pane(pane))
                {
                    self.renaming = None;
                }
                if refocus {
                    self.focus(window, cx);
                }
                cx.emit(TabEvent::Changed);
                cx.notify();
            }
        }
    }

    /// Moves the dragged divider to `position` (window coordinates).
    pub(super) fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let (Some(drag), Some(bounds)) = (&self.drag, self.bounds.get()) else {
            return;
        };
        let (at, start, len) = match drag.dir {
            SplitDir::Right => (
                f32::from(position.x - bounds.origin.x),
                drag.span.x,
                drag.span.w,
            ),
            SplitDir::Down => (
                f32::from(position.y - bounds.origin.y),
                drag.span.y,
                drag.span.h,
            ),
        };
        let ratio = (at - start) / len;
        self.state
            .layout
            .set_ratio(&drag.path, ratio, len, drag.pane_min);
        cx.notify();
    }
}

/// Slack for layout rounding, so a pane at its minimum never loses a
/// column or row to a fraction of a pixel.
const ROUNDING: f32 = 1.;

/// The smallest pane rect, in pixels, whose grid still gets 20 columns by
/// 4 rows: the grid plus the pane's border on both sides, the view's padding
/// left and right, and the header (which holds its own border) on top.
pub(super) fn min_size(cell: Size<Pixels>) -> (f32, f32) {
    let border = 2. * f32::from(PANE_BORDER);
    (
        f32::from(cell.width) * f32::from(MIN_COLS) + 2. * f32::from(PAD_X) + border + ROUNDING,
        f32::from(cell.height) * f32::from(MIN_ROWS) + f32::from(HEADER_H) + border + ROUNDING,
    )
}
