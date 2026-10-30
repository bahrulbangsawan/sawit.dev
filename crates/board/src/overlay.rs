//! What the canvas draws over the rendered page (spec D4): the selection's outlines, box, handles
//! and size, the hover outline, the text being edited (its box, its selected characters, an input
//! method's composition and the caret), the names of frames and sections, the marquee, snap guides
//! with their distances, where a drag drops, the previews of the pen, pencil, marker, highlighter,
//! connector and eraser, and vector edit mode's points and handles. [`overlay`] lists them back
//! to front as [`Shape`]s, in logical px from the canvas's top-left, from the engine's state; the
//! app paints the list as it is (`crates/app/src/board/overlay_paint.rs`) and only measures the
//! labels' text.
//!
//! A port of FigCraft's `ui-egui/src/canvas/overlays.rs`, with the frame names of
//! `frame_labels.rs` and the snap guides `measure_overlay.rs` draws from the tool's overlay,
//! limited to what the native canvas shows. Not drawn: layout grids, the pixel grid, constraint
//! guides, rulers and their guides, prototype connections, slot badges, the paint bucket's area,
//! shape builder regions, the lasso, the box measured to, the Scale tool's readout, arc and corner
//! radius handles, auto layout drop cells, swap targets, comments and the box around several
//! selected points; names are not turned with a rotated frame, and the caret doesn't blink.
//!
//! Every colour is a fixed canvas colour, the same on any page and in either theme ([`SELECTION`],
//! [`COMPONENT`], [`GUIDE`], white), except the names of frames and sections, whose grey reads on
//! the page they are on, and a stroke being drawn, which shows in the colour it will have. The
//! caret and the composition's underline, which FigCraft draws in its theme's text colour, are
//! [`SELECTION`], as the text's box is. So the caret is painted wider than FigCraft's, beside its
//! line ([`caret_band`]), to show where it meets the box's outline.

use std::sync::Arc;

use figcraft_doc::{Document, NodeId, NodeKind};
use figcraft_engine::tools::{self, Overlay, PENCIL_STROKE};
use figcraft_engine::vectoredit::{self, EditOverlay, VHit, VTool};
use figcraft_engine::{
    DocState, Preedit, Session, ToolId, ViewState, board, components, guard, textedit,
};
use figcraft_geom::Point;
use figcraft_geom::kurbo::{self, CubicBez, PathEl};

use crate::view;

/// An sRGB colour with straight (not premultiplied) alpha.
pub type Rgba = [u8; 4];

/// The selection: its outlines, box, handles and size, the hover outline, the marquee, where a drag
/// drops and the pen's previews.
pub const SELECTION: Rgba = [0x0D, 0x99, 0xFF, 0xFF];
/// Components, component sets and instances, and every layer in them, in place of [`SELECTION`].
pub const COMPONENT: Rgba = [0x97, 0x47, 0xFF, 0xFF];
/// Snap guides and measurements, and their distances.
pub const GUIDE: Rgba = [0xF2, 0x48, 0x22, 0xFF];

/// The size of the labels' text, and the height of their line (logical px).
pub const LABEL_TEXT: f64 = 11.0;
pub const LABEL_LINE: f64 = 14.0;

/// How wide the caret is painted (logical px): wider than the 1 px outline of the text's box,
/// which it meets at the start of a line ([`caret_band`]).
pub const CARET_WIDTH: f64 = 2.0;

const WHITE: Rgba = [0xFF; 4];

/// The room around a section's name inside its pill, and around a readout inside its pill (left
/// and right, top and bottom).
const NAME_PAD: (f64, f64) = (8.0, 4.0);
const READOUT_PAD: (f64, f64) = (4.0, 2.0);
/// How far above a frame its name's line ends.
const NAME_GAP: f64 = 4.0;
/// The least width a name has, however narrow its frame (an ellipsis at least), and the most a
/// section's has (ui-egui `frame_labels.rs`).
const NAME_MIN: f64 = 20.0;
const SECTION_NAME_MAX: f64 = 480.0;
/// How far around the canvas a frame may be and still have its name drawn: names sit outside.
const NAME_REACH: f64 = 20.0;
/// How far below the selection's box its size label starts.
const SIZE_GAP: f64 = 9.0;

/// The side of the selection box's corner handles, and the size across of a connector's end
/// handle and of a pen point or vector point (larger under the pointer).
const CORNER: f64 = 8.0;
const END: f64 = 10.0;
const POINT: f64 = 7.0;
const POINT_HOVERED: f64 = 9.0;

/// The marker's and the highlighter's strokes: weight (page units) and opacity, as `board.ink`
/// makes them (FigCraft's `boards/board.rs` keeps these private).
const MARKER: (f64, f32) = (4.0, 1.0);
const HIGHLIGHTER: (f64, f32) = (20.0, 0.45);

/// The colours of names on a light page or a dark one: a frame's name, and an unselected section's
/// name and pill (FigCraft's secondary text, text and hover colours in its light and dark themes).
struct Names {
    frame: Rgba,
    section: Rgba,
    pill: Rgba,
}

const ON_LIGHT: Names = Names {
    frame: [0x6E, 0x6E, 0x6E, 0xFF],
    section: [0x1E, 0x1E, 0x1E, 0xFF],
    pill: [0xF5, 0xF5, 0xF5, 0xFF],
};
const ON_DARK: Names = Names {
    frame: [0xB3, 0xB3, 0xB3, 0xFF],
    section: WHITE,
    pill: [0x38, 0x38, 0x38, 0xFF],
};

/// One thing drawn over the canvas. Positions are logical px from the canvas's top-left.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// A line through `points`, back to the first when `closed`, `width` px wide.
    Outline {
        points: Vec<(f64, f64)>,
        closed: bool,
        color: Rgba,
        width: f64,
        dashed: bool,
    },
    /// The polygon through `points`, filled.
    Fill {
        points: Vec<(f64, f64)>,
        color: Rgba,
    },
    /// A handle `size` px across at `center`: a circle when `round`, else a square, with `fill`
    /// inside a 1 px `stroke`.
    Handle {
        center: (f64, f64),
        size: f64,
        round: bool,
        fill: Rgba,
        stroke: Rgba,
    },
    /// One line of text, [`LABEL_TEXT`] px on a [`LABEL_LINE`] px line: the name of the frame or
    /// section `node`, or a readout (the selection's size, a guide's distance) when `node` is
    /// `None`. With a `background` the text sits on a pill of that colour. [`Shape::label_box`]
    /// says where it goes once its text is measured, and [`name_room`] how wide a name may be.
    Label {
        at: (f64, f64),
        text: String,
        color: Rgba,
        background: Option<Rgba>,
        node: Option<NodeId>,
    },
    /// The text caret: its line, from `top` to `bottom`, before the character it is at. It is
    /// painted as the band [`caret_band`] gives, beside that line.
    Caret {
        top: (f64, f64),
        bottom: (f64, f64),
        color: Rgba,
    },
}

impl Shape {
    /// Where a label goes once its text is measured `text_width` px wide: the top-left corner and
    /// the size of its box. The box is the text's line, padded into a pill when the label has a
    /// background. A name starts at `at`, its box's top-left; a readout is centred on `at`. The
    /// text is centred in the box. `None` for every other shape.
    pub fn label_box(&self, text_width: f64) -> Option<((f64, f64), (f64, f64))> {
        let Shape::Label {
            at,
            background,
            node,
            ..
        } = self
        else {
            return None;
        };
        let (pad_x, pad_y) = match (background, node) {
            (None, _) => (0.0, 0.0),
            (Some(_), Some(_)) => NAME_PAD,
            (Some(_), None) => READOUT_PAD,
        };
        let size = (text_width.max(0.0) + 2.0 * pad_x, LABEL_LINE + 2.0 * pad_y);
        let top_left = match node {
            Some(_) => *at,
            None => (at.0 - size.0 / 2.0, at.1 - size.1 / 2.0),
        };
        Some((top_left, size))
    }
}

