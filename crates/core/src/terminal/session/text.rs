//! The screen's text: the mouse selection and plain-text read-outs of the grid.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

use super::Session;

/// What a selection snaps to: double click picks words, triple click lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectKind {
    Cell,
    Word,
    Line,
}

impl SelectKind {
    /// From a mouse click count (1, 2, 3, then repeating).
    pub fn from_clicks(count: usize) -> Self {
        match count {
            2 => SelectKind::Word,
            n if n >= 3 => SelectKind::Line,
            _ => SelectKind::Cell,
        }
    }
}

impl Session {
    /// Starts a selection at screen cell (`col`, `row`); `right_half` is
    /// which half of the cell the mouse is over.
    pub fn select_start(&self, col: usize, row: usize, right_half: bool, kind: SelectKind) {
        let mut term = self.term.lock();
        let point = grid_point(&term, col, row);
        let ty = match kind {
            SelectKind::Cell => SelectionType::Simple,
            SelectKind::Word => SelectionType::Semantic,
            SelectKind::Line => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(ty, point, side(right_half)));
        self.changed();
    }

    /// Moves the end of the selection to screen cell (`col`, `row`).
    pub fn select_extend(&self, col: usize, row: usize, right_half: bool) {
        let mut term = self.term.lock();
        let point = grid_point(&term, col, row);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side(right_half));
            self.changed();
        }
    }

    pub fn clear_selection(&self) {
        if self.term.lock().selection.take().is_some() {
            self.changed();
        }
    }

    /// The selected text, if anything is selected.
    pub fn selection_text(&self) -> Option<String> {
        let term = self.term.lock();
        if term.selection.as_ref().is_none_or(|s| s.is_empty()) {
            return None;
        }
        term.selection_to_string().filter(|t| !t.is_empty())
    }

    /// The last `max_lines` lines of scrollback and screen as plain text,
    /// soft-wrapped rows joined, trailing blank lines dropped.
    pub fn text_tail(&self, max_lines: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let cols = grid.columns();
        let top = -(grid.history_size() as i32);
        let wraps = |row: i32| {
            grid[Line(row)][Column(cols - 1)]
                .flags
                .contains(Flags::WRAPLINE)
        };
        // From the bottom up, so that only the rows asked for are read, not the whole history:
        // a line ends on a row that doesn't wrap (or the last row) and takes the rows above it
        // that wrap onto it.
        let mut lines: Vec<String> = Vec::new();
        let mut end = grid.screen_lines() as i32 - 1;
        while end >= top && lines.len() < max_lines {
            let mut start = end;
            while start > top && wraps(start - 1) {
                start -= 1;
            }
            let mut text = String::new();
            for row in start..=end {
                let row = &grid[Line(row)];
                for col in 0..cols {
                    let cell = &row[Column(col)];
                    if !cell
                        .flags
                        .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                    {
                        text.push(cell.c);
                        text.extend(cell.zerowidth().into_iter().flatten());
                    }
                }
            }
            let text = text.trim_end();
            // Blank lines below the last text are left out.
            if !text.is_empty() || !lines.is_empty() {
                lines.push(text.to_string());
            }
            end = start - 1;
        }
        lines.reverse();
        lines.join("\n")
    }
}

impl Session {
    /// Where the cursor is, counted in rows from the top of the scrollback: a mark that
    /// [`Session::text_from`] reads output after. Marks move up when the scrollback is full and
    /// its oldest rows go; a mark above what is left reads from the top.
    pub fn cursor_mark(&self) -> usize {
        let term = self.term.lock();
        let grid = term.grid();
        grid.history_size() + grid.cursor.point.line.0.max(0) as usize
    }

    /// The text from row mark `from` up to (not including) row mark `to` ([`Self::cursor_mark`]),
    /// as plain text: soft-wrapped rows joined into one line, trailing spaces and blank lines at
    /// the end dropped. A screen holds characters, never escape codes, so none are copied. A `to`
    /// past the screen (`usize::MAX`) reads through its end.
    pub fn text_from(&self, from: usize, to: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let history = grid.history_size() as i64;
        let last = grid.screen_lines() as i64 - 1;
        let row_of = |mark: usize| {
            let mark = i64::try_from(mark).unwrap_or(i64::MAX);
            mark.saturating_sub(history).clamp(-history, last + 1) as i32
        };
        let (start, end) = (row_of(from), row_of(to));
        if start >= end {
            return String::new();
        }
        let cols = grid.columns();
        let mut lines: Vec<String> = Vec::new();
        let mut current = String::new();
        for row in start..end {
            let r = &grid[Line(row)];
            for col in 0..cols {
                let cell = &r[Column(col)];
                if !cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    current.push(cell.c);
                    current.extend(cell.zerowidth().into_iter().flatten());
                }
            }
            if !r[Column(cols - 1)].flags.contains(Flags::WRAPLINE) {
                lines.push(current.trim_end().to_string());
                current.clear();
            }
        }
        if !current.is_empty() {
            lines.push(current.trim_end().to_string());
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines.join("\n")
    }

    /// The screen as it shows now (not the scrollback), as plain text like [`Self::text_from`].
    pub fn screen_text(&self) -> String {
        let (history, rows) = {
            let term = self.term.lock();
            (term.grid().history_size(), term.grid().screen_lines())
        };
        self.text_from(history, history + rows)
    }
}

/// Screen cell to grid point, following the scrollback position.
fn grid_point<T>(term: &Term<T>, col: usize, row: usize) -> Point {
    let offset = term.grid().display_offset() as i32;
    let last_col = term.columns().saturating_sub(1);
    Point::new(Line(row as i32 - offset), Column(col.min(last_col)))
}

fn side(right_half: bool) -> Side {
    if right_half { Side::Right } else { Side::Left }
}
