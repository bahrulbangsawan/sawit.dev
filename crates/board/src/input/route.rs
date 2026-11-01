//! Where a key goes on the canvas, under Sawit's command policy, and the commands its shortcut or
//! key binding runs.

use figcraft_doc::{NodeId, NodeKind};
use figcraft_engine::keymap::{self, Context, Platform};
use figcraft_engine::{Session, guard, vectoredit};
use serde_json::json;

use super::keys::figcraft_key;
use super::text::{TEXT_CLIPBOARD, end_text_edit_before, text_key};
use super::{KeyOutcome, Mods};
use crate::policy::{is_clipboard, is_hidden, is_ui};
use crate::{BoardDoc, BoardError};

/// Send `key` (a FigCraft key name, see [`key_name`](super::key_name)) with `mods` to the canvas,
/// as FigCraft does.
///
/// A port of FigCraft's `Headless::key` (crates/mcp/src/headless.rs at the pinned rev) under
/// Sawit's command policy:
/// - The shortcut of a hidden command counts as no shortcut: [`KeyOutcome::Unhandled`].
/// - The shortcut of a command Sawit carries out itself, a copy or a paste among them, comes back
///   as [`KeyOutcome::Ui`], unrun.
/// - While text is edited, a typed character or space without ⌘ or ⌃ is left to the input method,
///   which inserts it: [`KeyOutcome::Unhandled`].
///
/// Where `Headless::key` and FigCraft's own keyboard (its egui UI) differ, this does as the
/// keyboard does:
/// - A letter is read in upper case, and a symbol from the US digit row as its digit with Shift,
///   since GPUI reports ⇧0 as `)`.
/// - While text is edited, only the shortcuts live while editing and the text editor's keys run.
///   ⌘Z and ⇧⌘Z end the edit, so it becomes one undo step, then undo or redo; ⌘A selects all the
///   text; ⌘↩ ends the edit; ⌘C, ⌘X, ⌘V and ⇧⌘V come back as [`KeyOutcome::Ui`] for the caller
///   to copy, cut and paste the text's characters; any other ⌘ with a character is
///   [`KeyOutcome::Unhandled`].
/// - A number key with nothing selected is [`KeyOutcome::Unhandled`].
/// - In vector edit mode ⌘X takes the selected points out of their paths, healing each path
///   through them, or with no point selected the selected segments, and the layers stay. Nothing
///   is copied: it is [`KeyOutcome::Handled`], and the system clipboard stays as it was.
/// - Delete (⌫) while the Pen draws finishes its path, which stays, where `edit.delete` would
///   take the path being drawn, which is selected, off the canvas.
/// - ⌘ with an arrow resizes the selection by the nudge (`transform.nudgeSize`): right and down
///   grow it, left and up shrink it, its top-left kept.
///
/// A key that is no shortcut and no key FigCraft knows is [`KeyOutcome::Unhandled`] too. What the
/// commands ask of the view stays in `session.ui_requests` for the caller. Errors are the engine's
/// refusals, and the engine's panics caught by [`guard::run`].
pub fn route_key(board: &mut BoardDoc, key: &str, mods: Mods) -> Result<KeyOutcome, BoardError> {
    route(board, key, mods, true)
}

/// [`route_key`] as the canvas's default key bindings take the key: Sawit's defaults
/// ([`crate::shortcuts::key_bindings`]), which leave out the keys Sawit keeps (⌘J, ⌥⌘O…) and the
/// commands the canvas doesn't offer (`/`), not FigCraft's raw keymap. A key one of them binds
/// runs its command through [`route_command`], as its binding does; any other key goes through
/// [`route_unbound_key`], as on the canvas. The keymap file's changes don't count: this is what
/// the canvas does with its default keys, the same with or without a keymap.
pub fn route_default_key(
    board: &mut BoardDoc,
    key: &str,
    mods: Mods,
) -> Result<KeyOutcome, BoardError> {
    let editing = board.session.ds().is_ok_and(|ds| ds.text_edit.is_some());
    match default_command(key, mods, editing) {
        Some(id) => route_command(board, id, Some((key, mods))),
        None => route_unbound_key(board, key, mods),
    }
}