/// Where the caret whose line runs from `top` to `bottom` (a [`Shape::Caret`]) is painted, as a
/// polygon: the band [`CARET_WIDTH`] px wide from that line toward the text's right, as a text view
/// draws its insertion point. Painted on its line, a caret at the start of a line would vanish into
/// the outline of the text's box, which is the same colour on the same edge (ui-egui draws the
/// caret in its theme's text colour instead, which the canvas's fixed colours leave out). `None`
/// for a caret with no length.
pub fn caret_band(top: (f64, f64), bottom: (f64, f64)) -> Option<[(f64, f64); 4]> {
    let (dx, dy) = (bottom.0 - top.0, bottom.1 - top.1);
    let length = dx.hypot(dy);
    if !length.is_normal() {
        return None;
    }
    // A quarter turn from down the text's lines is the text's right, however it is turned.
    let (rx, ry) = (dy / length * CARET_WIDTH, -dx / length * CARET_WIDTH);
    Some([
        top,
        (top.0 + rx, top.1 + ry),
        (bottom.0 + rx, bottom.1 + ry),
        bottom,
    ])
}

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

/// What to draw over the canvas of `s`'s open document, back to front:
///
/// 1. the names of frames and sections (labels with a `node`);
/// 2. the outline of the layer under the pointer, when it isn't selected;
/// 3. the outline of each selected layer;
/// 4. the selection's box, outlined around several layers, and its 4 corner handles (a
///    connector's 2 round end handles instead);
/// 5. the selection's size, `"{w} × {h}"` under its box;
/// 6. while text is edited, in place of 3 to 5: the edited text's box, its selected characters,
///    the input method's composition, underlined, and the caret;
/// 7. the marquee;
/// 8. snap guides and measurements, with their distances;
/// 9. the frame a drag drops into, and the line where it goes in an auto layout;
/// 10. the pen's path, the stroke the pencil, marker or highlighter is drawing, the connector
///     being drawn and the eraser;
/// 11. in vector edit mode, the edited layers' segments, points and handles.
///
/// Item 6 lays out the text as it shows, with the composition under way in it
/// ([`Composition::of`]); [`overlay_with`] takes one already made.
pub fn overlay(s: &Session) -> Vec<Shape> {
    overlay_with(s, Composition::of(s).as_ref())
}

/// [`overlay`], with `composition` the composition under way in the text `s` edits, as the
/// canvas keeps it ([`Composition::refresh`]) and renders it.
pub fn overlay_with(s: &Session, composition: Option<&Composition>) -> Vec<Shape> {
    let Ok(ds) = s.ds() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    frame_names(&mut out, ds);
    hover(&mut out, s, ds);
    selection(&mut out, s, ds);
    text_edit(&mut out, s, ds, composition);
    let (ov, view) = (&s.tool.overlay, &ds.view);
    marquee(&mut out, ov, view);
    guides(&mut out, ov, view);
    drops(&mut out, ov, &ds.doc, view);
    previews(&mut out, s, ds);
    vector_edit(&mut out, s, ds);
    out
}

/// The page quad (top-left, top-right, bottom-right, bottom-left) under the box on screen whose
/// top-left corner is `top_left` and whose size is `size`: how a frame's name, measured where it
/// was painted, goes into the engine's `ToolState::labels`, which takes a press or hover there as
/// one on the frame.
pub fn label_quad(top_left: (f64, f64), size: (f64, f64), view: &ViewState) -> [Point; 4] {
    let (x, y) = top_left;
    let (w, h) = size;
    [(x, y), (x + w, y), (x + w, y + h), (x, y + h)].map(|(sx, sy)| {
        let (px, py) = view.screen_to_page(sx, sy);
        Point::new(px, py)
    })
}

/// How wide the name of the frame or section `id` may be on screen, as ui-egui cuts names
/// (`frame_labels.rs`): a frame's name, the frame's width there, at least 20 px; a section's, that
/// less its pill's padding, 20 to 480 px. A name stays upright on a turned frame, so the width is
/// that of the frame's box on screen. The app cuts a longer name with an ellipsis, so the name, and
/// where the engine takes a press on it, stay over the frame. `None` for a layer not in `doc`.
pub fn name_room(doc: &Document, id: NodeId, view: &ViewState) -> Option<f64> {
    let n = doc.node(id)?;
    let (x0, _, x1, _) = extent(&node_quad(doc, id, view)?);
    let width = Some(x1 - x0).filter(|w| w.is_finite()).unwrap_or(0.0);
    Some(if matches!(n.kind, NodeKind::Section { .. }) {
        (width - 2.0 * NAME_PAD.0).clamp(NAME_MIN, SECTION_NAME_MAX)
    } else {
        width.max(NAME_MIN)
    })
}

/// Item 1 of [`overlay`]: the names of the frames and sections on the page, and of those in its
/// sections, as ui-egui places them, upright: a frame's just above its top-left corner, in its
/// component colour, the selection colour while it is selected or under the pointer, else grey; a
/// section's on a pill inside its top-left corner, white on the selection colour while it is
/// selected. Board objects (stickies, shapes, stamps) are frames without a name shown. A frame off
/// the canvas has none drawn, and the focus view (Dev Mode) shows none. The app cuts each name to
/// its [`name_room`].
fn frame_names(out: &mut Vec<Shape>, ds: &DocState) {
    if ds.focused().is_some() {
        return;
    }
    let (doc, view) = (&ds.doc, &ds.view);
    let names = if dark_page(doc, ds.page) {
        &ON_DARK
    } else {
        &ON_LIGHT
    };
    for id in named(doc, ds.page) {
        let (Some(n), Some(quad)) = (doc.node(id), node_quad(doc, id, view)) else {
            continue;
        };
        let (x0, y0, x1, y1) = extent(&quad);
        let off = x1 < -NAME_REACH
            || y1 < -NAME_REACH
            || x0 > view.vw + NAME_REACH
            || y0 > view.vh + NAME_REACH;
        if off {
            continue;
        }
        let selected = ds.selection.contains(&id);
        let label = if matches!(n.kind, NodeKind::Section { .. }) {
            Shape::Label {
                at: (x0, y0),
                text: n.name.clone(),
                color: if selected { WHITE } else { names.section },
                background: Some(if selected { SELECTION } else { names.pill }),
                node: Some(id),
            }
        } else {
            let component = matches!(
                n.kind,
                NodeKind::Component { .. } | NodeKind::ComponentSet { .. }
            );
            let color = if component {
                COMPONENT
            } else if selected || ds.hover == Some(id) {
                SELECTION
            } else {
                names.frame
            };
            Shape::Label {
                at: (x0, y0 - NAME_GAP - LABEL_LINE),
                text: n.name.clone(),
                color,
                background: None,
                node: Some(id),
            }
        };
        out.push(label);
    }
}

/// The layers whose name the canvas shows, in the order they are drawn: the visible frames and
/// sections on `page`, and in its sections whose contents show, each section after what it holds
/// (ui-egui's `rulers::boards` with its `labelled` filter). Board objects are left out.
fn named(doc: &Document, page: NodeId) -> Vec<NodeId> {
    fn walk(doc: &Document, id: NodeId, depth: usize, out: &mut Vec<NodeId>) {
        let Some(n) = doc.node(id) else { return };
        for &child in &n.children {
            let Some(c) = doc.node(child).filter(|c| c.visible) else {
                continue;
            };
            if matches!(
                c.kind,
                NodeKind::Section {
                    contents_hidden: false
                }
            ) && depth < 32
            {
                walk(doc, child, depth + 1, out);
            }
            let framed = c.kind.is_frame_like() || matches!(c.kind, NodeKind::Section { .. });
            if framed && !c.is_board_atom() {
                out.push(child);
            }
        }
    }
    let mut out = Vec::new();
    walk(doc, page, 0, &mut out);
    out
}

/// Item 2: the layer under the pointer, outlined 1.5 px, unless it is selected: with the Move or
/// Scale tool, while the preference to highlight layers on hover is on, and not while measuring
/// (Alt).
fn hover(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let Some(id) = ds.hover else { return };
    let shown = !ds.selection.contains(&id)
        && matches!(s.tool.tool, ToolId::Move | ToolId::Scale)
        && s.prefs.highlight_on_hover
        && s.tool.overlay.measure_box.is_none();
    if shown {
        let color = outline_color(&ds.doc, id);
        outline(out, &ds.doc, id, &ds.view, color, 1.5);
    }
}

