//! The file tree's context menu.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::*;

use super::super::{
    AddToGitExclude, AddToGitignore, CollapseAll, CopyEntry, CopyPath, CopyRelativePath, Cut,
    Delete, Duplicate, ExpandAll, FindInFolder, NewFile, NewFolder, OpenInDefaultApp,
    OpenInTerminal, Paste, Redo, Rename, RevealInFinder, Trash, Undo, ViewHistory,
};
use super::MenuState;

pub(super) fn file_menu(menu: PopupMenu, focus: FocusHandle, can: MenuState) -> PopupMenu {
    use crate::menu_icons as icon;
    let item = |label: &'static str, glyph: Lucide, action: Box<dyn Action>| {
        PopupMenuItem::new(label).icon(glyph).action(action)
    };
    menu.action_context(focus)
        .item(item("New File", icon::NEW_FILE, Box::new(NewFile)))
        .item(item("New Folder", icon::NEW_FOLDER, Box::new(NewFolder)))
        .separator()
        .item(item(
            "Reveal in Finder",
            icon::REVEAL,
            Box::new(RevealInFinder),
        ))
        .item(item(
            "Open in Default App",
            icon::OPEN_EXTERNAL,
            Box::new(OpenInDefaultApp),
        ))
        .item(item(
            "Open in Terminal",
            icon::OPEN_TERMINAL,
            Box::new(OpenInTerminal),
        ))
        .separator()
        .item(item(
            "Find in Folder…",
            Lucide::Search,
            Box::new(FindInFolder),
        ))
        .separator()
        .item(item("Cut", icon::CUT, Box::new(Cut)))
        .item(item("Copy", icon::COPY, Box::new(CopyEntry)))
        .item(item("Duplicate", icon::DUPLICATE, Box::new(Duplicate)))
        .item(item("Paste", icon::PASTE, Box::new(Paste)).disabled(!can.paste))
        .item(item("Undo", icon::UNDO, Box::new(Undo)).disabled(!can.undo))
        .item(item("Redo", icon::REDO, Box::new(Redo)).disabled(!can.redo))
        .separator()
        .item(item("Copy Path", icon::LINK, Box::new(CopyPath)))
        .item(item(
            "Copy Relative Path",
            icon::LINK,
            Box::new(CopyRelativePath),
        ))
        .separator()
        .item(
            item(
                "Add to .gitignore",
                Lucide::EyeOff,
                Box::new(AddToGitignore),
            )
            .disabled(!can.git),
        )
        .item(
            item(
                "Add to .git/info/exclude",
                Lucide::EyeOff,
                Box::new(AddToGitExclude),
            )
            .disabled(!can.git),
        )
        .item(item("View History", icon::HISTORY, Box::new(ViewHistory)).disabled(!can.git))
        .separator()
        .item(item(
            "Expand All",
            Lucide::UnfoldVertical,
            Box::new(ExpandAll),
        ))
        .item(item(
            "Collapse All",
            Lucide::FoldVertical,
            Box::new(CollapseAll),
        ))
        .separator()
        .item(item("Rename", icon::RENAME, Box::new(Rename)))
        .item(item("Move to Trash", icon::DELETE, Box::new(Trash)).danger(true))
        .item(item("Delete", Lucide::Delete, Box::new(Delete)).danger(true))
}
