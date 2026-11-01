//! The text being edited (item 6 of [`super::overlay`]): its box, its selected characters, an
//! input method's composition, underlined, and the caret; and the composition itself, laid out in
//! the document as it shows.

use std::sync::Arc;

use figcraft_doc::{Document, NodeId};
use figcraft_engine::{DocState, Preedit, Session, ViewState, guard, textedit};
use figcraft_geom::Point;
use figcraft_geom::kurbo;

use super::geometry::{alpha, node_quad, screen};
use super::{SELECTION, Shape};

/// What an input method composes in the text being edited: shown in the text, not yet in the
/// document. While it lasts, the canvas renders [`Composition::doc`] in place of the document, and
/// the overlay lays out the caret and the composition's underline in it, so both show the text as
/// it will read once committed.
pub struct Composition {
    /// The document as it shows: the composition in the edited text at its caret, laid out
    /// ([`textedit::preedit_doc`]).
    pub doc: Arc<Document>,
    /// The composition's characters in the edited text as it shows: from and to.
    pub range: (usize, usize),
    /// The input method's cursor: a character of the edited text as it shows.
    pub cursor: usize,
    /// What it shows: the document, the edited text, the composition and the caret it was made of.
    of: (Arc<Document>, NodeId, Preedit, usize),
}

impl Composition {
    /// The composition under way in the text `s` edits. `None` when nothing is composed.
    pub fn of(s: &Session) -> Option<Self> {
        let ds = s.ds().ok()?;
        let edit = ds.text_edit.as_ref()?;
        let preedit = edit.preedit.clone()?;
        let (doc, range, cursor) = textedit::preedit_doc(s)?;
        Some(Self {
            doc: Arc::new(doc),
            range,
            cursor,
            of: (ds.doc.clone(), edit.id, preedit, edit.caret),
        })
    }

    /// Keeps `kept` the composition under way in `s`: as it is while it still shows that one,
    /// made again once the document, the edited text, the composition or the caret has changed,
    /// and `None` once nothing is composed. Kept, its document stays the same one, so the render
    /// asked for it isn't asked for again. Made through [`guard::run`]: should that fail, `None`,
    /// and the canvas shows the document without the composition.
    pub fn refresh(kept: &mut Option<Self>, s: &Session) {
        if kept
            .as_ref()
            .is_some_and(|composition| composition.shows(s))
        {
            return;
        }
        *kept = guard::run("ui.preeditDoc", || Ok(Self::of(s)))
            .ok()
            .flatten();
    }

    /// Whether this is the composition under way in `s`.
    fn shows(&self, s: &Session) -> bool {
        let Ok(ds) = s.ds() else { return false };
        let Some(edit) = &ds.text_edit else {
            return false;
        };
        let (doc, id, preedit, caret) = &self.of;
        Arc::ptr_eq(doc, &ds.doc)
            && *id == edit.id
            && edit.preedit.as_ref() == Some(preedit)
            && *caret == edit.caret
    }
}

/// Item 6: while text is edited, the edited text as it shows, with the input method's composition
/// in it: its box, outlined; its selected characters, tinted 30 % with the selection colour; the
/// composition, underlined just below its baseline; and the caret, before the character it is at
/// (the input method's cursor while composing), which is painted beside its line
/// ([`caret_band`](super::caret_band)). The caret holds steady, so an idle canvas draws nothing.
/// Each is laid out in FigCraft's text layout and placed through the text's world transform and the
/// view (ui-egui `text_edit_overlay`).
pub(super) fn text_edit(
    out: &mut Vec<Shape>,
    s: &Session,
    ds: &DocState,
    composition: Option<&Composition>,
) {
    // The selected characters, from and to, as the text editor has them.
    let (Some(edit), Some((_, from, to))) = (&ds.text_edit, textedit::selected_range(s)) else {
        return;
    };
    let doc = composition.map_or(&*ds.doc, |composition| &*composition.doc);
    let Some(text) = TextShown::of(doc, edit.id, &ds.view) else {
        return;
    };
    if let Some(quad) = node_quad(doc, edit.id, &ds.view) {
        out.push(Shape::Outline {
            points: quad.to_vec(),
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        });
    }
    match composition {
        Some(composition) => underline(out, &text, composition.range),
        None if from < to => {
            for r in text.layout.selection_rects(from, to) {
                out.push(Shape::Fill {
                    points: vec![
                        text.at(r.x0, r.y0),
                        text.at(r.x1, r.y0),
                        text.at(r.x1, r.y1),
                        text.at(r.x0, r.y1),
                    ],
                    color: alpha(SELECTION, 0.3),
                });
            }
        }
        None => {}
    }
    let at = composition.map_or(edit.caret, |composition| composition.cursor);
    let (top, bottom) = text.caret(at);
    out.push(Shape::Caret {
        top,
        bottom,
        color: SELECTION,
    });
}

