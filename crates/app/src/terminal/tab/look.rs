//! How a pane of a split tab looks for where the keyboard is: active, current or inactive.

use gpui_kit::component::ThemeColor;
use gpui_kit::*;
use sawit_core::terminal::PaneId;

/// How a pane of a split tab shows where the keyboard is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaneLook {
    /// The tab's focused pane, with the keyboard: what is typed goes here.
    Active,
    /// The tab's focused pane while the keyboard is elsewhere in the window
    /// (an editor, a sidebar panel): where typing goes once the terminal has
    /// the keyboard back.
    Current,
    /// Any other pane.
    Inactive,
}

impl PaneLook {
    /// The look of `pane` in a tab whose focused pane is `focused`, with the
    /// keyboard in the tab or not.
    pub fn of(pane: PaneId, focused: PaneId, keyboard_here: bool) -> Self {
        match (pane == focused, keyboard_here) {
            (true, true) => PaneLook::Active,
            (true, false) => PaneLook::Current,
            (false, _) => PaneLook::Inactive,
        }
    }
}

/// What a pane looks like for its [`PaneLook`]. The focused pane never stands
/// out by colour alone: its header's title is also heavier.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PaneStyle {
    pub border: Hsla,
    pub header_bg: Hsla,
    pub title: Hsla,
    pub title_weight: FontWeight,
    /// Laid over the terminal to dim it, as iTerm2 dims inactive split panes.
    pub dim: Option<Hsla>,
}

/// How much of the terminal's background an inactive pane's terminal is
/// dimmed with. Its text stays readable: the default palette's #abb2bf on
/// #282c33 goes from 6.6:1 to 4.8:1, still over the 4.5:1 text needs.
pub const INACTIVE_DIM: f32 = 0.2;

/// The colours and weight of a pane that looks `look`, in `theme`, over a
/// terminal whose background is `terminal_bg`. The active pane takes the
/// theme's accent for its border and the active tab's colours for its header,
/// as the tab strip's active tab does; the current pane keeps the header but
/// borders in the quieter focus ring; the others sit back in the tab bar's
/// colours, their text muted and their terminal dimmed.
pub fn pane_style(look: PaneLook, theme: &ThemeColor, terminal_bg: Hsla) -> PaneStyle {
    match look {
        PaneLook::Active => PaneStyle {
            border: theme.primary,
            header_bg: theme.tab_active,
            title: theme.tab_active_foreground,
            title_weight: FontWeight::SEMIBOLD,
            dim: None,
        },
        PaneLook::Current => PaneStyle {
            border: theme.ring,
            header_bg: theme.tab_active,
            title: theme.tab_active_foreground,
            title_weight: FontWeight::SEMIBOLD,
            dim: None,
        },
        PaneLook::Inactive => PaneStyle {
            border: theme.border,
            header_bg: theme.tab_bar,
            title: theme.muted_foreground,
            title_weight: FontWeight::NORMAL,
            dim: Some(terminal_bg.opacity(INACTIVE_DIM)),
        },
    }
}
