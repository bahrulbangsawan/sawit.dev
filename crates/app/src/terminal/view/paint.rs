//! Painting a snapshot of the screen: cell backgrounds, text runs on the cell grid, the cursor.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::*;
use sawit_core::terminal::{CellView, CursorView, Palette, Rgb, Snapshot};

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

/// Where and how to paint a snapshot.
pub(super) struct Painter<'a> {
    pub(super) palette: &'a Palette,
    pub(super) origin: Point<Pixels>,
    pub(super) cell: Size<Pixels>,
    pub(super) font: &'a Font,
    pub(super) focused: bool,
    pub(super) selection: Hsla,
}

/// Paints one snapshot: backgrounds, text runs (glyphs forced onto the cell
/// grid), then the cursor.
pub(super) fn paint_snapshot(snap: &Snapshot, p: &Painter, window: &mut Window, cx: &mut App) {
    let (palette, origin, cell, font, focused) = (p.palette, p.origin, p.cell, p.font, p.focused);
    let background = |c: &CellView| {
        if c.selected {
            Some(p.selection)
        } else {
            (c.bg != palette.background).then(|| hsla(c.bg))
        }
    };

    let font_size = px(FONT_SIZE);
    for (row, line) in snap.lines.iter().enumerate() {
        let y = origin.y + cell.height * row as f32;
        // Backgrounds, merged across equal neighbours.
        let mut col = 0;
        while col < line.len() {
            let bg = background(&line[col]);
            let start = col;
            while col < line.len() && background(&line[col]) == bg {
                col += 1;
            }
            if let Some(bg) = bg {
                let bounds = Bounds::new(
                    point(origin.x + cell.width * start as f32, y),
                    size(cell.width * (col - start) as f32, cell.height),
                );
                window.paint_quad(fill(bounds, bg));
            }
        }

        // Text: one shaped line per row, a run per style change. Trailing
        // blanks (most of a typical row) are not shaped at all.
        let Some(last) = line
            .iter()
            .rposition(|c| (!c.spacer && c.ch != ' ') || c.underline || c.strikeout)
        else {
            continue;
        };
        let line = &line[..=last];
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
            let mut run_font = font.clone();
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
            continue;
        }
        let shaped =
            window
                .text_system()
                .shape_line(text.into(), font_size, &runs, Some(cell.width));
        let _ = shaped.paint(
            point(origin.x, y),
            cell.height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    if let Some((row, col)) = snap.cursor {
        let at = point(
            origin.x + cell.width * col as f32,
            origin.y + cell.height * row as f32,
        );
        let color = hsla(palette.cursor);
        let quad = match (snap.cursor_shape, focused) {
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
}
