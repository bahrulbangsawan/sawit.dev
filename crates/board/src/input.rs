//! Keys and the pointer on the canvas.
//!
//! GPUI and FigCraft name keys differently, so [`key_name`] and [`gpui_keystroke`] translate
//! between them, and [`route_key`] sends a key where FigCraft's own keymap says, under Sawit's
//! command policy ([`crate::policy`]).
//!
//! The pointer goes to FigCraft's tools as page points ([`pointer_event`], [`pointer`]). What the
//! tools and commands ask of the view comes back as requests: [`handle_requests`] carries out the
//! ones that need no view, and hands back the rest.

use std::borrow::Cow;

use figcraft_doc::{NodeId, NodeKind};
use figcraft_engine::keymap::{self, Context, Platform};
use figcraft_engine::{
    PointerEvent, PointerKind, Session, UiRequest, ViewState, guard, textedit, vectoredit,
};
use serde_json::json;

use crate::policy::{is_clipboard, is_hidden, is_ui};
use crate::{BoardDoc, BoardError};

pub use figcraft_engine::keymap::Mods;

/// What became of a key sent to the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The canvas used it: a command ran, or a tool or the text editor took it.
    Handled,
    /// It is the shortcut of a command Sawit carries out itself ([`crate::policy::UI_COMMANDS`]),
    /// or of a copy or a paste, which Sawit carries out around the engine
    /// ([`crate::policy::CLIPBOARD`]). Nothing ran: carrying it out is the caller's job.
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
            "edit.cut" if vector_editing(&board.session) => cut_points(board)?,
            "edit.delete" if board.session.tool.pen.is_some() => {
                let session = &mut board.session;
                // As in FigCraft, a pen that can't finish leaves nothing to report.
                let _ = guard::run("ui.penFinish", || session.pen_finish());
            }
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
fn shortcut(platform: Platform, context: Context, key: &str, mods: Mods) -> Option<&'static str> {
    keymap::lookup(platform, context, key, mods)
        .map(|binding| binding.id)
        .filter(|id| !is_hidden(id))
}

