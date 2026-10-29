//! Keys on the canvas. GPUI and FigCraft name keys differently, so [`key_name`] and
//! [`gpui_keystroke`] translate between them, and [`route_key`] sends a key where FigCraft's own
//! keymap says, under Sawit's command policy ([`crate::policy`]).

use figcraft_doc::{NodeId, NodeKind};
use figcraft_engine::keymap::{self, Context, Platform};
use figcraft_engine::{Session, guard, textedit, vectoredit};
use serde_json::json;

use crate::policy::{is_hidden, is_ui};
use crate::{BoardDoc, BoardError};

pub use figcraft_engine::keymap::Mods;

/// What became of a key sent to the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The canvas used it: a command ran, or a tool or the text editor took it.
    Handled,
    /// It is the shortcut of a command Sawit carries out itself ([`crate::policy::UI_COMMANDS`]).
    /// Nothing ran: carrying it out is the caller's job.
    Ui(&'static str),
    /// The canvas has no use for it: it is no shortcut, or a hidden command's, or it types into
    /// the text being edited, which the input method does.
    Unhandled,
}

/// The keys FigCraft names in words: GPUI's name, then FigCraft's.
const NAMED_KEYS: &[(&str, &str)] = &[
    ("backspace", "Backspace"),
    ("delete", "Delete"),
    ("left", "ArrowLeft"),
    ("right", "ArrowRight"),
    ("up", "ArrowUp"),
    ("down", "ArrowDown"),
    ("enter", "Enter"),
    ("escape", "Escape"),
    ("tab", "Tab"),
    ("space", "Space"),
    ("home", "Home"),
    ("end", "End"),
    ("pageup", "PageUp"),
    ("pagedown", "PageDown"),
    ("f1", "F1"),
    ("f2", "F2"),
    ("f3", "F3"),
    ("f4", "F4"),
    ("f5", "F5"),
    ("f6", "F6"),
    ("f7", "F7"),
    ("f8", "F8"),
    ("f9", "F9"),
    ("f10", "F10"),
    ("f11", "F11"),
    ("f12", "F12"),
];

/// FigCraft's name for a key as GPUI reports it (`Keystroke::key`): `backspace` is `Backspace`,
/// `left` is `ArrowLeft`, and a character stays as it is. `None` for a key FigCraft has no name
/// for, such as Caps Lock.
pub fn key_name(gpui_key: &str) -> Option<String> {
    if gpui_key.chars().count() == 1 {
        return Some(gpui_key.to_string());
    }
    NAMED_KEYS
        .iter()
        .find(|(gpui, _)| *gpui == gpui_key)
        .map(|(_, figcraft)| (*figcraft).to_string())
}

/// The GPUI keystroke for a FigCraft binding: `Shift+Cmd+K` is `shift-cmd-k`. The modifiers come
/// in the order ctrl, alt, shift, cmd, as Sawit's workspace writes keys. A shifted symbol is its
/// key with Shift (`Plus` is `shift-=`), and `Delete` is the key that deletes backwards, which
/// GPUI calls `backspace`. `None` when `spec` is no binding FigCraft can read.
pub fn gpui_keystroke(spec: &str) -> Option<String> {
    let (mods, key) = keymap::parse(spec)?;
    let key = gpui_key(key)?;
    let mut keystroke = String::new();
    for (on, modifier) in [
        (mods.ctrl, "ctrl-"),
        (mods.alt, "alt-"),
        (mods.shift, "shift-"),
        (mods.cmd, "cmd-"),
    ] {
        if on {
            keystroke.push_str(modifier);
        }
    }
    keystroke.push_str(&key);
    Some(keystroke)
}

