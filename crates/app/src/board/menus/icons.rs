//! The icons of the canvas's engine menus (B16): each command id to its icon, so the main menu
//! and the context menu show the pictures the toolbar and the app's other menus do (Copy is
//! [`menu_icons::COPY`] here as everywhere). A command without one of its own takes its menu's
//! (every `view.` command a view icon, and so on), so no item is left without a picture.

use gpui_kit::assets::IconName as Lucide;
use sawit_board::policy;

use crate::menu_icons as icon;

/// The icon of command `id` in the canvas's menus.
pub(in crate::board) fn command(id: &str) -> Lucide {
    if let Some(found) = exact(id) {
        return found;
    }
    let area = id.split('.').next().unwrap_or_default();
    match area {
        "file" => Lucide::File,
        "edit" => icon::EDIT,
        "select" => Lucide::MousePointer2,
        "view" => icon::SHOW,
        "object" => Lucide::Shapes,
        "layout" => Lucide::LayoutGrid,
        "component" => Lucide::Component,
        "arrange" => icon::LAYERS,
        "vector" | "bool" => Lucide::PenTool,
        "text" => Lucide::Type,
        "image" => icon::IMAGE,
        "window" => icon::NEW_WINDOW,
        "collab" => Lucide::Users,
        "branch" => icon::BRANCH,
        "help" => icon::HELP,
        "prefs" | "preferences" => icon::SETTINGS,
        _ => Lucide::Shapes,
    }
}

/// The icon of a submenu called `label` in the canvas's menus: the main menu's own menus, and
/// the submenus inside them.
pub(in crate::board) fn submenu(label: &str) -> Lucide {
    match label {
        "File" => Lucide::File,
        "Edit" => icon::EDIT,
        "View" => icon::SHOW,
        "Object" => Lucide::Shapes,
        "Text" => Lucide::Type,
        "Arrange" => icon::LAYERS,
        "Vector" => Lucide::PenTool,
        "Preferences" => icon::SETTINGS,
        "Help" => icon::HELP,
        "Copy as" => icon::COPY,
        "Select all with same" => Lucide::MousePointer2,
        "Outlines" => Lucide::SquareDashed,
        "Panels" => Lucide::PanelLeft,
        "Theme" => Lucide::Palette,
        "Boolean groups" => Lucide::Blend,
        "Send to Agent" | "Send to Agent…" | "Send Flow to Agent" => icon::AGENT,
        "Export" => icon::EXPORT,
        _ => Lucide::ChevronRight,
    }
}

/// The agent a Send to Agent command starts, if `id` is one.
pub(in crate::board) fn agent(id: &str) -> Option<sawit_core::terminal::AgentKind> {
    let (_, kind) = policy::SEND_TO.iter().find(|(own, _)| *own == id)?;
    sawit_core::terminal::AgentKind::all().find(|k| k.id() == *kind)
}

