//! State of one terminal tab: its split layout, which pane has focus,
//! which pane is zoomed and per-pane settings. Pure data, no UI.

use super::layout::{Direction, Layout, PaneId, Rect, Removed, SplitDir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Panes never get narrower than this many columns.
pub const MIN_COLS: u16 = 20;
/// Panes never get shorter than this many rows.
pub const MIN_ROWS: u16 = 4;

/// Settings the user can change on one pane.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PaneMeta {
    /// Custom name shown in the pane header. `None` uses the default title.
    pub name: Option<String>,
    /// Forward right-clicks to the program instead of opening the menu.
    pub send_right_clicks: bool,
}

/// Layout, focus and zoom of one terminal tab.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    /// The name the user gave the tab, which the tab strip shows whichever
    /// pane has focus. `None` shows the focused pane's name or title.
    #[serde(default)]
    pub name: Option<String>,
    /// The split tree.
    pub layout: Layout,
    /// The pane that receives keyboard input.
    pub focused: PaneId,
    /// The pane shown alone, when zoomed.
    pub zoomed: Option<PaneId>,
    /// Settings per pane.
    pub panes: BTreeMap<PaneId, PaneMeta>,
}

impl TabState {
    /// A tab with a single pane.
    pub fn new(first: PaneId) -> Self {
        Self {
            name: None,
            layout: Layout::Pane(first),
            focused: first,
            zoomed: None,
            panes: BTreeMap::from([(first, PaneMeta::default())]),
        }
    }

    /// Splits the focused pane. Refused (returns false, nothing changes)
    /// when either half would be smaller than `min` (width, height).
    /// On success the new pane gets focus and zoom is cleared.
    pub fn split(&mut self, dir: SplitDir, new: PaneId, area: Rect, min: (f32, f32)) -> bool {
        if !self.split_pane(self.focused, dir, new, area, min) {
            return false;
        }
        self.focused = new;
        true
    }

    /// Splits `pane`, which needn't be the focused one, and leaves the
    /// focus where it is: an agent's split, made while the user may type
    /// in another pane. Refused as [`Self::split`] is. On success zoom is
    /// cleared, so that both halves show.
    pub fn split_pane(
        &mut self,
        pane: PaneId,
        dir: SplitDir,
        new: PaneId,
        area: Rect,
        min: (f32, f32),
    ) -> bool {
        let Some((_, rect)) = self
            .layout
            .rects(area)
            .into_iter()
            .find(|(id, _)| *id == pane)
        else {
            return false;
        };
        let too_small = match dir {
            SplitDir::Right => rect.w / 2. < min.0,
            SplitDir::Down => rect.h / 2. < min.1,
        };
        if too_small || !self.layout.split(pane, dir, new) {
            return false;
        }
        self.panes.insert(new, PaneMeta::default());
        self.zoomed = None;
        true
    }

    /// Closes a pane. Clears zoom if it was zoomed and moves focus to the
    /// sibling that took its space if it was focused.
    pub fn close(&mut self, pane: PaneId) -> Removed {
        let removed = self.layout.remove(pane);
        if removed == Removed::NotFound {
            return removed;
        }
        self.panes.remove(&pane);
        if self.zoomed == Some(pane) {
            self.zoomed = None;
        }
        if let Removed::Focus(next) = removed
            && self.focused == pane
        {
            self.focused = next;
        }
        removed
    }

    /// Focuses a pane. Leaves zoom when another pane was zoomed.
    pub fn focus(&mut self, pane: PaneId) {
        self.focused = pane;
        if self.zoomed.is_some_and(|z| z != pane) {
            self.zoomed = None;
        }
    }

    /// Moves focus to the neighbouring pane. False when there is none.
    pub fn focus_dir(&mut self, dir: Direction, area: Rect) -> bool {
        match self.layout.neighbor(self.focused, dir, area) {
            Some(next) => {
                self.focus(next);
                true
            }
            None => false,
        }
    }

    /// Zooms the focused pane, or unzooms. False (no change) with one pane.
    pub fn toggle_zoom(&mut self) -> bool {
        if !self.is_split() {
            return false;
        }
        self.zoomed = match self.zoomed {
            Some(_) => None,
            None => Some(self.focused),
        };
        true
    }

    /// Names a pane. A blank name clears it.
    pub fn rename(&mut self, pane: PaneId, name: &str) {
        if let Some(meta) = self.panes.get_mut(&pane) {
            meta.name = clean_name(name);
        }
    }

    /// Names the tab. A blank name clears it.
    pub fn rename_tab(&mut self, name: &str) {
        self.name = clean_name(name);
    }

    /// Flips whether right-clicks go to the program in this pane.
    pub fn toggle_right_clicks(&mut self, pane: PaneId) {
        if let Some(meta) = self.panes.get_mut(&pane) {
            meta.send_right_clicks = !meta.send_right_clicks;
        }
    }

    /// The panes to draw and where: only the zoomed one when zoomed.
    pub fn visible(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        match self.zoomed {
            Some(pane) => vec![(pane, area)],
            None => self.layout.rects(area),
        }
    }

    /// True when the tab has more than one pane.
    pub fn is_split(&self) -> bool {
        self.panes.len() > 1
    }
}

/// `name` without the spaces around it; `None` when nothing is left.
fn clean_name(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_string())
}
