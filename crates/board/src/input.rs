//! Keys on the canvas. GPUI and FigCraft name keys differently, so [`key_name`] and
//! [`gpui_keystroke`] translate between them, and [`route_key`] sends a key where FigCraft's own
//! keymap says, under Sawit's command policy ([`crate::policy`]).

use std::borrow::Cow;

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

/// The US keyboard's digit row with Shift, each symbol and its digit. GPUI reports ⇧0 by the
/// character it types, `)`, with Shift off, and FigCraft's [`keymap::normalize`] reads that as a
/// key of its own: of these shifted digits it only knows `!`.
const SHIFTED_DIGITS: &[(&str, &str)] = &[
    (")", "0"),
    ("@", "2"),
    ("#", "3"),
    ("$", "4"),
    ("%", "5"),
    ("^", "6"),
    ("&", "7"),
    ("*", "8"),
    ("(", "9"),
];

/// `key` with `mods` as FigCraft's own keyboard names them: a letter in upper case, as vector edit
/// mode's keys expect (`A`), and a symbol from the US digit row as its digit with Shift (`)` is
/// ⇧0). Other keys stay as they are.
fn figcraft_key(key: &str, mut mods: Mods) -> (Cow<'_, str>, Mods) {
    if let Some(&(_, digit)) = SHIFTED_DIGITS.iter().find(|&&(symbol, _)| symbol == key) {
        mods.shift = true;
        return (Cow::Borrowed(digit), mods);
    }
    if matches!(key.as_bytes(), [b'a'..=b'z']) {
        return (Cow::Owned(key.to_ascii_uppercase()), mods);
    }
    (Cow::Borrowed(key), mods)
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
/// Where `Headless::key` and FigCraft's own keyboard (its egui UI) differ, this does as the
/// keyboard does:
/// - A letter is read in upper case, and a symbol from the US digit row as its digit with Shift,
///   since GPUI reports ⇧0 as `)`.
/// - While text is edited, only the shortcuts live while editing and the text editor's keys run.
///   ⌘Z and ⇧⌘Z end the edit, so it becomes one undo step, then undo or redo; ⌘A selects all the
///   text; ⌘↩ ends the edit; any other ⌘ with a character is [`KeyOutcome::Unhandled`].
/// - A number key with nothing selected is [`KeyOutcome::Unhandled`].
///
/// A key that is no shortcut and no key FigCraft knows is [`KeyOutcome::Unhandled`] too. What the
/// commands ask of the view stays in `session.ui_requests` for the caller. Errors are the engine's
/// refusals, and the engine's panics caught by [`guard::run`].
pub fn route_key(board: &mut BoardDoc, key: &str, mods: Mods) -> Result<KeyOutcome, BoardError> {
    let (key, mods) = figcraft_key(key, mods);
    let key: &str = &key;
    let platform = Platform::current();
    if board.session.ds().is_ok_and(|ds| ds.text_edit.is_some()) {
        return text_key(board, platform, key, mods);
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

/// A key while text is edited, taken as FigCraft's own keyboard takes it: the shortcuts live while
/// editing, then typing (the input method's), then the text editor's keys. Canvas shortcuts wait
/// until the edit ends.
fn text_key(
    board: &mut BoardDoc,
    platform: Platform,
    key: &str,
    mods: Mods,
) -> Result<KeyOutcome, BoardError> {
    if let Some(id) =
        shortcut(platform, Context::Text, key, mods).filter(|_| mods.cmd || mods.alt || mods.ctrl)
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
            // Canvas shortcuts (select all layers, duplicate, group…) wait until the edit ends.
            _ if key.chars().count() == 1 => return Ok(KeyOutcome::Unhandled),
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

/// End the text edit, which makes what was typed one undo step.
fn end_text_edit(board: &mut BoardDoc) -> Result<(), BoardError> {
    let session = &mut board.session;
    guard::run("ui.textEnd", || textedit::end(session))?;
    Ok(())
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
    use figcraft_doc::{ListStyle, NodeId};
    use figcraft_engine::keymap::{KEYS, Platform};
    use figcraft_engine::vectoredit::VTool;
    use serde_json::json;

    use super::*;

    const NONE: Mods = Mods {
        shift: false,
        alt: false,
        cmd: false,
        ctrl: false,
    };
    const CMD: Mods = Mods { cmd: true, ..NONE };
    const SHIFT: Mods = Mods {
        shift: true,
        ..NONE
    };

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

    /// The list style of sticky `id`'s first paragraph.
    fn list_style(board: &BoardDoc, id: NodeId) -> ListStyle {
        let doc = board.doc().unwrap();
        let text = figcraft_engine::board::text_of(&doc, id).unwrap();
        let text = doc.node(text).and_then(|n| n.kind.text()).unwrap();
        text.para_list(0).list
    }

    fn editing(board: &BoardDoc) -> bool {
        board.session.ds().unwrap().text_edit.is_some()
    }

    /// The text edit in progress.
    fn text_edit(board: &BoardDoc) -> figcraft_engine::TextEdit {
        board.session.ds().unwrap().text_edit.clone().unwrap()
    }

    /// The tool vector edit mode has chosen.
    fn vector_tool(board: &BoardDoc) -> VTool {
        board
            .session
            .ds()
            .unwrap()
            .vector_edit
            .as_ref()
            .unwrap()
            .tool
    }

    fn select(board: &mut BoardDoc, id: NodeId) {
        board.execute("select.set", &json!({"ids": [id]})).unwrap();
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

    #[test]
    fn vector_edit_keys_reach_vector_edit_mode() {
        let mut board = board();
        let made = board
            .execute(
                "shape.rectangle",
                &json!({"x": 0, "y": 0, "width": 100, "height": 100}),
            )
            .unwrap();
        let rect = figcraft_engine::params::id_of(&made["id"]).unwrap();
        select(&mut board, rect);
        let main_tool = tool(&board);
        // Enter on a shape edits it as a vector.
        assert_eq!(
            route_key(&mut board, "Enter", NONE),
            Ok(KeyOutcome::Handled)
        );
        assert!(board.session.ds().unwrap().vector_edit.is_some());
        // GPUI's lower-case letters pick vector edit mode's tools, not the canvas's (X is the
        // connector, Q the sticky, ⇧E the Design/Prototype switch).
        for (key, mods, picked) in [
            ("x", NONE, VTool::Cut),
            ("q", NONE, VTool::Lasso),
            ("b", NONE, VTool::Paint),
            ("p", NONE, VTool::Pen),
            ("e", SHIFT, VTool::Eraser),
            ("w", SHIFT, VTool::Width),
            ("v", NONE, VTool::Move),
        ] {
            assert_eq!(
                route_key(&mut board, key, mods),
                Ok(KeyOutcome::Handled),
                "{key}"
            );
            assert_eq!(vector_tool(&board), picked, "{key}");
        }
        assert_eq!(tool(&board), main_tool);
        // ⌘A selects the shape's four points, not every layer.
        assert_eq!(route_key(&mut board, "a", CMD), Ok(KeyOutcome::Handled));
        let edit = board.session.ds().unwrap().vector_edit.clone().unwrap();
        assert_eq!(edit.vertices.len(), 4);
        assert_eq!(board.session.selection(), [rect]);
    }

    #[test]
    fn cmd_a_selects_the_text_being_edited() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        // Right collapses the selection to a caret at the end.
        assert_eq!(
            route_key(&mut board, "ArrowRight", NONE),
            Ok(KeyOutcome::Handled)
        );
        let edit = text_edit(&board);
        assert_eq!((edit.anchor, edit.caret), (2, 2));
        let layers = board.session.selection();
        assert_eq!(route_key(&mut board, "a", CMD), Ok(KeyOutcome::Handled));
        let edit = text_edit(&board);
        assert_eq!((edit.anchor, edit.caret), (0, 2));
        assert_eq!(board.session.selection(), layers);
    }

    #[test]
    fn cmd_enter_ends_the_edit() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        assert_eq!(route_key(&mut board, "Enter", CMD), Ok(KeyOutcome::Handled));
        assert!(!editing(&board));
        // No newline replaced the selected text.
        assert_eq!(sticky_text(&board, note), "hi");
    }

    #[test]
    fn canvas_shortcuts_wait_while_editing() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        let doc = board.doc().unwrap();
        // ⌘D duplicates and ⌘G groups layers on the canvas.
        assert_eq!(route_key(&mut board, "d", CMD), Ok(KeyOutcome::Unhandled));
        assert_eq!(route_key(&mut board, "g", CMD), Ok(KeyOutcome::Unhandled));
        assert_eq!(board.doc().unwrap(), doc);
        assert!(editing(&board));
    }

    #[test]
    fn undo_while_editing_takes_back_the_typing() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        textedit::insert(&mut board.session, "x").unwrap();
        assert_eq!(sticky_text(&board, note), "x");
        assert_eq!(route_key(&mut board, "z", CMD), Ok(KeyOutcome::Handled));
        assert!(!editing(&board));
        // The edit was its own undo step: the sticky stays, as it read before.
        assert!(board.doc().unwrap().node(note).is_some());
        assert_eq!(sticky_text(&board, note), "hi");
        // ⇧⌘Z in a new edit ends that edit, then redoes the typing.
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        let shift_cmd = Mods { shift: true, ..CMD };
        assert_eq!(
            route_key(&mut board, "z", shift_cmd),
            Ok(KeyOutcome::Handled)
        );
        assert!(!editing(&board));
        assert_eq!(sticky_text(&board, note), "x");
    }

    #[test]
    fn number_keys_need_a_selection() {
        let mut board = board();
        let (revision, doc) = (board.revision(), board.doc().unwrap());
        // With nothing selected, object.opacityKeys would refuse.
        assert_eq!(route_key(&mut board, "5", NONE), Ok(KeyOutcome::Unhandled));
        assert_eq!(board.revision(), revision);
        assert_eq!(board.doc().unwrap(), doc);
        let note = sticky(&mut board, "");
        select(&mut board, note);
        assert_eq!(route_key(&mut board, "5", NONE), Ok(KeyOutcome::Handled));
        assert_eq!(board.doc().unwrap().node(note).unwrap().opacity, 0.5);
    }

    #[test]
    fn shifted_digits_from_a_us_keyboard() {
        let mut board = board();
        // GPUI sends ⇧0 as `)` with Shift off: view.zoom100.
        board.execute("view.set", &json!({"zoom": 2})).unwrap();
        assert_eq!(route_key(&mut board, ")", NONE), Ok(KeyOutcome::Handled));
        assert_eq!(board.session.ds().unwrap().view.zoom, 1.0);
        // ⇧2 zooms to the selection.
        let note = sticky(&mut board, "hi");
        select(&mut board, note);
        let view = board.session.ds().unwrap().view.clone();
        assert_eq!(route_key(&mut board, "@", NONE), Ok(KeyOutcome::Handled));
        assert_ne!(board.session.ds().unwrap().view, view);
        // While editing, ⇧⌘8 and ⇧⌘7 make lists, and ⇧0 is left to the input method to type.
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        assert_eq!(route_key(&mut board, "*", CMD), Ok(KeyOutcome::Handled));
        assert_eq!(list_style(&board, note), ListStyle::Bulleted);
        assert_eq!(route_key(&mut board, "&", CMD), Ok(KeyOutcome::Handled));
        assert_eq!(list_style(&board, note), ListStyle::Numbered);
        assert_eq!(route_key(&mut board, ")", NONE), Ok(KeyOutcome::Unhandled));
        assert_eq!(sticky_text(&board, note), "hi");
    }
}
