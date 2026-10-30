//! Sawit's command policy for the canvas, copied from the spec's Menu policy and Keyboard conflict
//! policy (docs/superpowers/specs/2026-10-29-native-canvas-design.md): the FigCraft commands the
//! canvas hides, the ones Sawit carries out itself instead of the engine, and the keystrokes Sawit
//! keeps while the canvas has the keyboard.
//!
//! The hidden commands also take the View menu's toggles of what the canvas doesn't draw: in
//! FigCraft its own canvas draws them over the page, and the native canvas's overlay
//! ([`crate::overlay`]) has none of them, so they would tick and change nothing.

/// Commands the canvas never shows and never runs from the keyboard: out of scope, or Sawit owns
/// them. Whole families are hidden through [`HIDDEN_PREFIXES`].
pub const HIDDEN: &[&str] = &[
    // File
    "file.new",
    "file.newBoard",
    "file.newFromTemplate",
    "window.new",
    "file.open",
    "file.openRecent",
    "file.close",
    "file.saveVersion",
    "file.versionHistory",
    "file.duplicate",
    "file.rename",
    // Collaboration
    "branch.create",
    "collab.join",
    "collab.publish",
    // Edit
    "edit.copyLink",
    "edit.findReplace",
    "edit.pickColor",
    "edit.setDefaultProperties",
    // View
    "view.comments",
    "view.multiplayerCursors",
    "view.assets",
    "view.prototype",
    "view.themeLight",
    "view.themeDark",
    "view.themeSystem",
    "view.fullscreen",
    "view.panelWidths",
    // View toggles of what the canvas doesn't draw (its overlay has no rulers, grids, slices, or
    // mask and frame outlines). Shown, they would tick and change nothing.
    "view.pixelGrid",
    "view.layoutGrids",
    "view.rulers",
    "view.slices",
    "view.maskOutlines",
    "view.frameOutlines",
    // Object and Text
    "object.setThumbnail",
    "text.missingFonts",
    // Preferences (plugins and libraries are hidden by prefix)
    "app.preferences",
    // Components
    "component.hideWhenPublishing",
    // Help and tools
    "help.gettingStarted",
    "help.page",
    "help.about",
    "tool.comment",
    "tool.shapeBuilder",
    // Document tabs
    "doc.nextTab",
    "doc.previousTab",
];

/// The command families the canvas hides whole: plugins and libraries.
pub const HIDDEN_PREFIXES: &[&str] = &["plugins.", "plugin.", "libraries."];

/// Commands Sawit carries out itself instead of the engine: import and place an image (an open
/// dialog), export (to `.nyawit/exports/`), save (the autosave now, or a copy elsewhere) and copy as
/// text, CSS, SVG or PNG (the system clipboard).
pub const UI_COMMANDS: &[&str] = &[
    // Import
    "file.import",
    "file.placeImage",
    // Export
    "file.export",
    "file.exportSvg",
    "file.exportFramesPdf",
    // Save
    "file.save",
    "file.saveAs",
    // Copy as
    "edit.copyAsText",
    "edit.copyAsCss",
    "edit.copyAsSvg",
    "edit.copyAsPng",
];

/// Keystrokes that stay Sawit's while the canvas has the keyboard, although a FigCraft command uses
/// them too. Written as GPUI keystrokes with the modifiers in the order ctrl, alt, shift, cmd, as
/// [`crate::input::gpui_keystroke`] writes them.
pub const SAWIT_WINS: &[&str] = &[
    "cmd-w",
    "cmd-o",
    "ctrl-tab",
    "ctrl-shift-tab",
    "ctrl-cmd-m",
    "alt-cmd-o",
    "cmd-j",
    "shift-cmd-j",
];

/// Keystrokes that go to the canvas while it edits text (bold, centre, a new sticky) and stay
/// Sawit's otherwise. Written as [`SAWIT_WINS`] is.
pub const SAWIT_WINS_UNLESS_EDITING: &[&str] = &["cmd-b", "alt-cmd-t", "shift-cmd-enter"];

/// Whether command `id` is hidden: out of the menus, and dead on the keyboard.
pub fn is_hidden(id: &str) -> bool {
    HIDDEN.contains(&id) || HIDDEN_PREFIXES.iter().any(|prefix| id.starts_with(prefix))
}

/// Whether Sawit carries out command `id` itself instead of running it in the engine.
pub fn is_ui(id: &str) -> bool {
    UI_COMMANDS.contains(&id)
}

#[cfg(test)]
mod tests {
    use figcraft_engine::cmd::view;
    use figcraft_engine::{catalog, keymap};

    use super::*;

    #[test]
    fn policy_lists_match_spec() {
        assert!(is_hidden("file.new"));
        assert!(is_hidden("plugins.manage"));
        assert!(!is_hidden("edit.copy"));
        assert!(is_ui("file.export"));
    }

    #[test]
    fn every_view_toggle_shown_is_one_the_canvas_carries_out() {
        // The toggles that tick in the menus: the render draws the outline modes, the soft proof
        // and pixel preview (`render_job` in crates/app/src/board/mod.rs), and the engine's own
        // tools snap as the three snapping ones say. Any other would tick and change nothing.
        const CARRIED_OUT: &[&str] = &[
            "view.outlines",
            "view.outlinesSelection",
            "view.outlinesHiddenLayers",
            "view.outlinesObjectBounds",
            "view.pixelPreview",
            "view.proofSrgb",
            "view.snapGeometry",
            "view.snapObjects",
            "view.snapPixel",
        ];
        let session = crate::BoardDoc::new_board("Canvas").unwrap().session;
        let ticking: Vec<&str> = catalog::items()
            .map(|item| item.id)
            .filter(|id| !is_hidden(id) && view::checked(&session, id).is_some())
            .collect();
        assert_eq!(ticking, CARRIED_OUT);
        // What the canvas doesn't draw is out of the menus and off the keyboard.
        for id in [
            "view.pixelGrid",
            "view.layoutGrids",
            "view.rulers",
            "view.slices",
            "view.maskOutlines",
            "view.frameOutlines",
        ] {
            assert!(
                view::checked(&session, id).is_some(),
                "{id} is no View toggle"
            );
            assert!(is_hidden(id), "{id}");
        }
    }

    #[test]
    fn policy_ids_are_figcraft_ids() {
        // A misspelt id would hide nothing, or leave a command to the engine.
        let known = |id: &str| {
            catalog::items().any(|m| m.id == id) || keymap::KEYS.iter().any(|b| b.id == id)
        };
        for id in HIDDEN.iter().chain(UI_COMMANDS) {
            assert!(known(id), "{id} is not a FigCraft menu item or shortcut");
        }
    }
}