/// The command the canvas's default binding of `key` with `mods` runs
/// ([`crate::shortcuts::key_bindings`]): in [`crate::shortcuts::TEXT_CONTEXT`] while text is
/// edited, else in [`crate::shortcuts::CANVAS_CONTEXT`]. `None` when the canvas binds no such key
/// by default.
fn default_command(key: &str, mods: Mods, editing: bool) -> Option<&'static str> {
    use crate::shortcuts::{
        CANVAS_CONTEXT, TEXT_CONTEXT, binding_keystroke, defaults, key_bindings,
    };
    let (key, mods) = figcraft_key(key, mods);
    let context = if editing {
        TEXT_CONTEXT
    } else {
        CANVAS_CONTEXT
    };
    let bound = key_bindings();
    defaults().find_map(|binding| {
        binding
            .keys
            .iter()
            .filter(|spec| keymap::matches(spec, Platform::Mac, &key, mods))
            .find_map(|spec| {
                let keystroke = binding_keystroke(spec)?;
                bound
                    .iter()
                    .any(|b| b.id == binding.id && b.context == context && b.keystroke == keystroke)
                    .then_some(binding.id)
            })
    })
}

/// [`route_key`] for a key no key binding of the app took. FigCraft's shortcuts are the app's key
/// bindings ([`crate::shortcuts::key_bindings`]), which the user may change in the keymap editor,
/// so this leaves them out: the key reaches the tools, the text editor and the canvas's own keys
/// (the arrows, the number keys), and is [`KeyOutcome::Unhandled`] otherwise. A shortcut's
/// binding runs [`route_command`].
pub fn route_unbound_key(
    board: &mut BoardDoc,
    key: &str,
    mods: Mods,
) -> Result<KeyOutcome, BoardError> {
    route(board, key, mods, false)
}

/// Runs command `id` for a key binding of the app, as [`route_key`] runs it for its shortcut. `key`
/// is the key that was pressed (a FigCraft key name, see [`key_name`](super::key_name)), if known.
///
/// - While text is edited copy and paste act on the text, and come back as [`KeyOutcome::Ui`]
///   for the caller ([`TEXT_CLIPBOARD`]). A command FigCraft's keymap has live while editing
///   (bold, a new sticky, save) runs inside the edit, as its shortcut does. Any other command,
///   which only a binding the user made runs here, ends the edit first, as the canvas's menus do
///   ([`end_text_edit_before`], ruling R-TEXTEDIT): run inside the edit, Select none or a delete
///   would leave the engine's "Edit text" gesture open, and the next click would take the typing
///   back.
/// - Otherwise the tools have the key first, as for a shortcut: in vector edit mode a letter picks
///   a vector tool, not the canvas's.
/// - A hidden command, or one FigCraft doesn't have, is [`KeyOutcome::Unhandled`].
pub fn route_command(
    board: &mut BoardDoc,
    id: &str,
    key: Option<(&str, Mods)>,
) -> Result<KeyOutcome, BoardError> {
    let Some(id) = command_id(id).filter(|id| !is_hidden(id)) else {
        return Ok(KeyOutcome::Unhandled);
    };
    if board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        if TEXT_CLIPBOARD.contains(&id) {
            return Ok(KeyOutcome::Ui(id));
        }
        if live_while_editing(id) {
            return run(board, id);
        }
        end_text_edit_before(board, id)?;
        return shortcut_command(board, id);
    }
    if let Some((key, mods)) = key {
        let (key, mods) = figcraft_key(key, mods);
        let key: &str = &key;
        let session = &mut board.session;
        if guard::run("ui.toolKey", || {
            session.tool_key(key, mods.shift, mods.alt, mods.cmd)
        })? {
            return Ok(KeyOutcome::Handled);
        }
    }
    shortcut_command(board, id)
}

