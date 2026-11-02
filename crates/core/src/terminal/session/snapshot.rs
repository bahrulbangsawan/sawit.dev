//! Snapshots of the visible screen, as cells with their colours resolved, for the UI to draw.

use alacritty_terminal::grid::{Dimensions, Indexed};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::COUNT;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use super::{Session, modes_of};
use crate::terminal::input::Modes;
use crate::terminal::palette::{Palette, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellView {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// Second half of a wide character; draw nothing.
    pub spacer: bool,
    /// Inside the mouse selection.
    pub selected: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorView {
    Block,
    Beam,
    Underline,
    Hidden,
}

/// Everything needed to draw the visible screen.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cols: usize,
    pub lines: Vec<Vec<CellView>>,
    /// (row, col) of the cursor when it is on screen.
    pub cursor: Option<(usize, usize)>,
    pub cursor_shape: CursorView,
    pub modes: Modes,
    /// Lines scrolled back into history (0 = live view).
    pub display_offset: usize,
}

impl Snapshot {
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                l.iter()
                    .filter(|c| !c.spacer)
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Session {
    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        let term = self.term.lock();
        let content = term.renderable_content();
        let cols = term.columns();
        let rows = term.screen_lines();
        // Every colour a cell can name, worked out once rather than for each cell: the one the
        // program set, else the palette's.
        let colors: [Rgb; COUNT] = std::array::from_fn(|ix| match content.colors[ix] {
            Some(c) => Rgb {
                r: c.r,
                g: c.g,
                b: c.b,
            },
            None => default_color(palette, ix),
        });
        let resolve = |color: Color| -> Rgb {
            match color {
                Color::Spec(c) => Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                },
                Color::Indexed(ix) => colors[ix as usize],
                Color::Named(named) => colors[named as usize],
            }
        };

        let selection = content.selection;
        let cursor_at = content.cursor.point;
        let cursor_shape = content.cursor.shape;
        let offset = content.display_offset as i32;
        let grid = term.grid();
        // Row by row: the grid's own iterator looks each cell's row up again.
        let lines = (0..rows as i32)
            .map(|row| {
                let line = Line(row - offset);
                grid[line][..Column(cols)]
                    .iter()
                    .enumerate()
                    .map(|(col, cell)| {
                        let point = Point::new(line, Column(col));
                        let selected = selection.is_some_and(|s| {
                            s.contains_cell(&Indexed { point, cell }, cursor_at, cursor_shape)
                        });
                        cell_view(cell, selected, resolve, palette)
                    })
                    .collect()
            })
            .collect();

        let mode = content.mode;
        let cursor_point: Point = content.cursor.point;
        let cursor_row = cursor_point.line.0 + offset;
        let cursor_visible = mode.contains(TermMode::SHOW_CURSOR)
            && content.cursor.shape != CursorShape::Hidden
            && cursor_row >= 0
            && (cursor_row as usize) < rows;
        let cursor = cursor_visible.then(|| {
            (
                cursor_row as usize,
                cursor_point.column.0.min(cols.saturating_sub(1)),
            )
        });
        let cursor_shape = match content.cursor.shape {
            CursorShape::Beam => CursorView::Beam,
            CursorShape::Underline => CursorView::Underline,
            CursorShape::Hidden => CursorView::Hidden,
            _ => CursorView::Block,
        };
        let modes = modes_of(mode);
        Snapshot {
            cols,
            lines,
            cursor,
            cursor_shape,
            modes,
            display_offset: content.display_offset,
        }
    }
}

/// What `cell` shows, its colours resolved by `resolve`.
fn cell_view(
    cell: &Cell,
    selected: bool,
    resolve: impl Fn(Color) -> Rgb,
    palette: &Palette,
) -> CellView {
    let flags = cell.flags;
    let mut fg = resolve(cell.fg);
    let mut bg = resolve(cell.bg);
    if flags.contains(Flags::BOLD)
        && let Color::Named(n) = cell.fg
        && (n as usize) < 8
    {
        fg = palette.ansi[n as usize + 8];
    }
    if flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut fg, &mut bg);
    }
    if flags.contains(Flags::DIM) {
        fg = Rgb {
            r: fg.r / 3 * 2,
            g: fg.g / 3 * 2,
            b: fg.b / 3 * 2,
        };
    }
    if flags.contains(Flags::HIDDEN) {
        fg = bg;
    }
    CellView {
        ch: if cell.c == '\0' || cell.c == '\t' {
            ' '
        } else {
            cell.c
        },
        fg,
        bg,
        bold: flags.contains(Flags::BOLD),
        italic: flags.contains(Flags::ITALIC),
        underline: flags.intersects(Flags::ALL_UNDERLINES),
        strikeout: flags.contains(Flags::STRIKEOUT),
        spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
        selected,
    }
}

/// The palette's colour for terminal colour `ix`, for one the program didn't set: the 256
/// indexed colours, then the named foreground, background and cursor, the dim colours (onto
/// the normal ones) and the bright and dim foregrounds.
fn default_color(palette: &Palette, ix: usize) -> Rgb {
    let dim = NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize;
    match ix {
        0..=255 => palette.indexed(ix as u8),
        _ if ix == NamedColor::Background as usize => palette.background,
        _ if ix == NamedColor::Cursor as usize => palette.cursor,
        _ if dim.contains(&ix) => palette.ansi[ix - NamedColor::DimBlack as usize],
        _ => palette.foreground,
    }
}
