//! Painting the screen: cell backgrounds, text runs on the cell grid, the cursor. A
//! [`FrameCache`] keeps what the last frame painted, so the screen is read again only once it
//! changed, and a row's backgrounds and shaped text are made again only when its cells did: a
//! pane drawn again for another pane's output, or for a line that changed, redoes little.

use std::collections::HashMap;
use std::ops::Range;

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::terminal::{CellView, CursorView, Palette, Rgb, Session, Snapshot};

pub(super) const FONT_SIZE: f32 = 13.;
const LINE_HEIGHT: f32 = 1.3;

/// The size of one grid cell: the mono font's advance by its line height.
pub(in crate::terminal) fn cell_size(window: &Window, cx: &App) -> Size<Pixels> {
    let font = font(cx.theme().mono_font_family.clone());
    let font_id = window.text_system().resolve_font(&font);
    let font_size = px(FONT_SIZE);
    let cell_width = window
        .text_system()
        .advance(font_id, font_size, 'm')
        .map(|s| s.width)
        .unwrap_or(px(8.));
    size(cell_width, (font_size * LINE_HEIGHT).round())
}

pub(super) fn hsla(c: Rgb) -> Hsla {
    Rgba {
        r: c.r as f32 / 255.,
        g: c.g as f32 / 255.,
        b: c.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// Where and how to paint the screen.
pub(super) struct Painter<'a> {
    pub(super) palette: &'a Palette,
    pub(super) origin: Point<Pixels>,
    pub(super) cell: Size<Pixels>,
    pub(super) font: &'a Font,
    pub(super) focused: bool,
    pub(super) selection: Hsla,
}

/// What the last frame painted, to paint the next one from.
#[derive(Default)]
pub(super) struct FrameCache {
    /// The screen as last read, and the session's [`Session::changes`] from just before.
    screen: Option<(u64, Snapshot)>,
    /// What the rows were made with besides their cells; when it changes, they are all made
    /// again.
    look: Option<Look>,
    /// The screen's rows, ready to paint.
    rows: Vec<Row>,
    /// How many rows were made, not kept from the frame before (for tests).
    #[cfg(test)]
    pub(super) made: usize,
}

/// What a row's backgrounds and text depend on besides its cells.
#[derive(PartialEq)]
struct Look {
    palette: Palette,
    font: Font,
    cell_width: Pixels,
    selection: Hsla,
}

/// One row, ready to paint: its backgrounds by column, merged across equal neighbours, and its
/// text shaped on the cell grid (`None` when it has none to draw). `hash` is its cells' hash,
/// kept once worked out, to find the row again when the screen scrolls it.
struct Row {
    backgrounds: Vec<(Range<usize>, Hsla)>,
    text: Option<ShapedLine>,
    hash: Option<u64>,
}

/// Paints `session`'s screen, as `cache` keeps it: backgrounds, text runs (glyphs forced onto
/// the cell grid), then the cursor.
pub(super) fn paint_screen(
    cache: &mut FrameCache,
    session: &Session,
    p: &Painter,
    window: &mut Window,
    cx: &mut App,
) {
    cache.update(session, p, window);
    let Some((_, snap)) = &cache.screen else {
        return;
    };
    let (origin, cell) = (p.origin, p.cell);
    for (ix, row) in cache.rows.iter().enumerate() {
        let y = origin.y + cell.height * ix as f32;
        for (cols, color) in &row.backgrounds {
            let bounds = Bounds::new(
                point(origin.x + cell.width * cols.start as f32, y),
                size(cell.width * cols.len() as f32, cell.height),
            );
            window.paint_quad(fill(bounds, *color));
        }
        if let Some(text) = &row.text {
            let _ = text.paint(
                point(origin.x, y),
                cell.height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        }
    }
    paint_cursor(snap, p, window);
}

impl FrameCache {
    /// Reads the screen again if it changed since the last frame, and makes again the rows
    /// whose cells changed (every row, when the font, cell width or colours did). A row the
    /// screen scrolled to another place is found by its cells and kept.
    fn update(&mut self, session: &Session, p: &Painter, window: &Window) {
        let look = Look {
            palette: p.palette.clone(),
            font: p.font.clone(),
            cell_width: p.cell.width,
            selection: p.selection,
        };
        // Read before the snapshot: output that comes during it moves the count on again.
        let changes = session.changes();
        if self.look.as_ref() != Some(&look) {
            self.look = Some(look);
            self.screen = None;
            self.rows.clear();
        } else if self
            .screen
            .as_ref()
            .is_some_and(|(seen, _)| *seen == changes)
        {
            return;
        }
        let snap = session.snapshot(p.palette);
        let old = self.screen.take().map(|(_, old)| old);
        let mut old_rows: Vec<Option<Row>> = std::mem::take(&mut self.rows)
            .into_iter()
            .map(Some)
            .collect();
        // The old rows by their cells' hash, once a row isn't where it was.
        let mut moved: Option<HashMap<u64, usize>> = None;
        let mut rows = Vec::with_capacity(snap.lines.len());
        for (ix, cells) in snap.lines.iter().enumerate() {
            let mut kept = None;
            let mut hash = None;
            if let Some(old) = &old {
                if old.lines.get(ix) == Some(cells) {
                    kept = old_rows[ix].take();
                }
                if kept.is_none() {
                    let cells_hash = *hash.insert(row_hash(cells));
                    let moved = moved.get_or_insert_with(|| by_hash(old, &mut old_rows));
                    if let Some(&from) = moved.get(&cells_hash)
                        && old.lines[from] == *cells
                    {
                        kept = old_rows[from].take();
                    }
                }
            }
            rows.push(kept.unwrap_or_else(|| {
                #[cfg(test)]
                {
                    self.made += 1;
                }
                make_row(cells, hash, p, window)
            }));
        }
        self.rows = rows;
        self.screen = Some((changes, snap));
    }
}

/// The rows of `old` not yet kept, by their cells' hash (worked out for those without one).
fn by_hash(old: &Snapshot, rows: &mut [Option<Row>]) -> HashMap<u64, usize> {
    rows.iter_mut()
        .zip(&old.lines)
        .enumerate()
        .filter_map(|(ix, (row, cells))| {
            let row = row.as_mut()?;
            Some((*row.hash.get_or_insert_with(|| row_hash(cells)), ix))
        })
        .collect()
}

/// A hash of a row's cells, to find the row again once the screen moved it. Rows it finds are
/// compared cell by cell all the same.
fn row_hash(cells: &[CellView]) -> u64 {
    // FxHash's mixing: a rotate, an xor and a multiply per word.
    let mix = |h: u64, word: u64| (h.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    let rgb = |c: Rgb| (c.r as u64) << 16 | (c.g as u64) << 8 | c.b as u64;
    cells.iter().fold(cells.len() as u64, |h, c| {
        let flags = c.bold as u64
            | (c.italic as u64) << 1
            | (c.underline as u64) << 2
            | (c.strikeout as u64) << 3
            | (c.spacer as u64) << 4
            | (c.selected as u64) << 5;
        let glyph = (c.ch as u64) << 32 | flags << 24 | rgb(c.fg);
        mix(mix(h, glyph), rgb(c.bg))
    })
}

/// `cells` ready to paint, with their hash when it is known.
fn make_row(cells: &[CellView], hash: Option<u64>, p: &Painter, window: &Window) -> Row {
    Row {
        backgrounds: backgrounds(cells, p),
        text: shape(cells, p, window),
        hash,
    }
}

/// A row's backgrounds by column, merged across equal neighbours: the selection's, and any
/// other than the terminal's own. Each run of like cells takes its colour once.
fn backgrounds(cells: &[CellView], p: &Painter) -> Vec<(Range<usize>, Hsla)> {
    let same =
        |a: &CellView, b: &CellView| a.selected == b.selected && (a.selected || a.bg == b.bg);
    let mut out: Vec<(Range<usize>, Hsla)> = Vec::new();
    let mut col = 0;
    while col < cells.len() {
        let start = col;
        let first = &cells[start];
        while col < cells.len() && same(&cells[col], first) {
            col += 1;
        }
        let color = if first.selected {
            Some(p.selection)
        } else {
            (first.bg != p.palette.background).then(|| hsla(first.bg))
        };
        let Some(color) = color else {
            continue;
        };
        match out.last_mut() {
            Some((cols, last)) if cols.end == start && *last == color => cols.end = col,
            _ => out.push((start..col, color)),
        }
    }
    out
}

/// A row's text shaped as one line, a run per style change; `None` when there is nothing to
/// draw. Trailing blanks (most of a typical row) are not shaped at all.
fn shape(cells: &[CellView], p: &Painter, window: &Window) -> Option<ShapedLine> {
    let last = cells
        .iter()
        .rposition(|c| (!c.spacer && c.ch != ' ') || c.underline || c.strikeout)?;
    let line = &cells[..=last];
    let mut text = String::with_capacity(line.len());
    let mut runs: Vec<TextRun> = Vec::new();
    // Runs are compared by this small key; building a Font (and its
    // Arc clones) per cell was most of the paint time.
    let mut run_key = None;
    for c in line {
        let ch = if c.spacer { ' ' } else { c.ch };
        let len = ch.len_utf8();
        text.push(ch);
        let key = (c.fg, c.bold, c.italic, c.underline, c.strikeout);
        if run_key == Some(key)
            && let Some(run) = runs.last_mut()
        {
            run.len += len;
            continue;
        }
        run_key = Some(key);
        let mut run_font = p.font.clone();
        if c.bold {
            run_font.weight = FontWeight::BOLD;
        }
        if c.italic {
            run_font.style = FontStyle::Italic;
        }
        let color = hsla(c.fg);
        runs.push(TextRun {
            len,
            font: run_font,
            color,
            background_color: None,
            underline: c.underline.then(|| UnderlineStyle {
                thickness: px(1.),
                color: Some(color),
                wavy: false,
            }),
            strikethrough: c.strikeout.then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(color),
            }),
        });
    }
    if text.trim().is_empty() {
        return None;
    }
    Some(
        window
            .text_system()
            .shape_line(text.into(), px(FONT_SIZE), &runs, Some(p.cell.width)),
    )
}

/// The cursor, in its shape: a block filled while the terminal has the keyboard, else outlined.
fn paint_cursor(snap: &Snapshot, p: &Painter, window: &mut Window) {
    let Some((row, col)) = snap.cursor else {
        return;
    };
    let (origin, cell) = (p.origin, p.cell);
    let at = point(
        origin.x + cell.width * col as f32,
        origin.y + cell.height * row as f32,
    );
    let color = hsla(p.palette.cursor);
    let quad = match (snap.cursor_shape, p.focused) {
        (CursorView::Hidden, _) => None,
        (CursorView::Beam, _) => Some(fill(Bounds::new(at, size(px(2.), cell.height)), color)),
        (CursorView::Underline, _) => Some(fill(
            Bounds::new(
                point(at.x, at.y + cell.height - px(2.)),
                size(cell.width, px(2.)),
            ),
            color,
        )),
        (CursorView::Block, true) => Some(fill(Bounds::new(at, cell), color.opacity(0.6))),
        (CursorView::Block, false) => {
            Some(outline(Bounds::new(at, cell), color, BorderStyle::Solid))
        }
    };
    if let Some(quad) = quad {
        window.paint_quad(quad);
    }
}
