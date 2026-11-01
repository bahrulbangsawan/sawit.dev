//! Snapshots of the visible screen, as cells with their colours resolved, for the UI to draw.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
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
        let colors = content.colors;
        let resolve = |color: Color, is_fg: bool| -> Rgb {
            let from_term = |ix: usize| {
                colors[ix].map(|c| Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                })
            };
            match color {
                Color::Spec(c) => Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                },
                Color::Indexed(ix) => from_term(ix as usize).unwrap_or_else(|| palette.indexed(ix)),
                Color::Named(named) => {
                    let ix = named as usize;
                    from_term(ix).unwrap_or_else(|| match named {
                        NamedColor::Foreground
                        | NamedColor::BrightForeground
                        | NamedColor::DimForeground => palette.foreground,
                        NamedColor::Background => palette.background,
                        NamedColor::Cursor => palette.cursor,
                        n if (n as usize) < 16 => palette.ansi[n as usize],
                        // Dim variants map onto the normal colors.
                        n => {
                            let base = (n as usize).saturating_sub(NamedColor::DimBlack as usize);
                            if base < 8 {
                                palette.ansi[base]
                            } else if is_fg {
                                palette.foreground
                            } else {
                                palette.background
                            }
                        }
                    })
                }
            }
        };

        let blank = CellView {
            ch: ' ',
            fg: palette.foreground,
            bg: palette.background,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
            selected: false,
        };
        let selection = content.selection;
        let cursor_at = content.cursor.point;
        let cursor_shape = content.cursor.shape;
        let mut lines = vec![vec![blank; cols]; rows];
        let offset = content.display_offset as i32;
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            let col = indexed.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            let selected =
                selection.is_some_and(|s| s.contains_cell(&indexed, cursor_at, cursor_shape));
            let cell = indexed.cell;
            let flags = cell.flags;
            let mut fg = resolve(cell.fg, true);
            let mut bg = resolve(cell.bg, false);
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
            lines[row as usize][col] = CellView {
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
            };
        }

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