/// Items 3 to 5: the selection, unless text or a vector is being edited: each layer's outline, in
/// the component colour for all of them when one is in a component; the selection's box, outlined
/// around several layers only (one layer's own outline is its box); 4 square handles on its
/// corners, or 2 on a line's ends, or none for a locked layer, while measuring or when the box is
/// too small on screen to show them; a selected connector's 2 round end handles instead; and the
/// size label, `"{w} × {h}"` in whole pixels (a line's length) centred under the box, unless the
/// preference to show dimensions is off.
fn selection(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let (doc, view, sel) = (&ds.doc, &ds.view, &ds.selection);
    if sel.is_empty() || ds.text_edit.is_some() || ds.vector_edit.is_some() {
        return;
    }
    let color = if sel.iter().any(|id| outline_color(doc, *id) == COMPONENT) {
        COMPONENT
    } else {
        SELECTION
    };
    for id in sel {
        outline(out, doc, *id, view, color, 1.0);
    }
    // The Shape Builder reshapes by regions: no box.
    if s.tool.tool == ToolId::ShapeBuilder {
        return;
    }
    let (Some(corners), Some((_, width, height))) = (
        tools::selection_box(doc, sel),
        tools::selection_frame(doc, sel),
    ) else {
        return;
    };
    let line = match sel.as_slice() {
        [id] => doc
            .node(*id)
            .is_some_and(|n| matches!(n.kind, NodeKind::Line)),
        _ => false,
    };
    let ends = board::selected_connector_ends(doc, sel).map(|(_, ends)| ends);
    let measuring = s.tool.overlay.measure_box.is_some();
    let locked = sel.iter().any(|id| tools::locked_in(doc, *id));
    let handles = if measuring || s.tool.dev_mode || ends.is_some() || locked {
        0
    } else if line {
        2
    } else if tools::box_sides_px(&corners, view.zoom).1 < tools::SMALL_BOX_PX {
        0
    } else {
        4
    };
    let corners = corners.map(|p| screen(view, p));
    if sel.len() > 1 {
        out.push(Shape::Outline {
            points: corners.to_vec(),
            closed: true,
            color,
            width: 1.0,
            dashed: false,
        });
    }
    for &center in corners.iter().take(handles) {
        out.push(Shape::Handle {
            center,
            size: CORNER,
            round: false,
            fill: WHITE,
            stroke: color,
        });
    }
    for end in ends.into_iter().flatten().filter(|_| !s.tool.dev_mode) {
        out.push(Shape::Handle {
            center: screen(view, end),
            size: END,
            round: true,
            fill: WHITE,
            stroke: color,
        });
    }
    if !measuring && ends.is_none() && s.prefs.show_dimensions {
        let text = if line {
            whole(width).to_string()
        } else {
            format!("{} × {}", whole(width), whole(height))
        };
        let (_, _, _, bottom) = extent(&corners);
        let middle = corners.iter().map(|c| c.0).sum::<f64>() / 4.0;
        let half = LABEL_LINE / 2.0 + READOUT_PAD.1;
        out.push(Shape::Label {
            at: (middle, bottom + SIZE_GAP + half),
            text,
            color: WHITE,
            background: Some(color),
            node: None,
        });
    }
}

/// Item 6: while text is edited, the edited text as it shows, with the input method's
/// composition in it: its box, outlined; its selected characters, tinted 30 % with the selection
/// colour; the composition, underlined just below its baseline; and the caret, before the
/// character it is at (the input method's cursor while composing), which is painted beside its
/// line ([`caret_band`]). The caret holds steady, so an idle canvas draws nothing. Each is laid
/// out in FigCraft's text layout and placed through the text's world transform and the view
/// (ui-egui `text_edit_overlay`).
fn text_edit(out: &mut Vec<Shape>, s: &Session, ds: &DocState, composition: Option<&Composition>) {
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

/// Item 7: the marquee, tinted 10 % with the selection colour, and outlined.
fn marquee(out: &mut Vec<Shape>, ov: &Overlay, view: &ViewState) {
    let Some([x0, y0, x1, y1]) = ov.marquee else {
        return;
    };
    let points = rect(view, [x0, y0, x1, y1]);
    out.push(Shape::Fill {
        points: points.clone(),
        color: alpha(SELECTION, 0.1),
    });
    out.push(Shape::Outline {
        points,
        closed: true,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    });
}

/// Item 8: the snap guides and measurements of the gesture under way: lines, dashed extension lines
/// and a small cross on a point snapped onto, then each distance on a pill centred on its line.
fn guides(out: &mut Vec<Shape>, ov: &Overlay, view: &ViewState) {
    let mut distances = Vec::new();
    for g in &ov.guides {
        let (a, b) = (at(view, g.x0, g.y0), at(view, g.x1, g.y1));
        if g.x0 == g.x1 && g.y0 == g.y1 {
            for (dx, dy) in [(4.0, 4.0), (4.0, -4.0)] {
                out.push(Shape::Outline {
                    points: vec![(a.0 - dx, a.1 - dy), (a.0 + dx, a.1 + dy)],
                    closed: false,
                    color: GUIDE,
                    width: 1.5,
                    dashed: false,
                });
            }
        } else {
            out.push(Shape::Outline {
                points: vec![a, b],
                closed: false,
                color: GUIDE,
                width: 1.0,
                dashed: g.dashed,
            });
        }
        if let Some(text) = &g.label {
            distances.push(Shape::Label {
                at: ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                text: text.clone(),
                color: WHITE,
                background: Some(GUIDE),
                node: None,
            });
        }
    }
    out.extend(distances);
}

/// Item 9: where a drag drops: the frame or section it goes into, outlined (a component's slot in
/// the component colour), and the line where it goes in an auto layout, 2 px wide.
fn drops(out: &mut Vec<Shape>, ov: &Overlay, doc: &Document, view: &ViewState) {
    if let Some(id) = ov.drop_target
        && let Some(quad) = node_quad(doc, id, view)
    {
        let color = if components::is_slot(doc, id) {
            COMPONENT
        } else {
            SELECTION
        };
        out.push(Shape::Outline {
            points: quad.to_vec(),
            closed: true,
            color,
            width: 1.0,
            dashed: false,
        });
    }
    if let Some([x0, y0, x1, y1]) = ov.insert_line {
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 2.0,
            dashed: false,
        });
    }
}

/// Item 10: what a tool is making: the stroke the pencil, marker or highlighter is drawing, in its
/// colour and weight (at least 1 px); the pen's points, their handles and the segment a click would
/// add; the connector being drawn, or the end of one being dragged; and the eraser's reach.
fn previews(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let (ov, view) = (&s.tool.overlay, &ds.view);
    let points: Vec<(f64, f64)> = ov.pen.iter().map(|[x, y]| at(view, *x, *y)).collect();
    if !points.is_empty() {
        match stroke(s, &ds.doc) {
            Some((color, weight)) => out.push(Shape::Outline {
                points,
                closed: false,
                color,
                width: (weight * view.zoom).max(1.0),
                dashed: false,
            }),
            None => {
                for [x0, y0, x1, y1] in &ov.pen_handles {
                    let (vertex, end) = (at(view, *x0, *y0), at(view, *x1, *y1));
                    bezier_handle(out, vertex, end, POINT, WHITE);
                }
                for center in points {
                    out.push(point(center, POINT, WHITE));
                }
                if let Some([x0, y0, x1, y1]) = ov.rubber {
                    let (a, b) = (at(view, x0, y0), at(view, x1, y1));
                    // Along the last point's out handle, when it has one.
                    let points = match ov.rubber_handle {
                        Some([hx, hy]) => cubic([a, at(view, hx, hy), b, b]),
                        None => vec![a, b],
                    };
                    out.push(Shape::Outline {
                        points,
                        closed: false,
                        color: SELECTION,
                        width: 1.0,
                        dashed: false,
                    });
                }
            }
        }
    } else if let Some([x0, y0, x1, y1]) = ov.rubber
        && ds.vector_edit.is_none()
    {
        // From where the connector starts to the pointer. Vector edit mode dashes its own.
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 1.5,
            dashed: false,
        });
    }
    if let Some([x, y, r]) = ov.brush {
        let ring = circle(at(view, x, y), r * view.zoom);
        for (color, width) in [(WHITE, 3.0), (SELECTION, 1.5)] {
            out.push(Shape::Outline {
                points: ring.clone(),
                closed: true,
                color,
                width,
                dashed: false,
            });
        }
    }
}

/// The stroke the current tool draws, which it shows while drawing: its colour as the screen shows
/// it, and its weight in page units. `None` for the Pen, whose path is a layer as it is drawn.
fn stroke(s: &Session, doc: &Document) -> Option<(Rgba, f64)> {
    let (color, (weight, opacity)) = match s.tool.tool {
        ToolId::Pencil => (PENCIL_STROKE.0, (PENCIL_STROKE.1, 1.0)),
        ToolId::Marker => (s.tool.board.marker, MARKER),
        ToolId::Highlighter => (s.tool.board.highlighter, HIGHLIGHTER),
        _ => return None,
    };
    let [r, g, b, a] = color.to_rgba8();
    let [r, g, b] = view::screen_rgb(doc.color_profile, [r, g, b]);
    Some((alpha([r, g, b, a], opacity), weight))
}