/// GPUI's name for a key as [`keymap::parse`] gives it.
fn gpui_key(key: &str) -> Option<String> {
    // FigCraft reads Backspace and Delete as one key, `Delete`: on a Mac keyboard that is ⌫.
    if key == "Delete" {
        return Some("backspace".into());
    }
    if key.chars().count() == 1 {
        return Some(key.to_ascii_lowercase());
    }
    // A binding may write a named key in any case (`parse` keeps it as written).
    NAMED_KEYS
        .iter()
        .find(|(_, figcraft)| figcraft.eq_ignore_ascii_case(key))
        .map(|(gpui, _)| (*gpui).to_string())
}

/// Send `key` (a FigCraft key name, see [`key_name`]) with `mods` to the canvas, as FigCraft does.
///
/// A port of FigCraft's `Headless::key` (crates/mcp/src/headless.rs at the pinned rev) under
/// Sawit's command policy:
/// - The shortcut of a hidden command counts as no shortcut: [`KeyOutcome::Unhandled`].
/// - The shortcut of a command Sawit carries out itself comes back as [`KeyOutcome::Ui`], unrun.
/// - While text is edited, a typed character or space without ⌘ or ⌃ is left to the input method,
///   which inserts it: [`KeyOutcome::Unhandled`].
///
/// A key that is no shortcut and no key FigCraft knows is [`KeyOutcome::Unhandled`] too. What the
/// commands ask of the view stays in `session.ui_requests` for the caller. Errors are the engine's
/// refusals, and the engine's panics caught by [`guard::run`].
pub fn route_key(board: &mut BoardDoc, key: &str, mods: Mods) -> Result<KeyOutcome, BoardError> {
    let platform = Platform::current();
    // Text editing keys go to the editor, after the shortcuts live while editing; other Cmd+letter
    // shortcuts (undo…) still reach the canvas.
    if board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        if let Some(id) = shortcut(platform, Context::Text, key, mods)
            .filter(|_| mods.cmd || mods.alt || mods.ctrl)
        {
            return run(board, id);
        }
        if !mods.cmd && !mods.ctrl && types_text(key) {
            return Ok(KeyOutcome::Unhandled);
        }
        if !(mods.cmd && key.len() == 1) {
            let (word, line) = keymap::text_motion(platform, key, mods);
            let session = &mut board.session;
            guard::run("ui.textKey", || {
                textedit::key(session, key, mods.shift, word, line)
            })?;
            return Ok(KeyOutcome::Handled);
        }
    }
    // Vector edit mode and shape builder keys (tools, nudging points, delete, join, Enter), as on
    // the canvas.
    let session = &mut board.session;
    if guard::run("ui.toolKey", || {
        session.tool_key(key, mods.shift, mods.alt, mods.cmd)
    })? {
        return Ok(KeyOutcome::Handled);
    }
    if let Some(id) = shortcut(platform, Context::Canvas, key, mods) {
        match id {
            "select.none" => guard::run("ui.escape", || board.session.escape())?,
            "select.children" => enter(board)?,
            id => return run(board, id),
        }
        return Ok(KeyOutcome::Handled);
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
            // With nothing selected the arrows pan the view.
            let cmd = if board.session.selection().is_empty() {
                "view.pan"
            } else {
                "transform.nudge"
            };
            board.execute(cmd, &json!({"dx": dx, "dy": dy, "big": mods.shift}))?;
        }
        // Number keys set the selection's opacity.
        digit
            if mods.is_none()
                && digit.len() == 1
                && digit.chars().all(|c| c.is_ascii_digit())
                && board.session.prefs.number_keys_opacity =>
        {
            board.execute("object.opacityKeys", &json!({"digits": digit}))?;
        }
        _ => return Ok(KeyOutcome::Unhandled),
    }
    Ok(KeyOutcome::Handled)
}

/// The command `key` with `mods` is the shortcut of in `context`. A hidden command's shortcut
/// counts as none.
fn shortcut(platform: Platform, context: Context, key: &str, mods: Mods) -> Option<&'static str> {
    keymap::lookup(platform, context, key, mods)
        .map(|binding| binding.id)
        .filter(|id| !is_hidden(id))
}

