//! Pieces the catalog pages (Dev Tools, MCP) share: the toolbar (search,
//! filter tabs, menus), status pills, tags, logo tiles, the list's columns
//! and their header, its pager (with the rows a page holds, kept per list
//! across launches) and the empty state.

mod list;
mod pager;
mod paging;
mod toolbar;

use gpui_kit::*;

pub use list::{Mark, TILE, columns, empty, header, label, list_item, pill, tag, tile};
pub use pager::{page_size_menu, pager};
pub use paging::{PageSize, Paging, keep_page_sizes, page_of, saved_size};
pub use toolbar::{divider, menu, search, tabs};

actions!(
    catalog,
    [
        /// Moves the keyboard to the page's search field.
        FocusSearch,
        /// Selects the next row of the list.
        SelectNext,
        /// Selects the previous row of the list.
        SelectPrevious,
        /// Runs the selected row's main action: Install, Upgrade, Retry, or opens a server's
        /// sheet; in the MCP page's sheet, Install.
        Confirm
    ]
);

/// The catalog pages' keys. The list's plain keys act while the search field doesn't have
/// the keyboard (↩ there moves it to the first row), and the MCP page's while its sheet is
/// closed; in the sheet, ↩ installs. The New Project page's move through its cards, and ↩ picks
/// one.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-f", FocusSearch, Some("ToolsPage")),
        KeyBinding::new("down", SelectNext, Some("ToolsPage && !Input")),
        KeyBinding::new("up", SelectPrevious, Some("ToolsPage && !Input")),
        KeyBinding::new("enter", Confirm, Some("ToolsPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("McpPage && !McpSheet")),
        KeyBinding::new("down", SelectNext, Some("McpPage && !Input && !McpSheet")),
        KeyBinding::new("up", SelectPrevious, Some("McpPage && !Input && !McpSheet")),
        KeyBinding::new("enter", Confirm, Some("McpPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("SkillsPage && !SkillsSheet")),
        KeyBinding::new(
            "down",
            SelectNext,
            Some("SkillsPage && !Input && !SkillsSheet"),
        ),
        KeyBinding::new(
            "up",
            SelectPrevious,
            Some("SkillsPage && !Input && !SkillsSheet"),
        ),
        KeyBinding::new("enter", Confirm, Some("SkillsPage && !Input")),
        KeyBinding::new("cmd-f", FocusSearch, Some("NewProjectPage")),
        KeyBinding::new("down", SelectNext, Some("NewProjectPage && !Input")),
        KeyBinding::new("up", SelectPrevious, Some("NewProjectPage && !Input")),
        KeyBinding::new("enter", Confirm, Some("NewProjectPage && !Input")),
    ]);
}

/// The row a step of ↑ or ↓ selects in a list of `len` rows, from `current`.
pub fn step(current: Option<usize>, len: usize, down: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (current, down) {
        (None, _) => 0,
        (Some(ix), true) => (ix + 1).min(last),
        (Some(ix), false) => ix.saturating_sub(1).min(last),
    })
}

/// `~/…` instead of the home folder's path.
pub fn tilde(path: &std::path::Path) -> String {
    tilde_in(
        path,
        std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .as_deref(),
    )
}

fn tilde_in(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    sawit_core::recent::tildify(path, home)
}

#[cfg(test)]
mod tests;