/// Item 11: vector edit mode, each edited layer's segments (thicker when selected or under the
/// pointer) and, but for the Variable width tool, its Bézier handles, its points (filled when
/// selected, larger under the pointer), a ring around the point the pen goes on from, and the pen's
/// next segment dashed; the active layer last.
fn vector_edit(out: &mut Vec<Shape>, s: &Session, ds: &DocState) {
    let Some(ve) = &ds.vector_edit else { return };
    let (doc, view, ov) = (&ds.doc, &ds.view, &s.tool.overlay);
    let points = ve.tool != VTool::Width;
    let hover_on = |id: NodeId| {
        ov.vector_hover
            .filter(|_| ov.vector_hover_on.unwrap_or(ve.id) == id)
    };
    for part in &ve.others {
        if let Some(o) = vectoredit::overlay(doc, &ve.view(part)) {
            edit_overlay(out, &o, view, points, hover_on(part.id), None);
        }
    }
    if let Some(o) = vectoredit::overlay(doc, ve) {
        edit_overlay(out, &o, view, points, hover_on(ve.id), ov.rubber);
    }
}

/// One edited layer's segments, and with `points` its handles and points, `hover` being the part
/// under the pointer, and the pen's `rubber` band.
fn edit_overlay(
    out: &mut Vec<Shape>,
    o: &EditOverlay,
    view: &ViewState,
    points: bool,
    hover: Option<VHit>,
    rubber: Option<[f64; 4]>,
) {
    let hovered_segment = match hover {
        Some(VHit::Segment { seg, .. }) => Some(seg),
        _ => None,
    };
    for (i, segment) in o.segments.iter().enumerate() {
        let width = if o.selected_segments.contains(&i) {
            3.0
        } else if hovered_segment == Some(i) {
            2.5
        } else {
            1.5
        };
        out.push(Shape::Outline {
            points: cubic(segment.map(|[x, y]| at(view, x, y))),
            closed: false,
            color: SELECTION,
            width,
            dashed: false,
        });
    }
    if !points {
        return;
    }
    for h in &o.handles {
        let hovered = hover
            == Some(VHit::Handle {
                seg: h.seg,
                start: h.start,
            });
        let size = if hovered { POINT_HOVERED } else { POINT };
        let selected = o.selected_handles.contains(&(h.seg, h.start));
        let fill = if selected { SELECTION } else { WHITE };
        let [vx, vy] = h.vertex;
        let [hx, hy] = h.handle;
        bezier_handle(out, at(view, vx, vy), at(view, hx, hy), size, fill);
    }
    for (i, [x, y]) in o.vertices.iter().enumerate() {
        let center = at(view, *x, *y);
        let hovered = matches!(hover, Some(VHit::Vertex { index }) if index == i);
        let size = if hovered { POINT_HOVERED } else { POINT };
        let selected = o.selected_vertices.contains(&i);
        out.push(point(
            center,
            size,
            if selected { SELECTION } else { WHITE },
        ));
        if o.anchor == Some(i) {
            out.push(Shape::Outline {
                points: circle(center, 7.5),
                closed: true,
                color: SELECTION,
                width: 1.0,
                dashed: false,
            });
        }
    }
    if let Some([x0, y0, x1, y1]) = rubber {
        out.push(Shape::Outline {
            points: vec![at(view, x0, y0), at(view, x1, y1)],
            closed: false,
            color: SELECTION,
            width: 1.0,
            dashed: true,
        });
    }
}

/// A Bézier handle from its point `vertex` to its `end`: the line, and a dot `size` across filled
/// with `fill`.
fn bezier_handle(out: &mut Vec<Shape>, vertex: (f64, f64), end: (f64, f64), size: f64, fill: Rgba) {
    out.push(Shape::Outline {
        points: vec![vertex, end],
        closed: false,
        color: SELECTION,
        width: 1.0,
        dashed: false,
    });
    out.push(point(end, size, fill));
}

/// A round point `size` across at `center`, filled with `fill`.
fn point(center: (f64, f64), size: f64, fill: Rgba) -> Shape {
    Shape::Handle {
        center,
        size,
        round: true,
        fill,
        stroke: SELECTION,
    }
}

/// Layer `id`'s outline, `width` px: its shape for ellipses, vectors (connectors and ink among
/// them), polygons, stars and boolean groups, its box otherwise.
fn outline(
    out: &mut Vec<Shape>,
    doc: &Document,
    id: NodeId,
    view: &ViewState,
    color: Rgba,
    width: f64,
) {
    let lines = shape_lines(doc, id, view)
        .or_else(|| node_quad(doc, id, view).map(|quad| vec![(quad.to_vec(), true)]));
    for (points, closed) in lines.into_iter().flatten() {
        out.push(Shape::Outline {
            points,
            closed,
            color,
            width,
            dashed: false,
        });
    }
}

/// Points on screen, and whether the line through them closes.
type Line = (Vec<(f64, f64)>, bool);

/// The outline of an ellipse, vector, polygon, star or boolean group on screen, as lines to within
/// half a pixel. `None` for other layers.
fn shape_lines(doc: &Document, id: NodeId, view: &ViewState) -> Option<Vec<Line>> {
    let n = doc.node(id)?;
    let shaped = matches!(
        n.kind,
        NodeKind::Ellipse { .. }
            | NodeKind::Vector { .. }
            | NodeKind::Polygon { .. }
            | NodeKind::Star { .. }
            | NodeKind::BooleanOp { .. }
    );
    if !shaped {
        return None;
    }
    let (path, _) = figcraft_render::node_outline(doc, n)?;
    let world = doc.world_transform(id);
    let mut lines = Vec::new();
    let mut line: Vec<(f64, f64)> = Vec::new();
    kurbo::flatten(path.iter(), 0.5 / view.zoom.max(1e-6), |el| match el {
        PathEl::MoveTo(p) => {
            if line.len() > 1 {
                lines.push((std::mem::take(&mut line), false));
            }
            line.clear();
            line.push(screen(view, world * p));
        }
        PathEl::LineTo(p) => line.push(screen(view, world * p)),
        PathEl::ClosePath => {
            // The next line, if any, starts where this one did.
            let start = line.first().copied();
            if line.len() > 1 {
                lines.push((std::mem::take(&mut line), true));
            }
            line.clear();
            line.extend(start);
        }
        _ => {}
    });
    if line.len() > 1 {
        lines.push((line, false));
    }
    Some(lines)
}

/// The colour of a layer's outline: [`COMPONENT`] for a component set, and for a component, an
/// instance and every layer in one; [`SELECTION`] otherwise.
fn outline_color(doc: &Document, id: NodeId) -> Rgba {
    let set = matches!(
        doc.node(id).map(|n| &n.kind),
        Some(NodeKind::ComponentSet { .. })
    );
    let in_component = components::owning_component(doc, id).is_some()
        || components::owning_instance(doc, id).is_some();
    if set || in_component {
        COMPONENT
    } else {
        SELECTION
    }
}

/// Whether `page`'s colour is dark: names on it are set light.
fn dark_page(doc: &Document, page: NodeId) -> bool {
    matches!(
        doc.node(page).map(|n| &n.kind),
        Some(NodeKind::Page { background, .. }) if background.is_dark()
    )
}

/// Layer `id`'s box on screen: its corners (top-left, top-right, bottom-right, bottom-left) turned
/// and flipped with it.
fn node_quad(doc: &Document, id: NodeId, view: &ViewState) -> Option<[(f64, f64); 4]> {
    let n = doc.node(id)?;
    let world = doc.world_transform(id);
    let corners = [
        (0.0, 0.0),
        (n.width, 0.0),
        (n.width, n.height),
        (0.0, n.height),
    ];
    Some(corners.map(|(x, y)| screen(view, world * Point::new(x, y))))
}

/// The page rectangle `[x0, y0, x1, y1]`'s corners on screen (top-left, top-right, bottom-right,
/// bottom-left).
fn rect(view: &ViewState, [x0, y0, x1, y1]: [f64; 4]) -> Vec<(f64, f64)> {
    vec![
        at(view, x0, y0),
        at(view, x1, y0),
        at(view, x1, y1),
        at(view, x0, y1),
    ]
}