/// The id of FigCraft's command `id` as FigCraft spells it: a shortcut's, a menu item's or an
/// engine command's. `None` when FigCraft has no such command.
pub fn command_id(id: &str) -> Option<&'static str> {
    keymap::KEYS
        .iter()
        .map(|binding| binding.id)
        .chain(figcraft_engine::catalog::items().map(|item| item.id))
        .chain(
            figcraft_engine::all_commands()
                .iter()
                .map(|command| command.id),
        )
        .find(|known| *known == id)
}

/// Whether FigCraft's keymap has a shortcut for command `id` that is live while text is edited:
/// the text styles, a new sticky, save, the shortcuts sheet.
fn live_while_editing(id: &str) -> bool {
    keymap::KEYS
        .iter()
        .any(|binding| binding.id == id && binding.context != Context::Canvas)
}

/// Runs command `id` for its shortcut on the canvas (not while text is edited): Escape and Enter's
/// commands do as those keys do, and ⌘X cuts points in vector edit mode.
fn shortcut_command(board: &mut BoardDoc, id: &'static str) -> Result<KeyOutcome, BoardError> {
    match id {
        "select.none" => guard::run("ui.escape", || board.session.escape())?,
        "select.children" => enter(board)?,
        "edit.cut" if vector_editing(&board.session) => cut_points(board)?,
        "edit.delete" if board.session.tool.pen.is_some() => {
            let session = &mut board.session;
            // As in FigCraft, a pen that can't finish leaves nothing to report.
            let _ = guard::run("ui.penFinish", || session.pen_finish());
        }
        id => return run(board, id),
    }
    Ok(KeyOutcome::Handled)
}

/// [`route_key`], with FigCraft's shortcuts looked up when `shortcuts` is set.
fn route(
    board: &mut BoardDoc,
    key: &str,
    mods: Mods,
    shortcuts: bool,
) -> Result<KeyOutcome, BoardError> {
    let (key, mods) = figcraft_key(key, mods);
    let key: &str = &key;
    let platform = Platform::current();
    if board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        return text_key(board, platform, key, mods, shortcuts);
    }
    // Vector edit mode and shape builder keys (tools, nudging points, delete, join, Enter), as on
    // the canvas.
    let session = &mut board.session;
    if guard::run("ui.toolKey", || {
        session.tool_key(key, mods.shift, mods.alt, mods.cmd)
    })? {
        return Ok(KeyOutcome::Handled);
    }
    if let Some(id) = shortcut(platform, Context::Canvas, key, mods).filter(|_| shortcuts) {
        return shortcut_command(board, id);
    }
    let Some((name, _)) = keymap::normalize(key) else {
        return Ok(KeyOutcome::Unhandled);
    };
    match name {
        "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
            let (dx, dy) = match name {
                "ArrowLeft" => (-1, 0),
                "ArrowRight" => (1, 0),
                "ArrowUp" => (0, -1),
                _ => (0, 1),
            };
            // With nothing selected the arrows pan the view; with ⌘ they resize the selection,
            // right and down growing it.
            let (cmd, params) = if board.session.selection().is_empty() {
                ("view.pan", json!({"dx": dx, "dy": dy, "big": mods.shift}))
            } else if mods.cmd {
                (
                    "transform.nudgeSize",
                    json!({"dw": dx, "dh": dy, "big": mods.shift}),
                )
            } else {
                (
                    "transform.nudge",
                    json!({"dx": dx, "dy": dy, "big": mods.shift}),
                )
            };
            board.execute(cmd, &params)?;
        }
        // Number keys set the selection's opacity. With nothing selected they do nothing, as in
        // FigCraft's own keyboard (the command would refuse).
        digit
            if mods.is_none()
                && digit.len() == 1
                && digit.chars().all(|c| c.is_ascii_digit())
                && board.session.prefs.number_keys_opacity
                && !board.session.selection().is_empty() =>
        {
            board.execute("object.opacityKeys", &json!({"digits": digit}))?;
        }
        _ => return Ok(KeyOutcome::Unhandled),
    }
    Ok(KeyOutcome::Handled)
}