/// The composition's characters `from..to` underlined: on each line they are on, from where they
/// start to the right of the last of them there, just below the baseline.
fn underline(out: &mut Vec<Shape>, text: &TextShown, (from, to): (usize, usize)) {
    let layout = &text.layout;
    for r in layout.selection_rects(from, to) {
        let Some(line) = layout.lines.iter().find(|l| (l.top - r.y0).abs() < 1e-6) else {
            continue;
        };
        let glyphs = layout.glyphs.get(line.glyph_start..line.glyph_end);
        let right = (glyphs.unwrap_or_default().iter())
            .filter(|g| g.char_end > g.char_start && g.char_start >= from && g.char_end <= to)
            .map(|g| g.x + g.advance)
            .fold(r.x0, f64::max);
        let y = line.baseline + (line.top + line.height - line.baseline) * 0.35;
        out.push(Shape::Outline {
            points: vec![text.at(r.x0, y), text.at(right, y)],
            closed: false,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        });
    }
}

/// The caret before character `index` of the text `s` edits, as it shows with `composition` in
/// it (the composition under way, if any): its top and bottom on the canvas, in logical px from
/// its top-left. `None` when no text is edited. An input method shows its candidates there.
pub fn caret_at(
    s: &Session,
    composition: Option<&Composition>,
    index: usize,
) -> Option<((f64, f64), (f64, f64))> {
    let ds = s.ds().ok()?;
    let edit = ds.text_edit.as_ref()?;
    let doc = composition.map_or(&*ds.doc, |composition| &*composition.doc);
    Some(TextShown::of(doc, edit.id, &ds.view)?.caret(index))
}

/// A text layer laid out where it shows on the canvas (ui-egui's `TextOnScreen`).
struct TextShown<'a> {
    layout: Arc<figcraft_text::TextLayout>,
    /// How far down its box the layout starts: its vertical alignment.
    dy: f64,
    world: kurbo::Affine,
    view: &'a ViewState,
}

impl<'a> TextShown<'a> {
    /// Text layer `id` of `doc`, on a canvas showing it as `view` does. `None` for another layer.
    fn of(doc: &Document, id: NodeId, view: &'a ViewState) -> Option<Self> {
        let n = doc.node(id)?;
        let (layout, dy) = figcraft_text::layout_node(n)?;
        Some(Self {
            layout,
            dy,
            world: doc.world_transform(id),
            view,
        })
    }

    /// Layout point (`x`, `y`) on screen; on its path, for text on a path.
    fn at(&self, x: f64, y: f64) -> (f64, f64) {
        let p = self.layout.warp_point(x, y);
        screen(self.view, self.world * Point::new(p.x, p.y + self.dy))
    }

    /// The caret before character `index`: its top and bottom on screen. In a text with no line
    /// laid out, a line 14 units tall at its top-left corner.
    fn caret(&self, index: usize) -> ((f64, f64), (f64, f64)) {
        let (x, line) = self.layout.caret(index);
        match self.layout.lines.get(line).or(self.layout.lines.last()) {
            Some(line) => (self.at(x, line.top), self.at(x, line.top + line.height)),
            None => {
                let top = self.at(0.0, 0.0);
                (top, (top.0, top.1 + 14.0 * self.view.zoom))
            }
        }
    }
}