/// The smallest and largest x and y of `points`.
fn extent(points: &[(f64, f64)]) -> (f64, f64, f64, f64) {
    points.iter().fold(
        (
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    )
}

/// The cubic Bézier through screen points `[start, control, control, end]`, as a line to within a
/// quarter pixel.
fn cubic([a, b, c, d]: [(f64, f64); 4]) -> Vec<(f64, f64)> {
    flat(CubicBez::new(a, b, c, d))
}

/// A circle on screen around `center`, `radius` px, as a closed line to within a quarter pixel.
/// No points for a radius that isn't positive.
fn circle(center: (f64, f64), radius: f64) -> Vec<(f64, f64)> {
    if !(radius > 0.0 && radius.is_finite()) {
        return Vec::new();
    }
    flat(kurbo::Circle::new(center, radius))
}

/// The points of `shape`'s outline (one piece) to within a quarter pixel.
fn flat(shape: impl kurbo::Shape) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    kurbo::flatten(shape.path_elements(0.1), 0.25, |el| {
        if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el {
            points.push((p.x, p.y));
        }
    });
    points
}

fn screen(view: &ViewState, p: Point) -> (f64, f64) {
    view.page_to_screen(p.x, p.y)
}

fn at(view: &ViewState, x: f64, y: f64) -> (f64, f64) {
    view.page_to_screen(x, y)
}

/// `color` with its alpha scaled by `opacity`.
fn alpha([r, g, b, a]: Rgba, opacity: f32) -> Rgba {
    [r, g, b, (f32::from(a) * opacity).round() as u8]
}

/// `v` to the nearest whole number.
fn whole(v: f64) -> i64 {
    v.round() as i64
}

#[cfg(test)]
mod tests {
    use figcraft_doc::{Document, NodeId};
    use figcraft_engine::{PointerEvent, PointerKind, Session, ToolId, textedit};
    use serde_json::{Value, json};

    use super::*;
    use crate::doc::BoardDoc;

    /// Runs `command`, which makes a layer, and returns that layer.
    fn make(board: &mut BoardDoc, command: &str, params: Value) -> NodeId {
        let made = board.execute(command, &params).unwrap();
        NodeId(made["id"].as_u64().unwrap())
    }

    /// Shows `board` through a 1200 × 800 canvas at `zoom`, the page point (-100, -100) at the
    /// canvas's top-left.
    fn zoom(board: &mut BoardDoc, zoom: f64) {
        let (vw, vh) = (1200.0, 800.0);
        let view = json!({
            "zoom": zoom, "vw": vw, "vh": vh,
            "cx": -100.0 + vw / 2.0 / zoom, "cy": -100.0 + vh / 2.0 / zoom,
        });
        board.execute("view.set", &view).unwrap();
    }

    /// A design file with a 200 × 150 frame at (0, 0) and a 100 × 80 rectangle at (300, 0) on its
    /// page, nothing selected, at 100 %: the frame's top-left is at (100, 100) on screen and the
    /// rectangle's at (400, 100).
    struct Fixture {
        board: BoardDoc,
        frame: NodeId,
        rect: NodeId,
    }

    fn fixture() -> Fixture {
        let mut board = BoardDoc {
            session: Session::with_document(Document::new("Design")),
        };
        let frame = make(
            &mut board,
            "node.create",
            json!({"type": "frame", "name": "Frame", "x": 0, "y": 0, "width": 200, "height": 150}),
        );
        let rect = make(
            &mut board,
            "node.create",
            json!({
                "type": "rectangle", "name": "Box", "x": 300, "y": 0, "width": 100, "height": 80,
            }),
        );
        board.session.set_selection(Vec::new());
        zoom(&mut board, 1.0);
        Fixture { board, frame, rect }
    }

    fn select(board: &mut BoardDoc, ids: &[NodeId]) {
        board.session.set_selection(ids.to_vec());
    }

    fn hover(board: &mut BoardDoc, id: NodeId) {
        board.session.ds_mut().unwrap().hover = Some(id);
    }

    /// A pointer event at page point (`x`, `y`), as the canvas sends them.
    fn pointer(board: &mut BoardDoc, kind: PointerKind, x: f64, y: f64) {
        let zoom = board.session.ds().unwrap().view.zoom;
        let event = PointerEvent::at(kind, x, y, zoom);
        board.session.pointer(&event).unwrap();
    }

    fn outlines(shapes: &[Shape]) -> Vec<&Shape> {
        let outline = |s: &&Shape| matches!(s, Shape::Outline { .. });
        shapes.iter().filter(outline).collect()
    }

    /// Where the handles are, and whether each is round.
    fn handles(shapes: &[Shape]) -> Vec<((f64, f64), bool)> {
        let handle = |s: &Shape| match s {
            Shape::Handle { center, round, .. } => Some((*center, *round)),
            _ => None,
        };
        shapes.iter().filter_map(handle).collect()
    }

    /// The labels' text and the layer each names.
    fn labels(shapes: &[Shape]) -> Vec<(&str, Option<NodeId>)> {
        let mut labels = Vec::new();
        for shape in shapes {
            if let Shape::Label { text, node, .. } = shape {
                labels.push((text.as_str(), *node));
            }
        }
        labels
    }

    /// The readouts (labels naming no layer).
    fn readouts(shapes: &[Shape]) -> Vec<&Shape> {
        let readout = |s: &&Shape| matches!(s, Shape::Label { node: None, .. });
        shapes.iter().filter(readout).collect()
    }

    #[test]
    fn nothing_selected_draws_only_frame_labels() {
        let f = fixture();
        let shapes = overlay(&f.board.session);
        assert_eq!(labels(&shapes), [("Frame", Some(f.frame))]);
        assert_eq!(shapes.len(), 1, "{shapes:?}");
        // Its line ends 4 px above the frame's top-left corner.
        let Shape::Label { at, .. } = &shapes[0] else {
            unreachable!()
        };
        assert_eq!(*at, (100.0, 100.0 - 4.0 - LABEL_LINE));
    }

    #[test]
    fn one_selected_layer_has_box_and_four_handles() {
        let mut f = fixture();
        select(&mut f.board, &[f.rect]);
        let shapes = overlay(&f.board.session);
        let corners = [
            (400.0, 100.0),
            (500.0, 100.0),
            (500.0, 180.0),
            (400.0, 180.0),
        ];
        // Its own outline is the box: no second one around it.
        assert_eq!(
            outlines(&shapes),
            [&Shape::Outline {
                points: corners.to_vec(),
                closed: true,
                color: SELECTION,
                width: 1.0,
                dashed: false,
            }]
        );
        assert_eq!(handles(&shapes), corners.map(|c| (c, false)));
        // Twice the zoom, twice as far from the canvas's top-left and from one another.
        zoom(&mut f.board, 2.0);
        let shapes = overlay(&f.board.session);
        let doubled = corners.map(|(x, y)| ((2.0 * x, 2.0 * y), false));
        assert_eq!(handles(&shapes), doubled);
    }

    #[test]
    fn size_label_reads_w_by_h() {
        let mut f = fixture();
        select(&mut f.board, &[f.rect]);
        let shapes = overlay(&f.board.session);
        let size = readouts(&shapes);
        assert_eq!(size.len(), 1, "{shapes:?}");
        let Shape::Label {
            text, background, ..
        } = size[0]
        else {
            unreachable!()
        };
        assert_eq!((text.as_str(), *background), ("100 × 80", Some(SELECTION)));
        // Centred under the box, 9 px below it, whatever the text's width.
        let (top_left, (width, _)) = size[0].label_box(40.0).unwrap();
        assert_eq!((top_left.0 + width / 2.0, top_left.1), (450.0, 189.0));
        // In whole pixels.
        let odd = make(
            &mut f.board,
            "node.create",
            json!({"type": "rectangle", "x": 0, "y": 300, "width": 99.6, "height": 80.4}),
        );
        select(&mut f.board, &[odd]);
        let shapes = overlay(&f.board.session);
        assert!(labels(&shapes).contains(&("100 × 80", None)), "{shapes:?}");
    }

    #[test]
    fn hover_outline_only_when_not_selected() {
        let mut f = fixture();
        select(&mut f.board, &[f.rect]);
        hover(&mut f.board, f.rect);
        assert_eq!(outlines(&overlay(&f.board.session)).len(), 1);
        hover(&mut f.board, f.frame);
        let shapes = overlay(&f.board.session);
        let outlines = outlines(&shapes);
        assert_eq!(outlines.len(), 2, "{shapes:?}");
        // The frame's box, 1.5 px, under the selection.
        let frame = vec![
            (100.0, 100.0),
            (300.0, 100.0),
            (300.0, 250.0),
            (100.0, 250.0),
        ];
        assert_eq!(
            outlines[0],
            &Shape::Outline {
                points: frame,
                closed: true,
                color: SELECTION,
                width: 1.5,
                dashed: false,
            }
        );
        // Only the Move and Scale tools show it.
        f.board.session.set_tool(ToolId::Rectangle);
        hover(&mut f.board, f.frame);
        assert_eq!(outlines_of(&f.board), 1);
    }