/// The command `key` with `mods` is the shortcut of in `context`. A hidden command's shortcut
/// counts as none.
pub(super) fn shortcut(
    platform: Platform,
    context: Context,
    key: &str,
    mods: Mods,
) -> Option<&'static str> {
    keymap::lookup(platform, context, key, mods)
        .map(|binding| binding.id)
        .filter(|id| !is_hidden(id))
}

/// Run command `id` for its shortcut, unless Sawit carries it out itself.
pub(super) fn run(board: &mut BoardDoc, id: &'static str) -> Result<KeyOutcome, BoardError> {
    if is_ui(id) || is_clipboard(id) {
        return Ok(KeyOutcome::Ui(id));
    }
    // From the keyboard, typing goes on in the new sticky, as in FigCraft's own menus.
    let params = if id == "board.nextSticky" {
        json!({"edit": true})
    } else {
        json!({})
    };
    board.execute(id, &params)?;
    Ok(KeyOutcome::Handled)
}

/// Enter on the canvas: with a shape or frame tool chosen it places one in the middle of the view;
/// on one text layer it starts editing it; it ends the path the pen draws; on shapes it edits them
/// as vectors (several at once); else it selects the children.
fn enter(board: &mut BoardDoc) -> Result<(), BoardError> {
    let session = &board.session;
    if session.tool.tool.draws_shape() {
        board.execute("tool.place", &json!({}))?;
    } else if let Some(id) = single_text(session) {
        board.execute("text.edit.begin", &json!({"id": id}))?;
    } else if session.tool.pen.is_some() {
        let session = &mut board.session;
        // As in FigCraft, a pen that can't end leaves nothing to report.
        let _ = guard::run("ui.penEnd", || session.pen_end());
    } else if let Some(ids) = vector_targets(session) {
        board.execute("vector.edit.begin", &json!({"ids": ids}))?;
    } else {
        board.execute("select.children", &json!({}))?;
    }
    Ok(())
}

/// The selected layer when exactly one text layer is selected.
fn single_text(session: &Session) -> Option<NodeId> {
    let doc = session.doc().ok()?;
    match session.selection().as_slice() {
        [id] if doc
            .node(*id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Text(_))) =>
        {
            Some(*id)
        }
        _ => None,
    }
}

/// The selected layers Enter edits as vectors.
fn vector_targets(session: &Session) -> Option<Vec<NodeId>> {
    vectoredit::enter_targets(session.doc().ok()?, &session.selection())
}

/// Whether layers are edited as vectors (vector edit mode).
fn vector_editing(session: &Session) -> bool {
    session.ds().is_ok_and(|ds| ds.vector_edit.is_some())
}

/// ⌘X in vector edit mode, as FigCraft's own keyboard cuts there (ui-egui shortcuts.rs `cut`):
/// the selected points go, in every layer edited, each taken out with a heal that joins the path
/// through it; else the selected segments go; with nothing selected, nothing does. The layers
/// stay, and nothing is copied.
fn cut_points(board: &mut BoardDoc) -> Result<(), BoardError> {
    let parts = board
        .session
        .ds()?
        .vector_edit
        .as_ref()
        .map(|edit| edit.parts())
        .unwrap_or_default();
    if parts.iter().any(|part| !part.vertices.is_empty()) {
        board.execute("vector.deleteHeal", &json!({}))?;
    } else if parts.iter().any(|part| !part.segments.is_empty()) {
        board.execute("vector.edit.delete", &json!({}))?;
    }
    Ok(())
}
