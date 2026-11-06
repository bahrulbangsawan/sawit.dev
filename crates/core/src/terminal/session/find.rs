//! Find in a terminal: the scrollback and the screen searched line by line, a line being the rows
//! a long line wrapped onto joined back up, so a match can run from one row onto the next. The
//! bar's matches ([`Session::find_all`]) and the highlights of the rows on screen
//! ([`Session::set_find`], which [`Session::snapshot`] marks) are worked out from the grid as it
//! is when asked, so new output never leaves a highlight on the wrong cells.

use std::ops::RangeInclusive;

use alacritty_terminal::grid::{Dimensions, Grid, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use regex::Regex;

use super::Session;

/// A match, from its first cell to its last, in grid lines (the screen's top row is line 0, the
/// scrollback above it negative).
pub type GridMatch = RangeInclusive<Point>;

/// What the terminal highlights while its find bar is open: every match of `regex`, and the
/// current one, `current`, more strongly.
#[derive(Debug, Clone)]
pub struct FindMarks {
    pub regex: Regex,
    pub current: Option<GridMatch>,
    /// The lines in the scrollback when `current` was found: as output pushes more into it, the
    /// match's lines move up by as many.
    pub history: usize,
}

impl FindMarks {
    /// The current match where it is now, the scrollback being `history` lines.
    pub fn current_now(&self, history: usize) -> Option<GridMatch> {
        let moved = history as i32 - self.history as i32;
        self.current.as_ref().map(|m| {
            let (mut from, mut to) = (*m.start(), *m.end());
            from.line -= moved;
            to.line -= moved;
            from..=to
        })
    }
}

/// How a cell is found: not, in a match, or in the current match.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Found {
    #[default]
    No,
    Match,
    Current,
}

impl Session {
    /// Every match of `re` in the scrollback and on the screen, top to bottom, with the number of
    /// lines then in the scrollback: a match's line moves up by as many as the scrollback grows.
    pub fn find_all(&self, re: &Regex) -> (Vec<GridMatch>, usize) {
        let term = self.term.lock();
        let grid = term.grid();
        let top = -(grid.history_size() as i32);
        let bottom = grid.screen_lines() as i32 - 1;
        (find_in_rows(grid, re, top, bottom), grid.history_size())
    }

    /// What to highlight from now on (`None`: nothing, the bar is closed).
    pub fn set_find(&self, marks: Option<FindMarks>) {
        *self.find.lock() = marks;
        self.changed();
    }

    /// The lines in the scrollback now, to move matches found earlier by how much it grew.
    pub fn history_size(&self) -> usize {
        self.term.lock().grid().history_size()
    }

    /// Scrolls so `line` is on screen, a third of the way down, unless it is already, below the
    /// top `clear` rows (which the find bar covers).
    pub fn reveal_line(&self, line: i32, clear: usize) {
        let mut term = self.term.lock();
        let rows = term.screen_lines() as i32;
        let offset = term.grid().display_offset() as i32;
        let row = line + offset;
        if (clear as i32..rows).contains(&row) {
            return;
        }
        let want = (rows / 3 - line).clamp(0, term.grid().history_size() as i32);
        term.scroll_display(Scroll::Delta(want - offset));
        drop(term);
        self.changed();
    }
}

/// The matches of `re` in rows `top..=bottom` of `grid`, and in the rows the lines that touch them
/// wrap from or onto, top to bottom.
pub fn find_in_rows(grid: &Grid<Cell>, re: &Regex, top: i32, bottom: i32) -> Vec<GridMatch> {
    let first = -(grid.history_size() as i32);
    let last = grid.screen_lines() as i32 - 1;
    let cols = grid.columns();
    if cols == 0 {
        return Vec::new();
    }
    let wraps = |row: i32| {
        grid[Line(row)][Column(cols - 1)]
            .flags
            .contains(Flags::WRAPLINE)
    };
    let mut start = top.max(first);
    while start > first && wraps(start - 1) {
        start -= 1;
    }
    let bottom = bottom.min(last);
    let mut found = Vec::new();
    let (mut text, mut points) = (String::new(), Vec::new());
    while start <= bottom {
        let mut end = start;
        while end < last && wraps(end) {
            end += 1;
        }
        text.clear();
        points.clear();
        for row in start..=end {
            let line = &grid[Line(row)];
            for col in 0..cols {
                let cell = &line[Column(col)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let point = Point::new(Line(row), Column(col));
                text.push(cell.c);
                text.extend(cell.zerowidth().into_iter().flatten());
                points.resize(text.len(), point);
            }
        }
        for m in re.find_iter(&text).filter(|m| !m.is_empty()) {
            let from = points[m.start()];
            let mut to = points[m.end() - 1];
            // A wide character's last cell is its spacer.
            let row = &grid[to.line];
            if to.column.0 + 1 < cols && row[to.column].flags.contains(Flags::WIDE_CHAR) {
                to.column += 1;
            }
            found.push(from..=to);
        }
        start = end + 1;
    }
    found
}

/// How the cell at `point` is found among `matches` (sorted), `current` being the current match.
pub fn found_at(matches: &[GridMatch], current: Option<&GridMatch>, point: Point) -> Found {
    let ix = matches.partition_point(|m| *m.end() < point);
    match matches.get(ix) {
        Some(m) if *m.start() <= point => {
            if current.is_some_and(|c| c == m) {
                Found::Current
            } else {
                Found::Match
            }
        }
        _ => Found::No,
    }
}