    fn outlines_of(board: &BoardDoc) -> usize {
        outlines(&overlay(&board.session)).len()
    }

    #[test]
    fn marquee_draws_fill_and_outline() {
        let mut f = fixture();
        // Pressed on empty canvas and dragged, not let go.
        pointer(&mut f.board, PointerKind::Down, 500.0, 300.0);
        pointer(&mut f.board, PointerKind::Move, 600.0, 400.0);
        let shapes = overlay(&f.board.session);
        let fills: Vec<&Shape> = shapes
            .iter()
            .filter(|s| matches!(s, Shape::Fill { .. }))
            .collect();
        let corners = vec![
            (600.0, 400.0),
            (700.0, 400.0),
            (700.0, 500.0),
            (600.0, 500.0),
        ];
        let tint = [0x0D, 0x99, 0xFF, 26];
        assert_eq!(
            fills,
            [&Shape::Fill {
                points: corners.clone(),
                color: tint,
            }]
        );
        let outline = Shape::Outline {
            points: corners,
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        };
        assert_eq!(outlines(&shapes), [&outline]);
    }

    #[test]
    fn connector_has_round_end_handles() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        let a = make(&mut board, "board.shape", json!({"x": 0, "y": 0}));
        let b = make(&mut board, "board.shape", json!({"x": 400, "y": 0}));
        let line = make(&mut board, "board.connector", json!({"from": a, "to": b}));
        select(&mut board, &[line]);
        zoom(&mut board, 1.0);
        let shapes = overlay(&board.session);
        let doc = board.doc().unwrap();
        let (_, ends) = figcraft_engine::board::selected_connector_ends(&doc, &[line]).unwrap();
        let ends = ends.map(|p| ((p.x + 100.0, p.y + 100.0), true));
        assert_eq!(handles(&shapes), ends);
        // Its ends are all it has: no box, no size.
        assert!(readouts(&shapes).is_empty(), "{shapes:?}");
    }

    #[test]
    fn label_quad_round_trips_through_view() {
        let mut f = fixture();
        zoom(&mut f.board, 2.0);
        let view = &f.board.session.ds().unwrap().view;
        let quad = label_quad((100.0, 50.0), (60.0, 14.0), view);
        // At 200 % the page quad is half the size of the box on screen.
        assert_eq!((quad[1].x - quad[0].x, quad[3].y - quad[0].y), (30.0, 7.0));
        let back = quad.map(|p| view.page_to_screen(p.x, p.y));
        assert_eq!(
            back,
            [(100.0, 50.0), (160.0, 50.0), (160.0, 64.0), (100.0, 64.0)]
        );
    }

    #[test]
    fn several_layers_share_one_outlined_box() {
        let mut f = fixture();
        select(&mut f.board, &[f.frame, f.rect]);
        let shapes = overlay(&f.board.session);
        // The frame's and the rectangle's own outlines, then the box around both.
        let union = [
            (100.0, 100.0),
            (500.0, 100.0),
            (500.0, 250.0),
            (100.0, 250.0),
        ];
        let outlines = outlines(&shapes);
        assert_eq!(outlines.len(), 3, "{shapes:?}");
        assert_eq!(
            outlines[2],
            &Shape::Outline {
                points: union.to_vec(),
                closed: true,
                color: SELECTION,
                width: 1.0,
                dashed: false,
            }
        );
        assert_eq!(handles(&shapes), union.map(|c| (c, false)));
        assert!(labels(&shapes).contains(&("400 × 150", None)), "{shapes:?}");
    }

    #[test]
    fn locked_and_small_layers_get_no_handles() {
        let mut f = fixture();
        // Locked: selected in the layers panel, but not resized on the canvas.
        f.board
            .session
            .doc_mut()
            .unwrap()
            .get_mut(f.rect)
            .unwrap()
            .locked = true;
        select(&mut f.board, &[f.rect]);
        let shapes = overlay(&f.board.session);
        assert!(handles(&shapes).is_empty(), "{shapes:?}");
        assert_eq!(outlines(&shapes).len(), 1);
        // Smaller on screen than its handles (both sides under 16 px): its outline only.
        let small = make(
            &mut f.board,
            "node.create",
            json!({"type": "rectangle", "x": 0, "y": 300, "width": 10, "height": 10}),
        );
        select(&mut f.board, &[small]);
        assert!(handles(&overlay(&f.board.session)).is_empty());
        zoom(&mut f.board, 2.0);
        assert_eq!(handles(&overlay(&f.board.session)).len(), 4);
    }

    #[test]
    fn label_box_places_names_and_readouts() {
        let name = |background| Shape::Label {
            at: (100.0, 50.0),
            text: "Frame".into(),
            color: SELECTION,
            background,
            node: Some(NodeId(7)),
        };
        // A frame's name starts at `at`, one line tall; a section's is on a pill there.
        assert_eq!(
            name(None).label_box(40.0),
            Some(((100.0, 50.0), (40.0, LABEL_LINE)))
        );
        assert_eq!(
            name(Some(SELECTION)).label_box(40.0),
            Some(((100.0, 50.0), (56.0, LABEL_LINE + 8.0)))
        );
        // A readout's pill is centred on `at`.
        let readout = Shape::Label {
            at: (100.0, 50.0),
            text: "24".into(),
            color: WHITE,
            background: Some(GUIDE),
            node: None,
        };
        let height = LABEL_LINE + 4.0;
        assert_eq!(
            readout.label_box(40.0),
            Some(((76.0, 50.0 - height / 2.0), (48.0, height)))
        );
        let caret = Shape::Caret {
            top: (0.0, 0.0),
            bottom: (0.0, 10.0),
            color: SELECTION,
        };
        assert_eq!(caret.label_box(40.0), None);
    }

    #[test]
    fn guides_draw_lines_crosses_and_distances() {
        use figcraft_engine::snap::Guide;
        let mut f = fixture();
        let line = |x0, y0, x1, y1| Guide::new(Point::new(x0, y0), Point::new(x1, y1));
        f.board.session.tool.overlay.guides = vec![
            Guide {
                label: Some("24".into()),
                ..line(0.0, 200.0, 100.0, 200.0)
            },
            Guide {
                dashed: true,
                ..line(0.0, 0.0, 0.0, 50.0)
            },
            line(10.0, 10.0, 10.0, 10.0),
        ];
        let shapes = overlay(&f.board.session);
        let guide = |points: Vec<(f64, f64)>, width, dashed| Shape::Outline {
            points,
            closed: false,
            color: GUIDE,
            width,
            dashed,
        };
        let expected = [
            guide(vec![(100.0, 300.0), (200.0, 300.0)], 1.0, false),
            guide(vec![(100.0, 100.0), (100.0, 150.0)], 1.0, true),
            // A point snapped onto: a cross.
            guide(vec![(106.0, 106.0), (114.0, 114.0)], 1.5, false),
            guide(vec![(106.0, 114.0), (114.0, 106.0)], 1.5, false),
            // The distance over the middle of its line, after every line.
            Shape::Label {
                at: (150.0, 300.0),
                text: "24".into(),
                color: WHITE,
                background: Some(GUIDE),
                node: None,
            },
        ];
        // After the frame's name.
        assert_eq!(shapes[1..], expected);
    }

    #[test]
    fn ink_previews_its_stroke() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        zoom(&mut board, 2.0);
        board.session.set_tool(ToolId::Marker);
        pointer(&mut board, PointerKind::Down, 0.0, 0.0);
        pointer(&mut board, PointerKind::Move, 50.0, 10.0);
        pointer(&mut board, PointerKind::Move, 100.0, 0.0);
        let shapes = overlay(&board.session);
        let [r, g, b, a] = board.session.tool.board.marker.to_rgba8();
        assert_eq!(
            shapes,
            [Shape::Outline {
                points: vec![(200.0, 200.0), (300.0, 220.0), (400.0, 200.0)],
                closed: false,
                color: [r, g, b, a],
                width: 8.0,
                dashed: false,
            }]
        );
    }

    #[test]
    fn names_have_their_frames_width() {
        let mut f = fixture();
        let room = |board: &BoardDoc, id| {
            let ds = board.session.ds().unwrap();
            name_room(&ds.doc, id, &ds.view).unwrap()
        };
        let narrow = make(
            &mut f.board,
            "node.create",
            json!({"type": "frame", "x": 0, "y": 300, "width": 5, "height": 5}),
        );
        let section = make(
            &mut f.board,
            "node.create",
            json!({"type": "section", "x": 0, "y": 400, "width": 100, "height": 100}),
        );
        // A frame's name: the frame's width on screen.
        assert_eq!(room(&f.board, f.frame), 200.0);
        zoom(&mut f.board, 2.0);
        assert_eq!(room(&f.board, f.frame), 400.0);
        // However narrow the frame, room for an ellipsis.
        assert_eq!(room(&f.board, narrow), NAME_MIN);
        // A section's pill keeps its padding inside the section, and holds 480 px of name at most.
        assert_eq!(room(&f.board, section), 200.0 - 16.0);
        zoom(&mut f.board, 10.0);
        assert_eq!(room(&f.board, section), SECTION_NAME_MAX);
        // The name of the 200 × 150 frame turned a quarter stays upright: the width of the
        // frame's box on screen, 150 px, not its top edge's 200.
        zoom(&mut f.board, 1.0);
        let doc = f.board.session.doc_mut().unwrap();
        doc.get_mut(f.frame).unwrap().transform =
            kurbo::Affine::rotate(std::f64::consts::FRAC_PI_2);
        let turned = room(&f.board, f.frame);
        assert!((turned - 150.0).abs() < 1e-9, "{turned}");
    }

    /// As built and ratified (delegate ruling on the report's Decision 3): a selection with a
    /// component in it is drawn in the component colour throughout.
    #[test]
    fn a_component_colours_the_whole_selection() {
        let mut f = fixture();
        let component = make(
            &mut f.board,
            "node.create",
            json!({"type": "component", "x": 0, "y": 300, "width": 100, "height": 100}),
        );
        select(&mut f.board, &[f.rect, component]);
        let shapes = overlay(&f.board.session);
        let mut drawn = (0, 0, 0);
        for shape in &shapes {
            match shape {
                Shape::Outline { color, .. } => {
                    assert_eq!(*color, COMPONENT);
                    drawn.0 += 1;
                }
                Shape::Handle { stroke, .. } => {
                    assert_eq!(*stroke, COMPONENT);
                    drawn.1 += 1;
                }
                Shape::Label {
                    node: None,
                    background,
                    ..
                } => {
                    assert_eq!(*background, Some(COMPONENT));
                    drawn.2 += 1;
                }
                _ => {}
            }
        }
        // Both outlines and the box, 4 handles, the size.
        assert_eq!(drawn, (3, 4, 1), "{shapes:?}");
    }

    /// As built and ratified (delegate ruling on the report's Decision 4): one selected line reads
    /// its length, on its 2 end handles.
    #[test]
    fn a_line_reads_its_length() {
        let mut f = fixture();
        let line = make(
            &mut f.board,
            "node.create",
            json!({"type": "line", "x": 0, "y": 300, "width": 100}),
        );
        select(&mut f.board, &[line]);
        let shapes = overlay(&f.board.session);
        assert_eq!(readouts(&shapes).len(), 1, "{shapes:?}");
        assert!(labels(&shapes).contains(&("100", None)), "{shapes:?}");
        assert_eq!(
            handles(&shapes),
            [(100.0, 400.0), (200.0, 400.0)].map(|c| (c, false))
        );
    }

    #[test]
    fn frame_names_read_on_their_page() {
        let mut f = fixture();
        let color = |board: &BoardDoc| match &overlay(&board.session)[0] {
            Shape::Label { color, .. } => *color,
            other => panic!("{other:?}"),
        };
        assert_eq!(color(&f.board), ON_LIGHT.frame);
        hover(&mut f.board, f.frame);
        assert_eq!(color(&f.board), SELECTION);
        f.board.session.ds_mut().unwrap().hover = None;
        let page = f.board.session.page().unwrap();
        let doc = f.board.session.doc_mut().unwrap();
        if let NodeKind::Page { background, .. } = &mut doc.get_mut(page).unwrap().kind {
            *background = figcraft_color::Color::DARK_PAGE;
        }
        assert_eq!(color(&f.board), ON_DARK.frame);
        // A section's name is on a pill inside its top-left corner.
        let section = make(
            &mut f.board,
            "node.create",
            json!({
                "type": "section", "name": "Ideas", "x": 0, "y": 400, "width": 300, "height": 200,
            }),
        );
        select(&mut f.board, &[]);
        let shapes = overlay(&f.board.session);
        let pill = Shape::Label {
            at: (100.0, 500.0),
            text: "Ideas".into(),
            color: ON_DARK.section,
            background: Some(ON_DARK.pill),
            node: Some(section),
        };
        assert!(shapes.contains(&pill), "{shapes:?}");
    }

    /// The carets drawn: each one's top and bottom.
    fn carets(shapes: &[Shape]) -> Vec<((f64, f64), (f64, f64))> {
        let caret = |s: &Shape| match s {
            Shape::Caret { top, bottom, .. } => Some((*top, *bottom)),
            _ => None,
        };
        shapes.iter().filter_map(caret).collect()
    }

    fn fills(shapes: &[Shape]) -> Vec<&Shape> {
        let fill = |s: &&Shape| matches!(s, Shape::Fill { .. });
        shapes.iter().filter(fill).collect()
    }

    /// The lines drawn that don't close: their points.
    fn open_lines(shapes: &[Shape]) -> Vec<&[(f64, f64)]> {
        let mut lines = Vec::new();
        for shape in shapes {
            if let Shape::Outline {
                points,
                closed: false,
                ..
            } = shape
            {
                lines.push(points.as_slice());
            }
        }
        lines
    }

    /// Whether `a` and `b` are the same point on screen, but for rounding.
    fn near(a: (f64, f64), b: (f64, f64)) -> bool {
        (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
    }

    /// A text layer reading "hi" with its top-left corner at page point (100, 50) of the
    /// fixture's design file, (200, 150) on screen at 100 %, being edited: the caret before
    /// character `caret`, or all of it selected.
    fn editing_hi(f: &mut Fixture, caret: Option<usize>) -> NodeId {
        // Without fonts nothing is shaped: the system's are scanned first, so the layer is made
        // the size its letters are.
        figcraft_text::scan_system_fonts();
        let text = make(
            &mut f.board,
            "node.create",
            json!({"type": "text", "x": 100, "y": 50, "text": "hi"}),
        );
        textedit::begin(&mut f.board.session, text, caret, false).unwrap();
        text
    }

    /// What text layer `id` reads in the document.
    fn reads(board: &BoardDoc, id: NodeId) -> String {
        let doc = board.doc().unwrap();
        let text = doc.node(id).and_then(|n| n.kind.text()).unwrap();
        text.characters.clone()
    }

    #[test]
    fn caret_overlay_while_editing() {
        let mut board = BoardDoc::new_board("Canvas").unwrap();
        zoom(&mut board, 1.0);
        let note = make(
            &mut board,
            "board.sticky",
            json!({"x": 0, "y": 0, "text": "hi"}),
        );
        assert!(carets(&overlay(&board.session)).is_empty());
        // Typing in it (all of "hi" selected): one caret, in the selection colour.
        assert!(figcraft_engine::board::edit_text(&mut board.session, note).unwrap());
        let shapes = overlay(&board.session);
        assert_eq!(carets(&shapes).len(), 1, "{shapes:?}");
        let caret_color = |s: &Shape| match s {
            Shape::Caret { color, .. } => Some(*color),
            _ => None,
        };
        let colors: Vec<Rgba> = shapes.iter().filter_map(caret_color).collect();
        assert_eq!(colors, [SELECTION]);
        // The selection makes way: no handles, no size.
        assert!(handles(&shapes).is_empty(), "{shapes:?}");
        assert!(readouts(&shapes).is_empty(), "{shapes:?}");
        // Once the typing ends, the caret goes.
        textedit::end(&mut board.session).unwrap();
        assert!(carets(&overlay(&board.session)).is_empty());
    }

    #[test]
    fn caret_stands_before_its_character_on_screen() {
        let mut f = fixture();
        let text = editing_hi(&mut f, Some(0));
        let shapes = overlay(&f.board.session);
        let [(top, bottom)] = carets(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        // Before the first character: at the text's left side, from its top down one line.
        assert!(near(top, (200.0, 150.0)), "{top:?}");
        assert!((bottom.0 - 200.0).abs() < 1e-6, "{bottom:?}");
        let line = bottom.1 - top.1;
        let (width, height) = {
            let doc = f.board.doc().unwrap();
            let n = doc.node(text).unwrap();
            (n.width, n.height)
        };
        assert!(line > 0.0 && line <= height + 1e-6, "{line} in {height}");
        // The text's box is outlined, and nothing else is.
        let outline = Shape::Outline {
            points: vec![
                (200.0, 150.0),
                (200.0 + width, 150.0),
                (200.0 + width, 150.0 + height),
                (200.0, 150.0 + height),
            ],
            closed: true,
            color: SELECTION,
            width: 1.0,
            dashed: false,
        };
        assert_eq!(outlines(&shapes), [&outline]);
        // After the last character: further right, within the text's box.
        textedit::key(&mut f.board.session, "End", false, false, false).unwrap();
        let [(end, _)] = carets(&overlay(&f.board.session))[..] else {
            panic!()
        };
        assert!(
            end.0 > top.0 + 1.0 && end.0 <= 200.0 + width + 1e-6,
            "{end:?}"
        );
        assert!((end.1 - top.1).abs() < 1e-6, "{end:?}");
        // At twice the zoom: twice as far from the canvas's top-left, and twice as tall.
        textedit::key(&mut f.board.session, "Home", false, false, false).unwrap();
        zoom(&mut f.board, 2.0);
        let [(top, bottom)] = carets(&overlay(&f.board.session))[..] else {
            panic!()
        };
        assert!(near(top, (400.0, 300.0)), "{top:?}");
        assert!((bottom.1 - top.1 - 2.0 * line).abs() < 1e-6, "{bottom:?}");
    }

    /// A caret at the start of a line stands on the left edge of the text's box, where the box's
    /// outline is drawn in the same colour: painted on its line, it would vanish into the
    /// outline, as in a new sticky. It is painted as a band from its line into the text, wider
    /// than the outline.
    #[test]
    fn a_caret_at_a_line_start_shows_inside_the_outlined_box() {
        let mut f = fixture();
        editing_hi(&mut f, Some(0));
        let shapes = overlay(&f.board.session);
        let [(top, bottom)] = carets(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        // The caret's line is the box's left edge, which the outline covers.
        let [
            Shape::Outline {
                points,
                closed: true,
                width,
                ..
            },
        ] = outlines(&shapes)[..]
        else {
            panic!("{shapes:?}")
        };
        assert!(near(top, points[0]), "{top:?} {points:?}");
        assert!(
            (bottom.0 - points[0].0).abs() < 1e-6,
            "{bottom:?} {points:?}"
        );
        assert!(CARET_WIDTH > *width);
        // Painted, it is the band from that edge into the box.
        let band = caret_band(top, bottom).unwrap();
        let inside = [
            top,
            (top.0 + CARET_WIDTH, top.1),
            (bottom.0 + CARET_WIDTH, bottom.1),
            bottom,
        ];
        assert!(
            band.iter().zip(inside).all(|(a, b)| near(*a, b)),
            "{band:?}"
        );
    }

    /// A caret's band runs from its line toward the text's right, however the text is turned. A
    /// caret with no length isn't painted.
    #[test]
    fn a_caret_band_runs_toward_the_text() {
        // Upright: the text's right is the screen's.
        let band = caret_band((10.0, 0.0), (10.0, 20.0));
        assert_eq!(
            band,
            Some([(10.0, 0.0), (12.0, 0.0), (12.0, 20.0), (10.0, 20.0)])
        );
        // A quarter turn clockwise: down its lines is the screen's left, and the text's right is
        // the screen's down.
        let band = caret_band((10.0, 0.0), (-10.0, 0.0));
        assert_eq!(
            band,
            Some([(10.0, 0.0), (10.0, 2.0), (-10.0, 2.0), (-10.0, 0.0)])
        );
        assert_eq!(caret_band((5.0, 5.0), (5.0, 5.0)), None);
    }

    #[test]
    fn selected_text_is_tinted() {
        let mut f = fixture();
        // All of "hi" selected: the caret after it, the selection back to its start.
        editing_hi(&mut f, None);
        let shapes = overlay(&f.board.session);
        let [(top, bottom)] = carets(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        let [Shape::Fill { points, color }] = fills(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        // The selection colour at 30 %, from the line's start to the caret, a line high.
        assert_eq!(*color, [0x0D, 0x99, 0xFF, 77]);
        let tinted = [
            (200.0, top.1),
            (top.0, top.1),
            (top.0, bottom.1),
            (200.0, bottom.1),
        ];
        assert_eq!(points.len(), 4, "{points:?}");
        for (point, corner) in points.iter().zip(tinted) {
            assert!(near(*point, corner), "{points:?} != {tinted:?}");
        }
        // Collapsed to a caret, nothing is tinted.
        textedit::key(&mut f.board.session, "ArrowRight", false, false, false).unwrap();
        let shapes = overlay(&f.board.session);
        assert!(fills(&shapes).is_empty(), "{shapes:?}");
        assert_eq!(carets(&shapes).len(), 1);
    }

    #[test]
    fn composition_is_underlined_where_it_shows() {
        let mut f = fixture();
        let text = editing_hi(&mut f, Some(2));
        let shapes = overlay(&f.board.session);
        let [(before, _)] = carets(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        let narrow = outlines(&shapes)[0].clone();
        textedit::preedit(&mut f.board.session, "ni", None).unwrap();
        let shapes = overlay(&f.board.session);
        // The caret is the input method's, at the composition's end, which shows after "hi"; the
        // composition is underlined from where the caret was to there.
        let [(top, bottom)] = carets(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        assert!(top.0 > before.0 + 1.0, "{top:?} {before:?}");
        let [&[start, end]] = open_lines(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        assert!((start.0 - before.0).abs() < 1e-6, "{start:?} {before:?}");
        assert!((end.0 - top.0).abs() < 1e-6, "{end:?} {top:?}");
        // Level, below the baseline, inside the line.
        assert!((start.1 - end.1).abs() < 1e-9, "{start:?} {end:?}");
        assert!(start.1 > top.1 && start.1 < bottom.1, "{start:?}");
        // Nothing is selected, and the box, drawn before the underline, grew with the
        // composition.
        assert!(fills(&shapes).is_empty(), "{shapes:?}");
        let [wide, _underline] = outlines(&shapes)[..] else {
            panic!("{shapes:?}")
        };
        let right = |s: &Shape| match s {
            Shape::Outline { points, .. } => points[1].0,
            _ => unreachable!(),
        };
        assert!(right(wide) > right(&narrow) + 1.0, "{wide:?} {narrow:?}");
        // The document doesn't have the composition.
        assert_eq!(reads(&f.board, text), "hi");
    }

    #[test]
    fn a_composition_is_made_again_only_when_it_changes() {
        let mut f = fixture();
        let text = editing_hi(&mut f, Some(2));
        let mut kept = None;
        Composition::refresh(&mut kept, &f.board.session);
        assert!(kept.is_none());
        textedit::preedit(&mut f.board.session, "ni", None).unwrap();
        Composition::refresh(&mut kept, &f.board.session);
        let first = kept.as_ref().expect("a composition");
        assert_eq!((first.range, first.cursor), ((2, 4), 4));
        let first = first.doc.clone();
        // Unchanged, it is the same document: the render asked for it stays the one asked for.
        Composition::refresh(&mut kept, &f.board.session);
        assert!(Arc::ptr_eq(&first, &kept.as_ref().unwrap().doc));
        // A longer composition, the input method's cursor in it: made again.
        textedit::preedit(&mut f.board.session, "nih", Some(1)).unwrap();
        Composition::refresh(&mut kept, &f.board.session);
        let grown = kept.as_ref().expect("a composition");
        assert!(!Arc::ptr_eq(&first, &grown.doc));
        assert_eq!((grown.range, grown.cursor), ((2, 5), 3));
        let shown = grown.doc.node(text).and_then(|n| n.kind.text()).unwrap();
        assert_eq!(shown.characters, "hinih");
        // Committed, there is none, and the document has the text.
        textedit::insert_ime_commit(&mut f.board.session, "你").unwrap();
        Composition::refresh(&mut kept, &f.board.session);
        assert!(kept.is_none());
        assert_eq!(reads(&f.board, text), "hi你");
    }
}