/// Run command `id` for its shortcut, unless Sawit carries it out itself.
fn run(board: &mut BoardDoc, id: &'static str) -> Result<KeyOutcome, BoardError> {
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

/// The copy and paste commands that act on the text being edited: its selected characters are
/// copied or cut, and a paste types the clipboard's text, in FigCraft's text editor for every ⌘V
/// it takes (Shift held or not). Paste to replace (⇧⌘R) replaces layers, and waits.
const TEXT_CLIPBOARD: &[&str] = &["edit.copy", "edit.cut", "edit.paste", "edit.pasteOver"];

/// A key while text is edited, taken as FigCraft's own keyboard takes it: the shortcuts live while
/// editing, then typing (the input method's), then the text editor's keys. Canvas shortcuts wait
/// until the edit ends, but for copy and paste, which act on the text ([`TEXT_CLIPBOARD`]).
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
            // Copy and paste act on the text, which the caller does with the system clipboard.
            // Other canvas shortcuts (select all layers, duplicate, group…) wait until the edit
            // ends.
            _ if key.chars().count() == 1 => {
                let clipboard = shortcut(platform, Context::Canvas, key, mods)
                    .filter(|id| TEXT_CLIPBOARD.contains(id));
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

/// A pointer event of `kind` at `local`, a point of the canvas in logical px from its top-left,
/// with `mods` held: the page point under it in `view`, the canvas's own view, as the tools take
/// it. It has no pen pressure: as on FigCraft's own Mac canvas, strokes draw at their weight.
pub fn pointer_event(
    kind: PointerKind,
    local: (f64, f64),
    view: &ViewState,
    mods: Mods,
) -> PointerEvent {
    let page = view.screen_to_page(local.0, local.1);
    event(kind, page, local, view.zoom, mods)
}

/// A pointer event of `kind` at `page`, a point of the page, which is where `view` shows it on
/// the canvas: the event a control op names by its page point.
pub(crate) fn page_pointer_event(
    kind: PointerKind,
    page: (f64, f64),
    view: &ViewState,
    mods: Mods,
) -> PointerEvent {
    let local = view.page_to_screen(page.0, page.1);
    event(kind, page, local, view.zoom, mods)
}

/// A pointer event of `kind` at page point `page`, which is `local` on the canvas at `zoom`.
fn event(
    kind: PointerKind,
    page: (f64, f64),
    local: (f64, f64),
    zoom: f64,
    mods: Mods,
) -> PointerEvent {
    PointerEvent {
        kind,
        x: page.0,
        y: page.1,
        sx: local.0,
        sy: local.1,
        shift: mods.shift,
        alt: mods.alt,
        cmd: mods.cmd,
        ctrl: mods.ctrl,
        zoom,
        pressure: None,
    }
}

/// Sends `event` to the canvas's tools, and takes out of the session what they asked of the view
/// (`session.ui_requests`), in order, for [`handle_requests`] and then the view.
///
/// Errors are the engine's refusals, and its panics caught by [`guard::run`]. The requests made
/// before an error stay in the session.
pub fn pointer(board: &mut BoardDoc, event: &PointerEvent) -> Result<Vec<UiRequest>, BoardError> {
    let session = &mut board.session;
    guard::run("ui.pointer", || session.pointer(event))?;
    Ok(std::mem::take(&mut board.session.ui_requests))
}

/// The pointer left the canvas: what hovering there showed goes (what Alt measured, a vector
/// part's highlight), unless a gesture still holds it.
pub fn pointer_left(board: &mut BoardDoc) -> Result<(), BoardError> {
    let session = &mut board.session;
    guard::run("ui.pointerLeft", || {
        session.pointer_left();
        Ok(())
    })?;
    Ok(())
}

/// Carries out the requests in `reqs` that need no view, as FigCraft's own canvas does, and hands
/// back the others (panels, menus, dialogs), in order, for the view.
///
/// - `board.editText` starts typing in the text of the board object it names (`data.id`): the
///   new sticky of ⇧⌘↩, for one.
///
/// A request that fails leaves the engine's error in `session.message`, FigCraft's message for
/// the user, for the view to show.
pub fn handle_requests(board: &mut BoardDoc, reqs: &[UiRequest]) -> Vec<UiRequest> {
    let mut rest = Vec::new();
    for req in reqs {
        match req.kind.as_str() {
            "board.editText" => {
                let Some(id) = req.data.get("id").and_then(figcraft_engine::params::id_of) else {
                    continue;
                };
                let session = &mut board.session;
                let edited = guard::run("ui.editText", || {
                    figcraft_engine::board::edit_text(session, id)
                });
                if let Err(error) = edited {
                    board.session.message = Some(error.to_string());
                }
            }
            _ => rest.push(req.clone()),
        }
    }
    rest
}

/// The way a resize handle pulls, as the four two-headed arrows a cursor shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeDir {
    /// Left and right: 0°.
    Horizontal,
    /// Up and down: 90°.
    Vertical,
    /// From the top left to the bottom right: 45°, as the screen's y goes down.
    DiagDown,
    /// From the top right to the bottom left: 135°.
    DiagUp,
}

/// The arrow of [`ResizeDir`] nearest a resize along `angle_radians` (0 is horizontal and π/2
/// vertical, on the screen, where y goes down), whichever way along the line it points: the angle
/// counts modulo π. An angle that is no number is horizontal.
pub fn resize_dir(angle_radians: f64) -> ResizeDir {
    use std::f64::consts::{FRAC_PI_8, PI};
    // Each arrow takes the angles within π/8 of its own.
    let angle = angle_radians.rem_euclid(PI);
    if !(FRAC_PI_8..7.0 * FRAC_PI_8).contains(&angle) {
        ResizeDir::Horizontal
    } else if angle < 3.0 * FRAC_PI_8 {
        ResizeDir::DiagDown
    } else if angle < 5.0 * FRAC_PI_8 {
        ResizeDir::Vertical
    } else {
        ResizeDir::DiagUp
    }
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
    fn clipboard_keys_are_left_to_the_view() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        select(&mut board, note);
        let (revision, doc) = (board.revision(), board.doc().unwrap());
        let shift_cmd = Mods { shift: true, ..CMD };
        for (key, mods, id) in [
            ("c", CMD, "edit.copy"),
            ("x", CMD, "edit.cut"),
            ("v", CMD, "edit.paste"),
            ("v", shift_cmd, "edit.pasteOver"),
            ("r", shift_cmd, "edit.pasteReplace"),
        ] {
            assert_eq!(
                route_key(&mut board, key, mods),
                Ok(KeyOutcome::Ui(id)),
                "{id}"
            );
        }
        // Nothing ran: the view copies and pastes, with the system clipboard and the other tabs.
        assert!(board.session.clipboard.nodes.is_empty());
        assert_eq!(board.revision(), revision);
        assert_eq!(board.doc().unwrap(), doc);
    }

    #[test]
    fn clipboard_keys_copy_and_paste_the_text_being_edited() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        let shift_cmd = Mods { shift: true, ..CMD };
        // ⌘C, ⌘X and ⌘V copy, cut and paste the characters, as in any text field, and so does
        // ⇧⌘V, as in FigCraft's text editor.
        for (key, mods, id) in [
            ("c", CMD, "edit.copy"),
            ("x", CMD, "edit.cut"),
            ("v", CMD, "edit.paste"),
            ("v", shift_cmd, "edit.pasteOver"),
        ] {
            assert_eq!(
                route_key(&mut board, key, mods),
                Ok(KeyOutcome::Ui(id)),
                "{id}"
            );
        }
        // Pasting to replace the selected layers waits, like the other canvas shortcuts.
        assert_eq!(
            route_key(&mut board, "r", shift_cmd),
            Ok(KeyOutcome::Unhandled)
        );
        assert_eq!(sticky_text(&board, note), "hi");
        assert!(editing(&board));
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

    /// A new open path through three points in a row at height `y`: A (100, y), B (200, y) and
    /// C (300, y).
    fn three_points(board: &mut BoardDoc, y: i32) -> NodeId {
        let made = board
            .execute(
                "vector.create",
                &json!({"points": [[100, y], [200, y], [300, y]], "closed": false}),
            )
            .unwrap();
        figcraft_engine::params::id_of(&made["id"]).unwrap()
    }

    /// The path A-B-C ([`three_points`]) edited as a vector, with B selected, as in FigCraft's
    /// tests of the cut in vector edit mode (ui-egui vector_keys_tests.rs).
    fn abc(board: &mut BoardDoc) -> NodeId {
        let path = three_points(board, 100);
        board
            .execute("vector.edit.begin", &json!({"id": path}))
            .unwrap();
        board
            .execute("vector.edit.select", &json!({"vertices": [1]}))
            .unwrap();
        path
    }

    /// The points and segments of path `id`.
    fn network(board: &BoardDoc, id: NodeId) -> figcraft_doc::VectorNetwork {
        vectoredit::network(&board.doc().unwrap(), id)
            .unwrap()
            .clone()
    }

    #[test]
    fn cmd_x_in_vector_edit_mode_takes_the_selected_points_not_the_layer() {
        let mut board = board();
        let path = abc(&mut board);
        assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
        // B is gone and a heal joins A and C: the layer stays, still edited.
        assert!(board.doc().unwrap().node(path).is_some());
        let net = network(&board, path);
        assert_eq!(net.vertices.len(), 2, "{net:?}");
        assert_eq!(net.segments.len(), 1, "{net:?}");
        assert!(board.session.ds().unwrap().vector_edit.is_some());
        // Nothing was copied, so the view has nothing to put on the system clipboard.
        assert!(board.session.clipboard.nodes.is_empty());
    }

    #[test]
    fn cmd_x_in_vector_edit_mode_with_nothing_selected_keeps_the_layer() {
        let mut board = board();
        let path = abc(&mut board);
        board
            .execute("vector.edit.select", &json!({"vertices": []}))
            .unwrap();
        let doc = board.doc().unwrap();
        assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
        assert_eq!(network(&board, path).vertices.len(), 3);
        assert_eq!(board.doc().unwrap(), doc);
        assert!(board.session.clipboard.nodes.is_empty());
    }

    #[test]
    fn cmd_x_in_vector_edit_mode_takes_points_selected_in_another_edited_layer() {
        let mut board = board();
        let (a, b) = (three_points(&mut board, 100), three_points(&mut board, 200));
        vectoredit::begin_many(&mut board.session, &[a, b]).unwrap();
        // Points selected in the second layer, then the first made active: only the layer that
        // isn't active has a selection.
        board
            .execute("vector.edit.select", &json!({"id": b, "vertices": [1]}))
            .unwrap();
        board
            .execute("vector.edit.select", &json!({"id": a, "add": true}))
            .unwrap();
        assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
        let doc = board.doc().unwrap();
        assert!(doc.node(a).is_some() && doc.node(b).is_some());
        assert_eq!(network(&board, a).vertices.len(), 3);
        assert_eq!(network(&board, b).vertices.len(), 2);
    }

    #[test]
    fn cmd_x_in_vector_edit_mode_takes_the_selected_segments() {
        let mut board = board();
        let path = abc(&mut board);
        // A-B selected, and no point: the segment goes, and A with it, which nothing joins now.
        board
            .execute("vector.edit.select", &json!({"segments": [0]}))
            .unwrap();
        assert_eq!(route_key(&mut board, "x", CMD), Ok(KeyOutcome::Handled));
        assert!(board.doc().unwrap().node(path).is_some());
        let net = network(&board, path);
        assert_eq!(net.segments.len(), 1, "{net:?}");
        assert_eq!(net.vertices.len(), 2, "{net:?}");
        assert!(board.session.clipboard.nodes.is_empty());
    }

    #[test]
    fn cmd_x_outside_vector_edit_mode_cuts_the_layer_in_the_view() {
        let mut board = board();
        let path = three_points(&mut board, 100);
        select(&mut board, path);
        let doc = board.doc().unwrap();
        // The selected path itself is cut, by the view, with the system clipboard.
        assert_eq!(
            route_key(&mut board, "x", CMD),
            Ok(KeyOutcome::Ui("edit.cut"))
        );
        assert_eq!(board.doc().unwrap(), doc);
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

    /// A new, empty design file.
    fn design() -> BoardDoc {
        BoardDoc {
            session: Session::with_document(figcraft_doc::Document::new("Design")),
        }
    }

    /// A pointer event of `kind` at page point (`x`, `y`), at 100%.
    fn at(kind: PointerKind, x: f64, y: f64) -> PointerEvent {
        PointerEvent::at(kind, x, y, 1.0)
    }

    /// Sends `events` to the tools, one after another, and what they asked of the view.
    fn send(board: &mut BoardDoc, events: &[PointerEvent]) -> Vec<UiRequest> {
        let mut requests = Vec::new();
        for event in events {
            requests.extend(pointer(board, event).unwrap());
        }
        requests
    }

    /// The top-level layers of the page on show.
    fn layers(board: &BoardDoc) -> Vec<NodeId> {
        let page = board.session.page().unwrap();
        board.doc().unwrap().node(page).unwrap().children.clone()
    }

    /// The text layer of board object `id`.
    fn text_of(board: &BoardDoc, id: NodeId) -> NodeId {
        figcraft_engine::board::text_of(&board.doc().unwrap(), id).unwrap()
    }

    /// The request a command makes to start typing in board object `id`.
    fn edit_text_request(id: NodeId) -> UiRequest {
        UiRequest {
            kind: "board.editText".into(),
            data: json!({"id": id}),
        }
    }

    #[test]
    fn pointer_event_uses_page_coordinates() {
        let view = ViewState {
            cx: 100.0,
            cy: 50.0,
            zoom: 2.0,
            vw: 800.0,
            vh: 600.0,
            ..ViewState::default()
        };
        let event = pointer_event(PointerKind::Down, (400.0, 300.0), &view, SHIFT);
        assert_eq!((event.x, event.y), (100.0, 50.0));
        assert_eq!((event.sx, event.sy), (400.0, 300.0));
        assert_eq!(event.zoom, 2.0);
        assert_eq!(event.kind, PointerKind::Down);
        assert!(event.shift && !event.alt && !event.cmd && !event.ctrl);
    }

    #[test]
    fn rectangle_tool_draws_one_undo_step() {
        let mut board = design();
        board.execute("tool.rectangle", &json!({})).unwrap();
        // As FigCraft's own drags go (ruling P2-C4): the last move sizes the shape, and the
        // release commits it.
        send(
            &mut board,
            &[
                at(PointerKind::Down, 0.0, 0.0),
                at(PointerKind::Move, 50.0, 40.0),
                at(PointerKind::Move, 100.0, 80.0),
                at(PointerKind::Up, 100.0, 80.0),
            ],
        );
        let layers = layers(&board);
        assert_eq!(layers.len(), 1);
        let doc = board.doc().unwrap();
        let drawn = doc.node(layers[0]).unwrap();
        assert!(
            matches!(drawn.kind, NodeKind::Rectangle),
            "{:?}",
            drawn.kind
        );
        assert_eq!((drawn.width, drawn.height), (100.0, 80.0));
        assert_eq!(board.session.undo_depth(), (1, 0));
        board.execute("edit.undo", &json!({})).unwrap();
        assert!(page_is_empty(&board));
    }

    #[test]
    fn drag_released_outside_ends_gesture() {
        let mut board = design();
        board.execute("tool.rectangle", &json!({})).unwrap();
        // The release comes far off the canvas, where the window's listener sees it.
        send(
            &mut board,
            &[
                at(PointerKind::Down, 0.0, 0.0),
                at(PointerKind::Move, 50.0, 40.0),
                at(PointerKind::Move, 100.0, 80.0),
                at(PointerKind::Up, 5000.0, 5000.0),
            ],
        );
        assert_eq!(layers(&board).len(), 1);
        assert!(!board.session.tool.busy());
        assert_eq!(board.session.undo_depth(), (1, 0));
    }

    #[test]
    fn sticky_click_starts_text_editing() {
        let mut board = board();
        board.execute("tool.sticky", &json!({})).unwrap();
        let requests = send(
            &mut board,
            &[
                at(PointerKind::Down, 0.0, 0.0),
                at(PointerKind::Up, 0.0, 0.0),
            ],
        );
        handle_requests(&mut board, &requests);
        let [note] = layers(&board)[..] else {
            panic!("layers {:?}", layers(&board))
        };
        assert!(editing(&board));
        assert_eq!(text_edit(&board).id, text_of(&board, note));
    }

    #[test]
    fn next_sticky_keeps_typing() {
        let mut board = board();
        let first = sticky(&mut board, "");
        assert!(figcraft_engine::board::edit_text(&mut board.session, first).unwrap());
        let shift_cmd = Mods { shift: true, ..CMD };
        assert_eq!(
            route_key(&mut board, "Enter", shift_cmd),
            Ok(KeyOutcome::Handled)
        );
        let requests = std::mem::take(&mut board.session.ui_requests);
        let rest = handle_requests(&mut board, &requests);
        assert!(rest.is_empty(), "{rest:?}");
        let [one, next] = layers(&board)[..] else {
            panic!("layers {:?}", layers(&board))
        };
        assert_eq!(one, first);
        // Typing goes on in the new sticky.
        assert_eq!(text_edit(&board).id, text_of(&board, next));
        textedit::insert(&mut board.session, "more").unwrap();
        assert_eq!(sticky_text(&board, next), "more");
    }

    #[test]
    fn requests_for_the_view_are_handed_back_in_order() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        let ask = |kind: &str| UiRequest {
            kind: kind.into(),
            data: json!({}),
        };
        let requests = [
            ask("panel.layers"),
            edit_text_request(note),
            ask("palette.actions"),
        ];
        let rest = handle_requests(&mut board, &requests);
        assert_eq!(rest, [ask("panel.layers"), ask("palette.actions")]);
        assert_eq!(text_edit(&board).id, text_of(&board, note));
        assert!(board.session.message.is_none());
    }

    #[test]
    fn a_text_edit_that_cant_start_leaves_a_message_for_the_view() {
        let mut board = board();
        let note = sticky(&mut board, "hi");
        board
            .execute("file.saveVersion", &json!({"name": "First"}))
            .unwrap();
        // An earlier version on show can't be edited.
        board
            .execute("version.preview", &json!({"index": 0}))
            .unwrap();
        let rest = handle_requests(&mut board, &[edit_text_request(note)]);
        assert!(rest.is_empty(), "{rest:?}");
        assert!(!editing(&board));
        let message = board.session.message.clone().unwrap_or_default();
        assert!(message.contains("earlier version"), "{message:?}");
    }

    #[test]
    fn leaving_the_canvas_clears_what_hovering_showed() {
        let mut board = design();
        // Alt over a layer measures to it, outlining the box measured to.
        board.session.tool.overlay.measure_box = Some([0.0, 0.0, 10.0, 10.0]);
        pointer_left(&mut board).unwrap();
        assert_eq!(board.session.tool.overlay.measure_box, None);
    }

    #[test]
    fn resize_dir_buckets_angles() {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        for (angle, dir) in [
            (0.0, ResizeDir::Horizontal),
            (FRAC_PI_2, ResizeDir::Vertical),
            (FRAC_PI_4, ResizeDir::DiagDown),
            (3.0 * FRAC_PI_4, ResizeDir::DiagUp),
            (PI, ResizeDir::Horizontal),
            // Either way along the line, and the nearest arrow.
            (-FRAC_PI_4, ResizeDir::DiagUp),
            (-FRAC_PI_2, ResizeDir::Vertical),
            (0.3, ResizeDir::Horizontal),
            (1.0, ResizeDir::DiagDown),
            (f64::NAN, ResizeDir::Horizontal),
        ] {
            assert_eq!(resize_dir(angle), dir, "{angle}");
        }
    }

    /// A click of the tool chosen at page point (`x`, `y`): a press and its release.
    fn click(board: &mut BoardDoc, x: f64, y: f64) {
        send(
            board,
            &[at(PointerKind::Down, x, y), at(PointerKind::Up, x, y)],
        );
    }

    /// A new 100 × 80 rectangle at page point (0, 0), and only it selected.
    fn rectangle(board: &mut BoardDoc) -> NodeId {
        let made = board
            .execute(
                "shape.rectangle",
                &json!({"x": 0, "y": 0, "width": 100, "height": 80}),
            )
            .unwrap();
        let rect = figcraft_engine::params::id_of(&made["id"]).unwrap();
        select(board, rect);
        rect
    }

    /// As FigCraft's own keyboard finishes the pen's path on Delete (ui-egui shortcuts.rs
    /// `run_binding`), where the edit.delete the key is bound to would take the path being drawn,
    /// which is selected, off the canvas mid-way.
    #[test]
    fn delete_while_the_pen_draws_finishes_the_path() {
        for key in ["Backspace", "Delete"] {
            let mut board = design();
            board.execute("tool.pen", &json!({})).unwrap();
            click(&mut board, 0.0, 0.0);
            click(&mut board, 100.0, 0.0);
            // The path being drawn is a layer already, and selected.
            let [path] = layers(&board)[..] else {
                panic!("layers {:?}", layers(&board))
            };
            assert_eq!(board.session.selection(), [path], "{key}");
            assert!(board.session.tool.pen.is_some());
            assert_eq!(route_key(&mut board, key, NONE), Ok(KeyOutcome::Handled));
            // Finished, not deleted: the path stays with its 2 points, the Pen stays chosen,
            // and its next click starts another path.
            assert_eq!(layers(&board), [path], "{key}");
            assert_eq!(network(&board, path).vertices.len(), 2, "{key}");
            assert!(board.session.tool.pen.is_none(), "{key}");
            assert_eq!(tool(&board), "pen");
            // The path is one undo step.
            assert_eq!(board.session.undo_depth(), (1, 0), "{key}");
        }
    }

    #[test]
    fn delete_after_the_pens_first_point_deletes_nothing() {
        let mut board = design();
        let rect = rectangle(&mut board);
        board.execute("tool.pen", &json!({})).unwrap();
        // One point: nothing drawn yet, and the pen is under way.
        click(&mut board, 300.0, 300.0);
        assert!(board.session.tool.pen.is_some());
        assert_eq!(
            route_key(&mut board, "Backspace", NONE),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(layers(&board), [rect]);
        assert!(board.session.tool.pen.is_none());
        assert_eq!(tool(&board), "pen");
    }

    /// As FigCraft's own keyboard resizes with ⌘ and an arrow (ui-egui shortcuts.rs `key_event`;
    /// keymap.rs GESTURES "⌘ arrows"): right and down grow, left and up shrink, by the nudge, its
    /// top-left kept.
    #[test]
    fn cmd_and_an_arrow_resize_the_selection() {
        let mut board = design();
        let rect = rectangle(&mut board);
        let (small, big) = (
            board.session.prefs.nudge_small,
            board.session.prefs.nudge_big,
        );
        let size = |board: &BoardDoc| {
            let doc = board.doc().unwrap();
            let n = doc.node(rect).unwrap();
            (n.width, n.height)
        };
        let top_left = |board: &BoardDoc| {
            let bounds = board.doc().unwrap().world_bounds(rect).unwrap();
            (bounds.x0, bounds.y0)
        };
        assert_eq!(
            route_key(&mut board, "ArrowRight", CMD),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(size(&board), (100.0 + small, 80.0));
        assert_eq!(
            route_key(&mut board, "ArrowUp", CMD),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(size(&board), (100.0 + small, 80.0 - small));
        // With Shift, by the big nudge.
        let shift_cmd = Mods { shift: true, ..CMD };
        assert_eq!(
            route_key(&mut board, "ArrowDown", shift_cmd),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(size(&board), (100.0 + small, 80.0 - small + big));
        assert_eq!(top_left(&board), (0.0, 0.0));
        // Without ⌘ an arrow moves it, as before.
        assert_eq!(
            route_key(&mut board, "ArrowRight", NONE),
            Ok(KeyOutcome::Handled)
        );
        assert_eq!(top_left(&board), (small, 0.0));
        assert_eq!(size(&board), (100.0 + small, 80.0 - small + big));
        // With nothing selected, ⌘ and an arrow pan the view, as an arrow does.
        board.execute("select.none", &json!({})).unwrap();
        let (view, doc) = (
            board.session.ds().unwrap().view.clone(),
            board.doc().unwrap(),
        );
        assert_eq!(
            route_key(&mut board, "ArrowLeft", CMD),
            Ok(KeyOutcome::Handled)
        );
        assert_ne!(board.session.ds().unwrap().view, view);
        assert_eq!(board.doc().unwrap(), doc);
    }
}
