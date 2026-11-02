//! The pages a window opens, in one list that two menus show: the account menu at the title bar's
//! end, and the footer's Pages menu beside Copy Path.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::*;

use super::{OpenBrowser, OpenCanvas, OpenDevTools, OpenMcp, OpenNewProject, OpenSkills};
use crate::keymap::OpenKeymapEditor;

/// Adds the pages to `menu`, each with its icon and the action that opens it, so the shortcut
/// bound to that action shows beside it.
pub(crate) fn pages(menu: PopupMenu) -> PopupMenu {
    let pages: [(&'static str, Icon, Box<dyn Action>); 7] = [
        ("Canvas", crate::board::ICON.into(), Box::new(OpenCanvas)),
        (
            "Browser",
            crate::browser::ICON.into(),
            Box::new(OpenBrowser),
        ),
        ("Dev Tools", Lucide::Package.into(), Box::new(OpenDevTools)),
        ("MCP Servers", Lucide::Plug.into(), Box::new(OpenMcp)),
        ("Skills", Lucide::Sparkles.into(), Box::new(OpenSkills)),
        (
            "New Project",
            Lucide::FolderPlus.into(),
            Box::new(OpenNewProject),
        ),
        (
            "Keymap Editor",
            Lucide::Keyboard.into(),
            Box::new(OpenKeymapEditor),
        ),
    ];
    pages.into_iter().fold(menu, |menu, (label, icon, action)| {
        menu.item(PopupMenuItem::new(label).icon(icon).action(action))
    })
}

/// The footer's Pages menu. The footer is the window's foot, so the menu opens upwards, its right
/// edge on the button's.
pub(super) fn pages_button() -> impl IntoElement {
    Button::new("pages-menu")
        .ghost()
        .xsmall()
        .icon(Lucide::LayoutGrid)
        .label("Pages")
        .dropdown_caret(true)
        .tooltip("Canvas, Browser, Dev Tools, MCP Servers, Skills and More")
        .dropdown_menu_with_anchor(Anchor::BottomRight, |menu, _, _| pages(menu))
}
