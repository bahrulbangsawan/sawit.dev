//! Find in a terminal's grid: a match across the rows a long line wraps onto, in the scrollback,
//! and the cells a snapshot marks.

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use regex::Regex;

use crate::terminal::{Found, GridSize, find_in_rows, found_at};

/// A terminal `cols` wide and `rows` high that has been sent `text`.
fn term(cols: u16, rows: u16, text: &str) -> Term<VoidListener> {
    let size = GridSize {
        rows,
        cols,
        cell_width: 8,
        cell_height: 16,
    };
    let mut term = Term::new(Config::default(), &size, VoidListener);
    let mut parser: Processor = Processor::new();
    parser.advance(&mut term, text.as_bytes());
    term
}

fn point(line: i32, col: usize) -> Point {
    Point::new(Line(line), Column(col))
}

#[test]
fn a_match_runs_across_wrapped_rows() {
    // Ten columns: "0123456789" fills the first row, "needle" starts at its column 7 and wraps.
    let term = term(10, 4, "0123456needle tail");
    let grid = term.grid();
    let re = Regex::new("needle").unwrap();
    let found = find_in_rows(grid, &re, 0, 3);
    assert_eq!(found, vec![point(0, 7)..=point(1, 2)]);
    // Searching only the second row still finds it: the line it is part of is read whole.
    assert_eq!(find_in_rows(grid, &re, 1, 1), found);
    // Each of its cells is marked, the current match more strongly.
    let current = found[0].clone();
    assert_eq!(found_at(&found, None, point(0, 9)), Found::Match);
    assert_eq!(
        found_at(&found, Some(&current), point(1, 0)),
        Found::Current
    );
    assert_eq!(found_at(&found, None, point(1, 3)), Found::No);
}

#[test]
fn the_scrollback_is_searched_too() {
    let mut text = String::from("first needle\r\n");
    for n in 0..20 {
        text.push_str(&format!("line {n}\r\n"));
    }
    text.push_str("last needle");
    let term = term(20, 5, &text);
    let grid = term.grid();
    let top = -(grid.history_size() as i32);
    assert!(top < 0, "lines went into the scrollback");
    let re = Regex::new("needle").unwrap();
    let found = find_in_rows(grid, &re, top, grid.screen_lines() as i32 - 1);
    assert_eq!(found.len(), 2);
    assert_eq!(*found[0].start(), point(top, 6), "in the scrollback");
    assert_eq!(found[1].start().line, Line(4), "on the screen's last row");
}
