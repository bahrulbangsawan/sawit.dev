//! A scrub of the Design panel: the values a field goes through while a label is dragged or a
//! colour picker is open, as previews within one interaction of the engine's, which ends as one
//! undo step or none; and the window's listeners that give a label's drag its moves and its end.

use figcraft_engine::{guard, textedit};
use gpui_kit::*;
use sawit_board::BoardError;
use serde_json::json;

use super::Scrub;
use crate::board::BoardView;

impl BoardView {
    /// Starts a scrub of field `key` on the selected layers: the engine opens an interaction
    /// (`Session::begin_interaction`), and the values [`scrub_to`](Self::scrub_to) sets until
    /// [`scrub_end`](Self::scrub_end) are previews within it, which end as one undo step or none.
    /// A scrub still on ends first, kept, rather than take this one into its undo step. The
    /// engine's refusal (a version on show in place of the document) shows as a toast, and no
    /// scrub starts.
    ///
    /// The engine has one interaction at a time, and keeps one that is open when another begins,
    /// so a scrub opens only an interaction of its own (final review C1): its cancel would
    /// otherwise put the document back to where that other gesture began, and its commit would
    /// end that gesture early.
    /// - A text being edited on the canvas holds one for the whole edit. The edit ends first, as
    ///   ⌘Z ends it (`sawit_board::input`): what was typed is an undo step of its own, which the
    ///   scrub's Escape doesn't take back. A text left empty goes then, as the engine ends any edit.
    /// - Any other gesture still open (a pen's path being drawn) is the canvas's: no scrub starts.
    ///   A colour picker's colours then run as plain edits, which join that gesture's undo step.
    /// - With no layer selected, no scrub starts either.
    pub fn scrub_begin(&mut self, key: &'static str, cx: &mut Context<Self>) {
        if self.scrub.is_some() {
            self.scrub_end(true, cx);
        }
        if self.editing_text() {
            self.run(
                |board| {
                    let session = &mut board.session;
                    guard::run("ui.textEnd", || textedit::end(session)).map_err(BoardError::from)
                },
                cx,
            );
        }
        if let Some(gesture) = self.open_gesture() {
            log::debug!("no scrub of {key}: the canvas's gesture \"{gesture}\" is open");
            return;
        }
        let Some(sel) = self.design_selection().filter(|sel| !sel.ids.is_empty()) else {
            return;
        };
        let begun = self.run(
            |board| {
                let session = &mut board.session;
                guard::run("ui.scrub", || session.begin_interaction(key)).map_err(BoardError::from)
            },
            cx,
        );
        if begun.is_some() {
            self.scrub = Some(Scrub {
                key,
                sel,
                drag: None,
            });
        }
    }

    /// Sets field `key` to `value` on the layers of the scrub that is on: a `node.set` within its
    /// interaction, which the canvas shows at once and which makes no undo step of its own. Without
    /// a scrub on it does nothing.
    pub fn scrub_to(&mut self, key: &'static str, value: f64, cx: &mut Context<Self>) {
        let Some(scrub) = &self.scrub else {
            return;
        };
        let edit = scrub.sel.set(key, json!(value));
        self.apply_edit(edit, cx);
    }

    /// Ends the scrub that is on. With `commit`, what it set stays, as one undo step
    /// (`Session::commit_interaction`; none when nothing changed). Without, the layers go back to
    /// how they were when it began, and no undo step is made (`Session::cancel_interaction`).
    pub fn scrub_end(&mut self, commit: bool, cx: &mut Context<Self>) {
        // A colour picker's scrub ends with any other.
        self.design.colour = None;
        if self.scrub.take().is_none() {
            return;
        }
        self.run(
            |board| {
                let session = &mut board.session;
                guard::run("ui.scrub", || {
                    if commit {
                        session.commit_interaction()
                    } else {
                        session.cancel_interaction()
                    }
                })
                .map_err(BoardError::from)
            },
            cx,
        );
    }

    /// Ends the scrub that is on, kept, whatever it scrubs: a colour picker's closes its picker
    /// ([`Self::end_colour_scrub`]), a label's drag scrubs nothing more until its release. Without
    /// a scrub on it does nothing.
    pub(in crate::board) fn end_scrub(&mut self, cx: &mut Context<Self>) {
        if self.design.colour.is_some() {
            self.end_colour_scrub(cx);
        } else if self.scrub.is_some() {
            self.scrub_end(true, cx);
        }
    }

    /// The label of the engine's interaction that is open, if one is: a gesture on the canvas, or
    /// a scrub.
    pub(super) fn open_gesture(&self) -> Option<String> {
        let ds = self.board()?.session.ds().ok()?;
        ds.interaction.as_ref().map(|gesture| gesture.label.clone())
    }

    /// Whether a label's drag is scrubbing a field: it takes the keys until it ends
    /// ([`BoardView::on_key`]).
    pub(in crate::board) fn scrubbing_label(&self) -> bool {
        self.scrub
            .as_ref()
            .is_some_and(|scrub| scrub.drag.is_some())
    }
}

/// Registers, for the frame being painted, the listeners on the whole window that give a label's
/// scrub its moves and its end wherever the pointer goes, as the canvas's drags get theirs
/// (`events::track_drags`, ruling P5-C14): a move changes the value, and the left button's release
/// ends the scrub, kept. A press, or a move without the button, means the release never came: it
/// ends the scrub too. They listen in the capture phase, before anything under the pointer can
/// stop the events.
pub(in crate::board) fn track_scrub(view: &WeakEntity<BoardView>, window: &mut Window) {
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view.update(cx, |view, cx| view.scrub_drag(event, cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                let _ = view.update(cx, |view, cx| view.end_label_scrub(cx));
            }
        }
    });
    window.on_mouse_event({
        let view = view.clone();
        move |_: &MouseDownEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture {
                let _ = view.update(cx, |view, cx| view.end_label_scrub(cx));
            }
        }
    });
}
