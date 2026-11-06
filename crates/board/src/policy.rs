//! Sawit's command policy for the canvas, copied from the spec's Menu policy and Keyboard conflict
//! policy (docs/superpowers/specs/2026-10-29-native-canvas-design.md): the FigCraft commands the
//! canvas hides, the ones Sawit carries out itself instead of the engine, and the keystrokes Sawit
//! keeps while the canvas has the keyboard.
//!
//! The View menu's toggles of the canvas's aids (layout grids, the pixel grid, rulers, slices, and
//! mask and frame outlines) are shown: the overlay draws them ([`crate::overlay`]).

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
    "file.duplicate",
    "file.rename",
    // Collaboration
    "branch.create",
    "collab.join",
    "collab.publish",
    // Edit
    "edit.copyLink",
    // View
    "view.multiplayerCursors",
    "view.themeLight",
    "view.themeDark",
    "view.themeSystem",
    "view.fullscreen",
    // FigCraft's window minimizes (⇧⌘\ at its newer revisions); Sawit's window is Sawit's.
    "view.minimize",
    "view.panelWidths",
    // Object and Text
    "text.missingFonts",
    // Preferences (plugins and libraries are hidden by prefix). FigCraft's UI scale is its own
    // window's: Sawit's chrome has its own zoom.
    "app.preferences",
    "pref.uiScaleUp",
    "pref.uiScaleDown",
    "pref.uiScaleReset",
    // Components
    "component.hideWhenPublishing",
    // Help and tools
    "help.gettingStarted",
    "help.page",
    "help.about",
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

/// Add Routes from Dev Server…: lays the project's dev server's routes out as live frames. Sawit's
/// own command, which the app carries out.
pub const ADD_ROUTES: &str = "sawit.addRoutes";

/// Commands of Sawit's own, not FigCraft's, that the main menu (and so the Actions palette) holds,
/// and the app carries out: their id, their label, the menu they are in and the item they follow
/// there.
pub const SAWIT_COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        ADD_ROUTES,
        "Add Routes from Dev Server…",
        "File",
        "file.placeImage",
    ),
    (MAXIMIZE, "Maximize Canvas", "View", ""),
    (
        EXPORT_HTML,
        "Export as HTML…",
        "File",
        "file.exportFramesPdf",
    ),
    (EXPORT_REACT, "Export as React…", "File", EXPORT_HTML),
    (BUILD_FULL, "Build Animation: Full", "View", MAXIMIZE),
    (BUILD_FAST, "Build Animation: Fast (2×)", "View", BUILD_FULL),
    (BUILD_OFF, "Build Animation: Off", "View", BUILD_FAST),
    (BUILD_SOUND, "Build Sounds", "View", BUILD_OFF),
    (
        SUBAGENT_CURSORS,
        "Show Subagents as Separate Cursors",
        "View",
        BUILD_SOUND,
    ),
    (VARIABLES, "Variables…", "View", "view.proofSrgb"),
    (COMMENTS, "Comments…", "View", VARIABLES),
];

/// How agents' builds play on the canvas (Full, Fast, Off), whether they click, and whether a
/// session's subagents show as cursors of their own: Sawit's own settings, which the app keeps and
/// ticks in the View menu.
pub const BUILD_FULL: &str = "sawit.buildFull";
pub const BUILD_FAST: &str = "sawit.buildFast";
pub const BUILD_OFF: &str = "sawit.buildOff";
pub const BUILD_SOUND: &str = "sawit.buildSound";
pub const SUBAGENT_CURSORS: &str = "sawit.subagentCursors";

/// Maximize Canvas: the canvas fills the window, or the layout comes back. Sawit's own command,
/// which the app carries out; its keys are the workspace's (⌃⌘3).
pub const MAXIMIZE: &str = "sawit.maximize";

/// Send to Agent… for the screens and artboards selected (several hand off as one flow): one
/// command for each coding agent Sawit can start with a prompt (the agent's id after the dot),
/// Copy Prompt and Reveal Handoff Folder. Sawit's own, which the app carries out
/// (`crate::handoff`).
pub const SEND_TO: &[(&str, &str)] = &[
    ("sawit.sendTo.claude-code", "claude-code"),
    ("sawit.sendTo.codex", "codex"),
    ("sawit.sendTo.gemini", "gemini"),
    ("sawit.sendTo.opencode", "opencode"),
];
pub const COPY_HANDOFF_PROMPT: &str = "sawit.copyHandoffPrompt";
pub const REVEAL_HANDOFF: &str = "sawit.revealHandoff";

/// Whether `id` is one of the handoff's commands ([`SEND_TO`], Copy Prompt, Reveal).
pub fn is_handoff(id: &str) -> bool {
    SEND_TO.iter().any(|(own, _)| *own == id) || id == COPY_HANDOFF_PROMPT || id == REVEAL_HANDOFF
}

/// Export HTML and Export React of the screen or the artboard selected alone: Sawit's own, which
/// the app carries out (`crate::frame_code`, `crate::screens::export`).
pub const EXPORT_HTML: &str = "sawit.exportHtml";
pub const EXPORT_REACT: &str = "sawit.exportReact";

