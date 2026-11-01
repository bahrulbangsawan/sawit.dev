//! The pointer on the canvas: its events as the tools take them, a triple click, leaving the
//! canvas, the requests the tools make of the view, and the way a resize handle pulls.

use figcraft_engine::{PointerEvent, PointerKind, Session, UiRequest, ViewState, guard};
use figcraft_geom::Point;
use serde_json::json;

use super::Mods;
use crate::{BoardDoc, BoardError};

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

/// A third quick press of the button (or a later one of the same run of clicks) at `event`'s
/// point, taken as FigCraft's canvas takes a triple click (ui-egui `canvas/input.rs`, and
/// `triple_click_line` in `shortcuts.rs`), and what the tools then asked of the view, as
/// [`pointer`] gives it:
///
/// - Where nothing is edited it is a double-click: it edits, enters or opens as one does.
/// - While text is edited it is a press, which puts the caret where it is, and then the line it
///   is on is selected (`text.edit.selectLine`), when it is on the text.
/// - In vector edit mode it is a press, but on empty canvas a double-click, which leaves the
///   mode: the double-click that entered it counts on.
///
/// `event` is the press; its kind is set here. Errors are as [`pointer`]'s, and the engine's
/// refusal to select the line.
pub fn triple_click(
    board: &mut BoardDoc,
    event: &PointerEvent,
) -> Result<Vec<UiRequest>, BoardError> {
    let mut event = event.clone();
    let session = &board.session;
    let double = guard::run("ui.tripleClick", || Ok(triple_is_double(session, &event)))?;
    event.kind = if double {
        PointerKind::DoubleClick
    } else {
        PointerKind::Down
    };
    let requests = pointer(board, &event)?;
    if !double {
        select_line_at(board, &event)?;
    }
    Ok(requests)
}

/// Whether a triple click at `event`'s point is a double-click to the tools of `s`: unless a text
/// or a vector is edited, or in vector edit mode on empty canvas, clear of every edited layer
/// (while the pen goes on from no point).
fn triple_is_double(s: &Session, event: &PointerEvent) -> bool {
    let Ok(ds) = s.ds() else { return true };
    match (&ds.text_edit, &ds.vector_edit) {
        (None, None) => true,
        (None, Some(edit)) if edit.anchor.is_none() => {
            let p = Point::new(event.x, event.y);
            let reach = 4.0 * 6.0 / ds.view.zoom.max(1e-6);
            edit.parts().iter().all(|part| {
                ds.doc
                    .world_render_bounds(part.id)
                    .is_none_or(|b| !b.inflate(reach, reach).contains(p))
            })
        }
        _ => false,
    }
}

/// While text is edited and `event`'s point is on it, selects the line there, as a triple click
/// does in FigCraft.
fn select_line_at(board: &mut BoardDoc, event: &PointerEvent) -> Result<(), BoardError> {
    let Some(id) = board.session.ds()?.text_edit.as_ref().map(|edit| edit.id) else {
        return Ok(());
    };
    let p = Point::new(event.x, event.y);
    let session = &board.session;
    let index = guard::run("ui.textLine", || {
        let on_text = session
            .doc()?
            .world_bounds(id)
            .is_some_and(|b| b.contains(p));
        Ok(on_text.then(|| session.text_index_at(id, p).ok()).flatten())
    })?;
    if let Some(index) = index {
        board.execute("text.edit.selectLine", &json!({"index": index}))?;
    }
    Ok(())
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