/// The commands with an icon of their own.
fn exact(id: &str) -> Option<Lucide> {
    Some(match id {
        // File
        "file.new" | "file.newBoard" => icon::NEW_FILE,
        "file.newFromTemplate" => Lucide::LayoutTemplate,
        "file.open" | "file.openRecent" => icon::OPEN,
        "file.import" => icon::IMPORT,
        "file.placeImage" => Lucide::ImagePlus,
        "file.save" | "file.saveAs" => icon::SAVE,
        "file.saveVersion" | "file.versionHistory" | "file.revert" => icon::HISTORY_CLOCK,
        "file.export" | "file.exportFramesPdf" => icon::EXPORT,
        "file.exportSvg" => icon::VECTOR,
        "file.duplicate" => icon::DUPLICATE,
        "file.rename" => icon::RENAME,
        "file.close" => icon::CLOSE,
        // Edit
        "edit.undo" => icon::UNDO,
        "edit.redo" => icon::REDO,
        "edit.cut" => icon::CUT,
        "edit.copy" | "edit.copyAsText" | "edit.copyAsCss" | "edit.copyProperties" => icon::COPY,
        "edit.copyAsSvg" => icon::VECTOR,
        "edit.copyAsPng" => icon::IMAGE,
        "edit.copyLink" => icon::LINK,
        "edit.paste" | "edit.pasteOver" | "edit.pasteReplace" | "edit.pasteProperties" => {
            icon::PASTE
        }
        "edit.duplicate" => icon::DUPLICATE,
        "edit.delete" => icon::DELETE,
        "edit.findReplace" => Lucide::Search,
        "edit.pickColor" => Lucide::Pipette,
        "select.all" | "select.inverse" | "select.matching" => icon::SELECT_ALL,
        "select.none" => icon::CLOSE,
        // View
        "view.zoomIn" => Lucide::ZoomIn,
        "view.zoomOut" => Lucide::ZoomOut,
        "view.zoomFit" | "view.zoomSelection" => Lucide::Scan,
        "view.toggleUi" => icon::HIDE,
        "view.fullscreen" => icon::MAXIMIZE,
        "view.rulers" => Lucide::RulerDimensionLine,
        "view.comments" => Lucide::MessageSquare,
        "view.layers" => Lucide::PanelLeft,
        "view.design" => Lucide::PanelRight,
        // Object
        "object.frameSelection" => Lucide::Frame,
        "object.group" => Lucide::Group,
        "object.ungroup" => Lucide::Ungroup,
        "object.flipH" => Lucide::FlipHorizontal2,
        "object.flipV" => Lucide::FlipVertical2,
        "object.toggleVisible" => icon::SHOW,
        "object.toggleLock" => Lucide::Lock,
        "object.mask" | "object.releaseMask" => Lucide::SquareDashed,
        "object.rotate180" | "object.rotateLeft" => Lucide::RotateCcw,
        "object.rotateRight" => Lucide::RotateCw,
        "image.crop" => Lucide::Crop,
        "arrange.front" => Lucide::BringToFront,
        "arrange.back" => Lucide::SendToBack,
        "arrange.forward" => Lucide::ArrowUp,
        "arrange.backward" => Lucide::ArrowDown,
        "layout.add" | "layout.suggest" => Lucide::LayoutGrid,
        "component.create" | "component.createMultiple" => Lucide::Component,
        "component.detach" => Lucide::Unlink,
        // Sawit's own
        policy::ADD_ROUTES => Lucide::Waypoints,
        policy::MAXIMIZE => icon::MAXIMIZE,
        policy::EXPORT_HTML => icon::HTML,
        policy::EXPORT_REACT => icon::REACT,
        policy::BUILD_FULL | policy::BUILD_FAST | policy::BUILD_OFF => Lucide::Film,
        policy::BUILD_SOUND => Lucide::Volume2,
        policy::SUBAGENT_CURSORS => Lucide::MousePointer2,
        policy::VARIABLES => Lucide::Variable,
        policy::COMMENTS => Lucide::MessageSquareText,
        policy::COPY_HANDOFF_PROMPT => icon::COPY,
        policy::REVEAL_HANDOFF => icon::REVEAL,
        _ if id.starts_with("sawit.sendTo.") => icon::AGENT,
        "help.shortcuts" | "help.keyboardShortcuts" => icon::KEYBOARD,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{agent, command, icon, policy};

    #[test]
    fn same_action_same_icon_as_the_app() {
        assert_eq!(command("edit.copy"), icon::COPY);
        assert_eq!(command("edit.delete"), icon::DELETE);
        assert_eq!(command(policy::REVEAL_HANDOFF), icon::REVEAL);
        // A command with no icon of its own takes its menu's.
        assert_eq!(command("view.somethingNew"), icon::SHOW);
        assert!(agent("sawit.sendTo.claude-code").is_some());
        assert!(agent("edit.copy").is_none());
    }
}