/// Run command `id` for its shortcut, unless Sawit carries it out itself.
fn run(board: &mut BoardDoc, id: &'static str) -> Result<KeyOutcome, BoardError> {
    if is_ui(id) {
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

/// Whether `key` types into text: one printable character, or the space bar.
fn types_text(key: &str) -> bool {
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => !c.is_control(),
        _ => key.eq_ignore_ascii_case("Space"),
    }
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

#[cfg(test)]
mod tests {
    use figcraft_doc::NodeId;
    use figcraft_engine::keymap::{KEYS, Platform};
    use serde_json::json;

    use super::*;

    const NONE: Mods = Mods {
        shift: false,
        alt: false,
        cmd: false,
        ctrl: false,
    };
    const CMD: Mods = Mods { cmd: true, ..NONE };

    fn board() -> BoardDoc {
        BoardDoc::new_board("Canvas").unwrap()
    }

    /// A new sticky reading `text`.
    fn sticky(board: &mut BoardDoc, text: &str) -> NodeId {
        let made = board
            .execute("board.sticky", &json!({"x": 0, "y": 0, "text": text}))
            .unwrap();
        figcraft_engine::params::id_of(&made["id"]).unwrap()
    }

    /// What sticky `id` reads.
    fn sticky_text(board: &BoardDoc, id: NodeId) -> String {
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).unwrap();
        let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
        text.characters.clone()
    }

    fn editing(board: &BoardDoc) -> bool {
        board.session.ds().unwrap().text_edit.is_some()
    }

    fn tool(board: &BoardDoc) -> &'static str {
        board.session.tool.tool.id()
    }

    fn page_is_empty(board: &BoardDoc) -> bool {
        let page = board.session.page().unwrap();
        board.doc().unwrap().node(page).unwrap().children.is_empty()
    }

    /// A keystroke as `gpui_keystroke` writes it, split into its modifiers and its key. The
    /// modifiers must come in the order ctrl, alt, shift, cmd, or they end up in the key.
    fn split_keystroke(keystroke: &str) -> (Mods, &str) {
        let mut rest = keystroke;
        let mut take = |prefix: &str| match rest.strip_prefix(prefix) {
            Some(after) => {
                rest = after;
                true
            }
            None => false,
        };
        let (ctrl, alt, shift, cmd) = (take("ctrl-"), take("alt-"), take("shift-"), take("cmd-"));
        (
            Mods {
                shift,
                alt,
                cmd,
                ctrl,
            },
            rest,
        )
    }

    #[test]
    fn key_names_map_gpui_to_figcraft() {
        assert_eq!(key_name("backspace").as_deref(), Some("Backspace"));
        assert_eq!(key_name("left").as_deref(), Some("ArrowLeft"));
        assert_eq!(key_name("k").as_deref(), Some("k"));
        assert_eq!(key_name("f5").as_deref(), Some("F5"));
        assert_eq!(key_name("capslock"), None);
    }

    #[test]
    fn gpui_keystrokes_from_specs() {
        assert_eq!(
            gpui_keystroke("Shift+Cmd+K").as_deref(),
            Some("shift-cmd-k")
        );
        assert_eq!(gpui_keystroke("Cmd+\\").as_deref(), Some("cmd-\\"));
        assert_eq!(gpui_keystroke("Delete").as_deref(), Some("backspace"));
        assert_eq!(gpui_keystroke("Plus").as_deref(), Some("shift-="));
    }

    #[test]
    fn every_mac_shortcut_round_trips_through_gpui() {
        // A binding's GPUI keystroke, read back through `key_name`, is that binding's key again.
        for spec in KEYS.iter().flat_map(|b| b.mac) {
            let keystroke = gpui_keystroke(spec).unwrap_or_else(|| panic!("{spec}: no keystroke"));
            let (mods, key) = split_keystroke(&keystroke);
            let name = key_name(key).unwrap_or_else(|| panic!("{spec} -> {keystroke}: no name"));
            assert!(
                keymap::matches(spec, Platform::Mac, &name, mods),
                "{spec} -> {keystroke} -> {name} {mods:?}"
            );
        }
    }

    #[test]
    fn v_selects_move_tool() {
        let mut board = board();
        board.execute("tool.sticky", &json!({})).unwrap();
        assert_eq!(tool(&board), "sticky");
        assert_eq!(route_key(&mut board, "v", NONE), Ok(KeyOutcome::Handled));
        assert_eq!(tool(&board), "move");
    }

    #[test]
    fn undo_runs_through_the_keymap() {
        let mut board = board();
        sticky(&mut board, "");
        assert!(!page_is_empty(&board));
        assert_eq!(route_key(&mut board, "z", CMD), Ok(KeyOutcome::Handled));
        assert!(page_is_empty(&board));
    }

    #[test]
    fn hidden_commands_are_blocked() {
        let mut board = board();
        let before = tool(&board);
        // ⌘N is file.new, I is edit.pickColor (the eyedropper).
        assert_eq!(route_key(&mut board, "n", CMD), Ok(KeyOutcome::Unhandled));
        assert_eq!(route_key(&mut board, "i", NONE), Ok(KeyOutcome::Unhandled));
        assert_eq!(board.session.docs.len(), 1);
        assert_eq!(tool(&board), before);
    }

    #[test]
    fn ui_commands_are_returned() {
        let mut board = board();
        assert_eq!(
            route_key(&mut board, "s", CMD),
            Ok(KeyOutcome::Ui("file.save"))
        );
    }

    #[test]
    fn ui_requests_are_left_for_the_view() {
        let mut board = board();
        assert_eq!(route_key(&mut board, "k", CMD), Ok(KeyOutcome::Handled));
        let kinds: Vec<&str> = board
            .session
            .ui_requests
            .iter()
            .map(|r| r.kind.as_str())
            .collect();
        assert_eq!(kinds, ["palette.actions"]);
        assert_eq!(route_key(&mut board, "\\", CMD), Ok(KeyOutcome::Handled));
        assert!(board.session.ds().unwrap().view.ui_hidden);
    }

    #[test]
    fn typing_is_left_to_the_ime_while_editing() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        // Editing starts with all of "hi" selected.
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        assert_eq!(route_key(&mut board, "a", NONE), Ok(KeyOutcome::Unhandled));
        assert_eq!(
            route_key(&mut board, "Space", NONE),
            Ok(KeyOutcome::Unhandled)
        );
        assert_eq!(sticky_text(&board, note), "hi");
        // Backspace is the editor's: it deletes the selection, and the edit goes on.
        assert_eq!(
            route_key(&mut board, "Backspace", NONE),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(sticky_text(&board, note), "");
        assert!(editing(&board));
    }

    #[test]
    fn next_sticky_asks_to_edit_the_new_sticky() {
        let mut board = board();
        let first = sticky(&mut board, "");
        assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
        let shift_cmd = Mods { shift: true, ..CMD };
        assert_eq!(
            route_key(&mut board, "Enter", shift_cmd),
            Ok(KeyOutcome::Handled)
        );
        let selection = board.session.selection();
        let [next] = selection.as_slice() else {
            panic!("selection {selection:?}")
        };
        assert_ne!(*next, first);
        let edits: Vec<Option<NodeId>> = board
            .session
            .ui_requests
            .iter()
            .filter(|r| r.kind == "board.editText")
            .map(|r| figcraft_engine::params::id_of(&r.data["id"]))
            .collect();
        assert_eq!(edits, [Some(*next)]);
    }

    #[test]
    fn keys_without_a_shortcut_are_unhandled() {
        let mut board = board();
        assert_eq!(route_key(&mut board, "g", NONE), Ok(KeyOutcome::Unhandled));
        assert_eq!(
            route_key(&mut board, "NoSuchKey", NONE),
            Ok(KeyOutcome::Unhandled)
        );
    }
}
