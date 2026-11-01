//! Keys while text is edited, taken as FigCraft's own keyboard takes them, and ending the edit.

use figcraft_engine::keymap::{self, Context, Platform};
use figcraft_engine::{guard, textedit};

use super::route::{run, shortcut};
use super::{KeyOutcome, Mods};
use crate::policy::keeps_text_edit;
use crate::{BoardDoc, BoardError};

/// The copy and paste commands that act on the text being edited: its selected characters are
/// copied or cut, and a paste types the clipboard's text, in FigCraft's text editor for every ⌘V
/// it takes (Shift held or not). Paste to replace (⇧⌘R) replaces layers, and waits.
pub const TEXT_CLIPBOARD: &[&str] = &["edit.copy", "edit.cut", "edit.paste", "edit.pasteOver"];

/// A key while text is edited, taken as FigCraft's own keyboard takes it: the shortcuts live while
/// editing, then typing (the input method's), then the text editor's keys. Canvas shortcuts wait
/// until the edit ends, but for copy and paste, which act on the text ([`TEXT_CLIPBOARD`]).
///
/// Without `shortcuts`, the shortcuts are left to the app's key bindings
/// ([`route_unbound_key`](super::route_unbound_key)): those live while editing, and copy and paste.
pub(super) fn text_key(
    board: &mut BoardDoc,
    platform: Platform,
    key: &str,
    mods: Mods,
    shortcuts: bool,
) -> Result<KeyOutcome, BoardError> {
    if let Some(id) = shortcut(platform, Context::Text, key, mods)
        .filter(|_| shortcuts && (mods.cmd || mods.alt || mods.ctrl))
    {
        return run(board, id);
    }
    if !mods.cmd && !mods.ctrl && types_text(key) {
        return Ok(KeyOutcome::Unhandled);
    }
    if mods.cmd {
        match key {
            // The edit ends first, so the typing is its own undo step and undo takes back that.
            "Z" => {
                end_text_edit(board)?;
                return run(board, if mods.shift { "edit.redo" } else { "edit.undo" });
            }
            "A" => return text_editor_key(board, platform, "SelectAll", mods),
            "Enter" => {
                end_text_edit(board)?;
                return Ok(KeyOutcome::Handled);
            }
            // Copy and paste act on the text, which the caller does with the system clipboard.
            // Other canvas shortcuts (select all layers, duplicate, group…) wait until the edit
            // ends.
            _ if key.chars().count() == 1 => {
                let clipboard = shortcut(platform, Context::Canvas, key, mods)
                    .filter(|id| shortcuts && TEXT_CLIPBOARD.contains(id));
                return Ok(clipboard.map_or(KeyOutcome::Unhandled, KeyOutcome::Ui));
            }
            _ => {}
        }
    }
    text_editor_key(board, platform, key, mods)
}

/// Hand `key` to the text editor: a named key (Backspace, ArrowLeft, Enter…) or `SelectAll`.
fn text_editor_key(
    board: &mut BoardDoc,
    platform: Platform,
    key: &str,
    mods: Mods,
) -> Result<KeyOutcome, BoardError> {
    let (word, line) = keymap::text_motion(platform, key, mods);
    let session = &mut board.session;
    guard::run("ui.textKey", || {
        textedit::key(session, key, mods.shift, word, line)
    })?;
    Ok(KeyOutcome::Handled)
}

/// Whether `key` types into text: one printable character, or the space bar.
fn types_text(key: &str) -> bool {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => !c.is_control(),
        _ => key.eq_ignore_ascii_case("Space"),
    }
}

/// Ends the text edit, if a text is being typed in, as a press outside the text ends it: what was
/// typed becomes one undo step ("Edit text"), a sticky's text gives the selection back to its
/// sticky, and a new text left empty goes. With nothing typed in, it does nothing. Errors are the
/// engine's, and its panics caught by [`guard::run`].
pub fn end_text_edit(board: &mut BoardDoc) -> Result<(), BoardError> {
    let session = &mut board.session;
    guard::run("ui.textEnd", || textedit::end(session))?;
    Ok(())
}

/// Ends the text edit ([`end_text_edit`]) before command `id` runs, unless `id` acts on the text
/// being typed in ([`keeps_text_edit`]: the Text menu's commands, and copy and paste). The
/// canvas's chrome and menus call it before the commands they run (ruling R-TEXTEDIT).
///
/// The engine types inside one gesture, "Edit text", which only the edit's end commits. A command
/// that drops the edit without ending it leaves that gesture open: Select none, a page or tool
/// switch, or deleting the text being typed in. With the gesture open, no later command is an
/// undo step of its own. The next click on a layer then cancels the gesture, which puts the
/// document back as it was before the typing, and the autosave writes that (Plan 6 final review
/// C1).
pub fn end_text_edit_before(board: &mut BoardDoc, id: &str) -> Result<(), BoardError> {
    if keeps_text_edit(id) {
        Ok(())
    } else {
        end_text_edit(board)
    }
}
