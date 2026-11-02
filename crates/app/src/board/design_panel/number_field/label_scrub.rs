//! A number field's label as a drag handle: dragging it sideways scrubs the value from the one the
//! layers share, by the field's step for each pixel, ten times as fast with ⇧ held.

use figcraft_doc::Document;
use figcraft_engine::guard;
use gpui_kit::*;
use sawit_board::design::{DesignSelection, Mixed, parse_number};
use serde_json::Value;

use super::super::Scrub;
use crate::board::BoardView;

/// A scrub by a label's drag: where the pointer was last, in the window, how far the drag has gone
/// since the press, in pixels (ten for each one moved with ⇧ held), the value it began from, and
/// the value it set last.
pub(in crate::board::design_panel) struct LabelDrag {
    last_x: f64,
    travel: f64,
    from: f64,
    value: f64,
}

/// How far a label's drag moves field `key` for each pixel: an opacity by 1%, any other number
/// by 1 (a rotation by a degree).
pub(super) fn step(key: &str) -> f64 {
    if key == "opacity" { 0.01 } else { 1.0 }
}

/// `value` as a label's drag leaves field `key`: rounded to a hundredth, as the field shows it,
/// and held in the field's range by the rule typed input has ([`parse_number`]), so that a size,
/// gap, padding or radius stops at 0, and an opacity at 0 and at 100%.
pub(super) fn scrubbed(key: &str, value: f64) -> f64 {
    let value = (value * 100.0).round() / 100.0;
    // A number below what the field takes is the only one `parse_number` refuses here.
    parse_number(key, &value.to_string()).unwrap_or(0.0)
}

/// A number that `value` holds: itself, or the largest of a list, as FigCraft's own panel starts a
/// scrub of corners that differ from the largest.
pub(super) fn number(value: &Value) -> Option<f64> {
    value.as_f64().or_else(|| {
        let items = value.as_array()?;
        items.iter().filter_map(Value::as_f64).reduce(f64::max)
    })
}

/// Where a scrub of field `key` on the layers of `sel` starts: the value they share, or, when
/// they differ, the value of the first of them (in `doc`) that has one. `None` when none has.
fn start_value(sel: &DesignSelection, doc: &Document, key: &str) -> Option<f64> {
    match sel.get(key) {
        Mixed::Same(value) => number(&value),
        Mixed::Mixed => {
            sel.ids
                .iter()
                .find_map(|id| match DesignSelection::read(doc, &[*id]).get(key) {
                    Mixed::Same(value) => number(&value),
                    Mixed::Absent | Mixed::Mixed => None,
                })
        }
        Mixed::Absent => None,
    }
}

impl BoardView {
    /// The left button went down on field `key`'s label, at `x` in the window: a scrub by the
    /// label's drag starts there, from the layers' value ([`start_value`]). A field being typed in
    /// is applied first, and the canvas takes the keyboard, so that the drag's Escape comes to it
    /// ([`BoardView::on_key`]).
    pub(super) fn begin_label_scrub(
        &mut self,
        key: &'static str,
        x: Pixels,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.commit_focused_field(window, cx);
        window.focus(&self.focus, cx);
        let Some(sel) = self.design_selection() else {
            return;
        };
        let Some(doc) = self.board().and_then(|board| board.doc().ok()) else {
            return;
        };
        let from = guard::run("ui.scrub", || Ok(start_value(&sel, &doc, key)));
        let Some(from) = from.ok().flatten() else {
            return;
        };
        self.scrub_begin(key, cx);
        if let Some(scrub) = &mut self.scrub {
            scrub.drag = Some(LabelDrag {
                last_x: f64::from(x),
                travel: 0.0,
                from,
                // What a move too small to count would set: no edit until the value changes.
                value: scrubbed(key, from),
            });
        }
    }

    /// The pointer moved, anywhere: while a label's drag scrubs a field, its value follows the
    /// pointer sideways, by the field's step for each pixel ([`step`]), ten times as fast while ⇧
    /// is held. A move without the left button means the drag's release never came: the scrub
    /// ends, kept.
    pub(in crate::board::design_panel) fn scrub_drag(
        &mut self,
        event: &MouseMoveEvent,
        cx: &mut Context<Self>,
    ) {
        let held = event.pressed_button == Some(MouseButton::Left);
        let Some(Scrub {
            key,
            drag: Some(drag),
            ..
        }) = &mut self.scrub
        else {
            return;
        };
        if !held {
            self.scrub_end(true, cx);
            return;
        }
        let x = f64::from(event.position.x);
        let rate = if event.modifiers.shift { 10.0 } else { 1.0 };
        drag.travel += (x - drag.last_x) * rate;
        drag.last_x = x;
        let value = scrubbed(key, drag.from + drag.travel.round() * step(key));
        if value == drag.value {
            return;
        }
        drag.value = value;
        let key = *key;
        self.scrub_to(key, value, cx);
    }

    /// The left button came up, or went down, anywhere: a label's scrub that is on ends, kept.
    pub(in crate::board::design_panel) fn end_label_scrub(&mut self, cx: &mut Context<Self>) {
        if self.scrubbing_label() {
            self.scrub_end(true, cx);
        }
    }
}