/// Comments…: the window of the page's comment threads. Sawit's own command, which the app
/// carries out.
pub const COMMENTS: &str = "sawit.comments";

/// Variables…: the window of the file's variable collections, their modes and their values.
/// Sawit's own command, which the app carries out.
pub const VARIABLES: &str = "sawit.variables";

/// Copy, cut and paste. The engine runs them on its own copy of the layers, but each canvas tab
/// has its own session, and the system clipboard is Sawit's to read and write, so Sawit carries
/// them out around the engine ([`crate::clipboard`]): what one tab copies pastes in another, and a
/// paste takes what was copied last, here or in another app. Their shortcuts come back to the view
/// as [`crate::input::KeyOutcome::Ui`], as [`UI_COMMANDS`]' do.
pub const CLIPBOARD: &[&str] = &[
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.pasteOver",
    "edit.pasteReplace",
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

/// Whether Sawit carries out command `id` itself instead of running it in the engine: one of
/// [`UI_COMMANDS`], or of Sawit's own ([`SAWIT_COMMANDS`]).
pub fn is_ui(id: &str) -> bool {
    UI_COMMANDS.contains(&id) || SAWIT_COMMANDS.iter().any(|(own, ..)| *own == id) || is_handoff(id)
}

/// Whether command `id` copies or pastes, which Sawit carries out around the engine
/// ([`CLIPBOARD`]).
pub fn is_clipboard(id: &str) -> bool {
    CLIPBOARD.contains(&id)
}

/// Whether command `id` acts on the text being typed in, and so runs with the edit still open:
/// the Text menu's commands (`text.*`), which style the text or the characters selected in it,
/// and copy and paste ([`CLIPBOARD`]), which the canvas's keys and menus send to its characters.
/// Before any other command the canvas ends the edit first
/// ([`crate::input::end_text_edit_before`], ruling R-TEXTEDIT). An agent's `command` op hands
/// copy and paste to the engine, which acts on layers, so it keeps the edit open for a copy alone
/// (`crate::ops`, CANVAS-CORE-2).
pub fn keeps_text_edit(id: &str) -> bool {
    id.starts_with("text.") || is_clipboard(id)
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
        assert!(is_ui(ADD_ROUTES) && !is_hidden(ADD_ROUTES));
    }

    #[test]
    fn every_view_toggle_shown_is_one_the_canvas_carries_out() {
        // The toggles that tick in the menus: the render draws the outline modes, the soft proof
        // and pixel preview (`render_job` in crates/app/src/board/mod.rs), and the engine's own
        // tools snap as the three snapping ones say. Any other would tick and change nothing.
        const CARRIED_OUT: &[&str] = &[
            "view.pixelGrid",
            "view.layoutGrids",
            "view.rulers",
            "view.slices",
            "view.comments",
            "view.outlines",
            "view.outlinesSelection",
            "view.outlinesHiddenLayers",
            "view.outlinesObjectBounds",
            "view.pixelPreview",
            "view.maskOutlines",
            "view.frameOutlines",
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
        // The aids the overlay draws (`crate::overlay`) are View toggles that are shown.
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
            assert!(!is_hidden(id), "{id}");
        }
    }

    #[test]
    fn clipboard_commands_are_their_own_list() {
        for id in [
            "edit.copy",
            "edit.cut",
            "edit.paste",
            "edit.pasteOver",
            "edit.pasteReplace",
        ] {
            assert!(is_clipboard(id), "{id}");
            // The engine still runs them: they are neither hidden nor the spec's UI-handled ids.
            assert!(!is_hidden(id) && !is_ui(id), "{id}");
        }
        assert!(!is_clipboard("edit.copyAsPng"));
        assert!(!is_clipboard("edit.pasteProperties"));
    }

    #[test]
    fn only_text_commands_and_copy_and_paste_keep_a_text_edit_open() {
        // They act on the text being typed in: its style, or its characters.
        for id in [
            "text.bold",
            "text.alignCenter",
            "text.bulletList",
            "edit.copy",
            "edit.cut",
            "edit.paste",
            "edit.pasteOver",
        ] {
            assert!(keeps_text_edit(id), "{id}");
        }
        // Any other ends the edit first, those that drop it without ending it among them: Select
        // none, deleting what is typed in, a page or tool switch.
        for id in [
            "select.none",
            "select.all",
            "edit.delete",
            "edit.undo",
            "page.select",
            "tool.hand",
            "view.zoomFit",
            "board.nextSticky",
            "file.export",
            "edit.copyAsText",
        ] {
            assert!(!keeps_text_edit(id), "{id}");
        }
    }

    #[test]
    fn policy_ids_are_figcraft_ids() {
        // A misspelt id would hide nothing, or leave a command to the engine.
        let known = |id: &str| {
            catalog::items().any(|m| m.id == id) || keymap::KEYS.iter().any(|b| b.id == id)
        };
        for id in HIDDEN.iter().chain(UI_COMMANDS).chain(CLIPBOARD) {
            assert!(known(id), "{id} is not a FigCraft menu item or shortcut");
        }
    }
}
