//! Find in a view: the query and its options as a pattern, the matches in a text, and where the
//! find bar is among them ("3 of 12"). Every view's ⌘F bar uses it: the editor, the Markdown
//! preview, the PDF viewer, the browser's page (as a pattern the page's script runs), the canvas
//! and the terminal (see `terminal` for its grid).

use std::ops::Range;

pub use regex::Regex;
use regex::RegexBuilder;

/// How a query matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindOptions {
    /// Upper and lower case differ.
    pub match_case: bool,
    /// A match starts and ends at a word's edge.
    pub whole_word: bool,
    /// The query is a regular expression.
    pub regex: bool,
}

/// `query` with `options` as a regular expression, its source as `pattern` writes it. `None`
/// for an empty query; an error for a regular expression that doesn't parse, as the bar shows it.
pub fn matcher(query: &str, options: FindOptions) -> Result<Option<Regex>, String> {
    if query.is_empty() {
        return Ok(None);
    }
    RegexBuilder::new(&pattern(query, options))
        .case_insensitive(!options.match_case)
        .multi_line(true)
        .build()
        .map(Some)
        .map_err(|e| match e {
            regex::Error::Syntax(_) => "Invalid regular expression".to_string(),
            other => other.to_string(),
        })
}

/// The pattern for `query`, without the case flag: the query itself as a regular expression, else
/// escaped; between word edges for a whole word. A browser's page runs it as JavaScript's
/// `RegExp`, which reads the same for what `escape` writes and `\b`.
pub fn pattern(query: &str, options: FindOptions) -> String {
    let body = if options.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };
    if options.whole_word {
        format!(r"\b(?:{body})\b")
    } else {
        body
    }
}

/// Where `re` matches in `text`, as byte ranges, without the empty matches a regular expression
/// such as `a*` finds between characters.
pub fn find_all(text: &str, re: &Regex) -> Vec<Range<usize>> {
    re.find_iter(text)
        .filter(|m| !m.is_empty())
        .map(|m| m.range())
        .collect()
}

/// Where the find bar is among the matches: how many there are, and which one is current.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FindState {
    pub total: usize,
    pub current: Option<usize>,
}

impl FindState {
    /// New matches, `total` of them: the current one is the first at or after `near` (the match
    /// that was current, or where the view's cursor is), wrapping round to the first.
    pub fn found(total: usize, near: Option<usize>) -> Self {
        let current = (total > 0).then(|| near.filter(|&n| n < total).unwrap_or(0));
        Self { total, current }
    }

    /// Steps to the next match, after the last back to the first.
    pub fn step_next(&mut self) -> Option<usize> {
        self.step(1)
    }

    /// Steps to the previous match, before the first round to the last.
    pub fn step_previous(&mut self) -> Option<usize> {
        self.step(-1)
    }

    fn step(&mut self, by: isize) -> Option<usize> {
        if self.total == 0 {
            self.current = None;
            return None;
        }
        let total = self.total as isize;
        let next = match self.current {
            Some(at) => (at as isize + by).rem_euclid(total),
            None if by > 0 => 0,
            None => total - 1,
        };
        self.current = Some(next as usize);
        self.current
    }

    /// What the bar says: `3 of 12`, `No results`, or nothing before a query is typed.
    pub fn label(&self, query: &str) -> String {
        if query.is_empty() {
            return String::new();
        }
        match (self.total, self.current) {
            (0, _) => "No results".to_string(),
            (total, Some(at)) => format!("{} of {total}", at + 1),
            (total, None) => format!("{total} found"),
        }
    }
}

/// The first of `starts` (sorted match starts) at or after `at`, else the first: the match a
/// search begins on from the cursor.
pub fn first_from(starts: impl IntoIterator<Item = usize>, at: usize) -> Option<usize> {
    let mut first = None;
    for (ix, start) in starts.into_iter().enumerate() {
        first.get_or_insert(ix);
        if start >= at {
            return Some(ix);
        }
    }
    first
}

#[cfg(test)]
mod tests;
