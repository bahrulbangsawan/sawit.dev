//! What a terminal's menus do to its screen (backlog item 22): Select All, Clear, Reset Terminal,
//! the scrollback as text for Export Output…, and the link or path under the mouse for Open Link,
//! Copy Link and Open File.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::vte::ansi::{ClearMode, Handler};

use super::Session;

/// What sits under the mouse in a terminal: a web link, or something that reads as a file path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Link(String),
    Path(String),
}

/// The link or path in `token` (a run of non-blank characters), without the brackets, quotes
/// and punctuation around it, and a path's `:line:column`.
pub fn target_in(token: &str) -> Option<Target> {
    // Brackets and quotes around it, and a sentence's punctuation after it, in any order.
    let token = token
        .trim_start_matches(|c: char| "([{<\"'`".contains(c))
        .trim_end_matches(|c: char| ")]}>\"'`,;.:".contains(c));
    if token.is_empty() {
        return None;
    }
    if let Some(at) = token.find("://") {
        let scheme = &token[..at];
        if ["http", "https", "file"].contains(&scheme) && token.len() > at + 3 {
            return Some(Target::Link(token.to_string()));
        }
        return None;
    }
    let looks_like_path = token.starts_with('/')
        || token.starts_with("./")
        || token.starts_with("../")
        || token.starts_with("~/")
        || (token.contains('/') && !token.starts_with('-'));
    if !looks_like_path {
        return None;
    }
    // `src/main.rs:12:5` names a file at a line: the file is what opens.
    let path = token
        .split(':')
        .next()
        .filter(|p| !p.is_empty())
        .unwrap_or(token);
    Some(Target::Path(path.to_string()))
}

impl Session {
    /// Selects the whole scrollback and screen.
    pub fn select_all(&self) {
        let mut term = self.term.lock();
        let top = Point::new(Line(-(term.grid().history_size() as i32)), Column(0));
        let bottom = Point::new(
            Line(term.screen_lines() as i32 - 1),
            Column(term.columns().saturating_sub(1)),
        );
        let mut selection = Selection::new(SelectionType::Simple, top, Side::Left);
        selection.update(bottom, Side::Right);
        term.selection = Some(selection);
        drop(term);
        self.changed();
    }

    /// Clears the scrollback, and the screen unless the shell redraws it: `at_prompt` sends the
    /// shell ⌃L, which clears the screen and draws its prompt again at the top. A program in
    /// front keeps its screen (it would not redraw a cleared one).
    pub fn clear(&self, at_prompt: bool) {
        {
            let mut term = self.term.lock();
            term.clear_screen(ClearMode::Saved);
            term.selection = None;
        }
        if at_prompt {
            self.write(&b"\x0c"[..]);
        }
        self.changed();
    }

    /// Resets the terminal's state (modes, colours, the alternate screen, the scrollback), as
    /// `reset` would, and has the shell draw its prompt again.
    pub fn reset(&self, at_prompt: bool) {
        self.term.lock().reset_state();
        if at_prompt {
            self.write(&b"\x0c"[..]);
        }
        self.changed();
    }

    /// The whole scrollback and screen as plain text, for Export Output…: wrapped rows joined,
    /// blank lines at the end dropped.
    pub fn all_text(&self) -> String {
        let (history, rows) = {
            let term = self.term.lock();
            (term.grid().history_size(), term.screen_lines())
        };
        self.text_from(0, history + rows)
    }

    /// The run of non-blank characters at screen cell (`col`, `row`), as the screen shows it now
    /// (scrolled or not): what a right-click there points at.
    pub fn token_at(&self, col: usize, row: usize) -> String {
        let term = self.term.lock();
        let offset = term.grid().display_offset() as i32;
        let line = Line(row as i32 - offset);
        let cols = term.columns();
        let cells: Vec<char> = (0..cols).map(|c| term.grid()[line][Column(c)].c).collect();
        if col >= cells.len() || cells[col].is_whitespace() {
            return String::new();
        }
        let start = cells[..col]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map_or(0, |i| i + 1);
        let end = cells[col..]
            .iter()
            .position(|c| c.is_whitespace())
            .map_or(cells.len(), |i| col + i);
        cells[start..end].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Target, target_in};

    #[test]
    fn links_and_paths_are_told_apart() {
        assert_eq!(
            target_in("(https://sawit.dev/docs)."),
            Some(Target::Link("https://sawit.dev/docs".into()))
        );
        assert_eq!(
            target_in("src/main.rs:12:5"),
            Some(Target::Path("src/main.rs".into()))
        );
        assert_eq!(
            target_in("~/notes.txt"),
            Some(Target::Path("~/notes.txt".into()))
        );
        assert_eq!(target_in("hello"), None);
        assert_eq!(target_in("--flag=a/b"), None);
        assert_eq!(target_in("ssh://host"), None);
    }
}
