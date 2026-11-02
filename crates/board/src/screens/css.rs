//! The references a style sheet makes: `url(…)` and `@import`, found by a small scanner that
//! knows CSS's strings, comments and escapes, so a reference is rewritten in place.
//!
//! [`references`] lists them, [`rewrite_urls`] changes the `url(…)`s of a sheet or of a `style`
//! attribute, and [`flatten`] inlines a sheet's local `@import`s with each sheet's `url(…)`s
//! resolved from that sheet's own file. Both exports use them: the single HTML file turns the
//! references into `data:` URLs, the React export into paths in its assets folder.

mod imports;
#[cfg(test)]
mod tests;

use std::fmt::Write as _;
use std::ops::Range;

pub use imports::{Flat, flatten, wrap};

/// What kind of reference a [`Reference`] is.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// `url(…)`, its URL quoted or not.
    Url,
    /// A string in `image-set(…)` or `-webkit-image-set(…)`, which is a URL there.
    ImageSet,
    /// A whole `@import` rule; `conditions` is what follows its URL (`screen`, `layer(base)`,
    /// `supports(display: grid)`), trimmed.
    Import { conditions: String },
}

/// A reference in a style sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Reference {
    pub kind: Kind,
    /// What it refers to, its CSS escapes decoded.
    pub href: String,
    /// Where it is in the sheet: the `url(…)`, the string, or the `@import` rule through its `;`.
    pub span: Range<usize>,
}

/// The `url(…)`s, `image-set(…)` strings and `@import` rules in `css`, in order. What is in a
/// comment or in another string isn't one; a `url(…)` inside an `@import` is part of the rule.
pub fn references(css: &str) -> Vec<Reference> {
    let b = css.as_bytes();
    let mut found = Vec::new();
    let mut i = 0;
    let mut depth = 0usize;
    // The parenthesis depths of the `image-set(` calls the scanner is in.
    let mut image_sets: Vec<usize> = Vec::new();
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'*') => i = comment_end(b, i),
            b'"' | b'\'' => {
                let (end, href) = string(css, i);
                if image_sets.last() == Some(&depth) {
                    let span = i..end;
                    found.push(Reference {
                        kind: Kind::ImageSet,
                        href,
                        span,
                    });
                }
                i = end;
            }
            b'\\' => i = escape_end(css, i),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                if image_sets.last() == Some(&depth) {
                    image_sets.pop();
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'@' => {
                let name_end = ident_end(b, i + 1);
                if css[i + 1..name_end].eq_ignore_ascii_case("import")
                    && let Some(rule) = import(css, i, name_end)
                {
                    i = rule.span.end;
                    found.push(rule);
                    continue;
                }
                i = name_end;
            }
            c if starts_ident(c) => {
                let end = ident_end(b, i);
                if b.get(end) == Some(&b'(') {
                    let name = &css[i..end];
                    if name.eq_ignore_ascii_case("url") {
                        let (href, url_end) = url_body(css, end + 1);
                        if let Some(href) = href {
                            found.push(Reference {
                                kind: Kind::Url,
                                href,
                                span: i..url_end,
                            });
                        }
                        i = url_end;
                        continue;
                    }
                    if name.to_ascii_lowercase().ends_with("image-set") {
                        image_sets.push(depth + 1);
                    }
                }
                i = end;
            }
            _ => i += 1,
        }
    }
    found
}

/// `css` with each `url(…)` (and `image-set(…)` string) that `f` gives a new reference for
/// rewritten to it, written `url("…")` (a quoted string in `image-set`). `f` gets the reference
/// as it is, escapes decoded; what it gives `None` for stays exactly as written, and so do
/// `@import` rules.
pub fn rewrite_urls(css: &str, mut f: impl FnMut(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(css.len());
    let mut last = 0;
    for reference in references(css) {
        if matches!(reference.kind, Kind::Import { .. }) {
            continue;
        }
        if let Some(new) = f(&reference.href) {
            out.push_str(&css[last..reference.span.start]);
            out.push_str(&token(&reference.kind, &new));
            last = reference.span.end;
        }
    }
    out.push_str(&css[last..]);
    out
}

/// `href` as a CSS string: in double quotes, with `"`, `\` and control characters escaped.
pub fn quoted(href: &str) -> String {
    let mut out = String::with_capacity(href.len() + 2);
    out.push('"');
    for c in href.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\{:x} ", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `href` as a `url("…")`.
pub fn url_token(href: &str) -> String {
    format!("url({})", quoted(href))
}

/// The text a rewritten reference of `kind` is written as.
fn token(kind: &Kind, href: &str) -> String {
    match kind {
        Kind::ImageSet => quoted(href),
        _ => url_token(href),
    }
}

/// `text` with CSS escapes decoded: `\"`, `\\`, `\26 ` (a code point in hex), and an escaped
/// newline (a string's line continuation) dropped.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek().copied() {
            None => {}
            Some('\n' | '\x0c') => {
                chars.next();
            }
            Some('\r') => {
                chars.next();
                chars.next_if_eq(&'\n');
            }
            Some(h) if h.is_ascii_hexdigit() => {
                let mut code = 0u32;
                for _ in 0..6 {
                    match chars.peek().and_then(|h| h.to_digit(16)) {
                        Some(digit) => {
                            code = code * 16 + digit;
                            chars.next();
                        }
                        None => break,
                    }
                }
                match chars.peek() {
                    Some(' ' | '\t' | '\n' | '\x0c') => {
                        chars.next();
                    }
                    Some('\r') => {
                        chars.next();
                        chars.next_if_eq(&'\n');
                    }
                    _ => {}
                }
                let decoded = char::from_u32(code).filter(|_| code != 0);
                out.push(decoded.unwrap_or('\u{FFFD}'));
            }
            Some(other) => {
                chars.next();
                out.push(other);
            }
        }
    }
    out
}

/// The `@import` rule whose `@` is at `at` and whose name ends at `name_end`: `None` when no URL
/// follows the name, or a block does (it isn't a statement then).
fn import(css: &str, at: usize, name_end: usize) -> Option<Reference> {
    let b = css.as_bytes();
    let j = skip_space_and_comments(b, name_end);
    let (href, after) = match b.get(j)? {
        b'"' | b'\'' => {
            let (end, href) = string(css, j);
            (href, end)
        }
        _ => {
            let end = ident_end(b, j);
            if !css[j..end].eq_ignore_ascii_case("url") || b.get(end) != Some(&b'(') {
                return None;
            }
            let (href, end) = url_body(css, end + 1);
            (href?, end)
        }
    };
    let mut k = after;
    let mut depth = 0usize;
    let semicolon = loop {
        match b.get(k) {
            None => break None,
            Some(b';') if depth == 0 => break Some(k),
            Some(b'{' | b'}') if depth == 0 => return None,
            Some(b'(') => {
                depth += 1;
                k += 1;
            }
            Some(b')') => {
                depth = depth.saturating_sub(1);
                k += 1;
            }
            Some(b'"' | b'\'') => k = string(css, k).0,
            Some(b'/') if b.get(k + 1) == Some(&b'*') => k = comment_end(b, k),
            Some(b'\\') => k = escape_end(css, k),
            Some(_) => k += 1,
        }
    };
    let (conditions_end, end) = match semicolon {
        Some(at) => (at, at + 1),
        None => (b.len(), b.len()),
    };
    let conditions = css[after..conditions_end].trim().to_string();
    Some(Reference {
        kind: Kind::Import { conditions },
        href,
        span: at..end,
    })
}

/// The URL of a `url(` whose parenthesis ends at `start`, and where the `url(…)` ends: after its
/// `)`. A bad unquoted URL (a quote, a parenthesis or a space inside it) has none.
fn url_body(css: &str, start: usize) -> (Option<String>, usize) {
    let b = css.as_bytes();
    let mut j = skip_space(b, start);
    if matches!(b.get(j), Some(b'"' | b'\'')) {
        let (end, href) = string(css, j);
        return (Some(href), function_end(css, end));
    }
    let from = j;
    while j < b.len() {
        match b[j] {
            b')' => return (Some(unescape(&css[from..j])), j + 1),
            b'\\' => j = escape_end(css, j),
            b'"' | b'\'' | b'(' => return (None, bad_url_end(css, j)),
            c if is_space(c) => {
                let k = skip_space(b, j);
                return match b.get(k) {
                    Some(b')') => (Some(unescape(&css[from..j])), k + 1),
                    None => (Some(unescape(&css[from..j])), k),
                    Some(_) => (None, bad_url_end(css, k)),
                };
            }
            _ => j += 1,
        }
    }
    (Some(unescape(&css[from..])), b.len())
}

/// Where a function whose arguments go on from `start` ends: after the `)` that closes it, or at
/// the end of the text.
fn function_end(css: &str, start: usize) -> usize {
    function_close(css, start).map_or(css.len(), |at| at + 1)
}

/// The `)` that closes a function whose arguments go on from `start`: strings, comments and
/// inner parentheses skipped.
fn function_close(css: &str, start: usize) -> Option<usize> {
    let b = css.as_bytes();
    let mut depth = 0usize;
    let mut j = start;
    while j < b.len() {
        match b[j] {
            b')' if depth == 0 => return Some(j),
            b')' => {
                depth -= 1;
                j += 1;
            }
            b'(' => {
                depth += 1;
                j += 1;
            }
            b'"' | b'\'' => j = string(css, j).0,
            b'/' if b.get(j + 1) == Some(&b'*') => j = comment_end(b, j),
            b'\\' => j = escape_end(css, j),
            _ => j += 1,
        }
    }
    None
}

/// Where the rest of a bad URL ends: after the next `)` that isn't escaped.
fn bad_url_end(css: &str, start: usize) -> usize {
    let b = css.as_bytes();
    let mut j = start;
    while j < b.len() {
        match b[j] {
            b')' => return j + 1,
            b'\\' => j = escape_end(css, j),
            _ => j += 1,
        }
    }
    b.len()
}

/// The string whose quote is at `start`: where it ends (after its closing quote, or at the line
/// break or end that cuts it short) and its value, escapes decoded.
fn string(css: &str, start: usize) -> (usize, String) {
    let b = css.as_bytes();
    let quote = b[start];
    let mut j = start + 1;
    while j < b.len() {
        match b[j] {
            c if c == quote => return (j + 1, unescape(&css[start + 1..j])),
            b'\\' => j = escape_end(css, j),
            b'\n' | b'\r' | b'\x0c' => break,
            _ => j += 1,
        }
    }
    let j = j.min(b.len());
    (j, unescape(&css[start + 1..j]))
}

/// Where the escape whose `\` is at `at` ends: after the character it escapes (a `\r\n` counts
/// as one), or after the up to six hex digits of a code point and the one space that may end
/// them.
fn escape_end(css: &str, at: usize) -> usize {
    let b = css.as_bytes();
    let rest = &css[at + 1..];
    if rest.starts_with("\r\n") {
        return at + 3;
    }
    let hex = rest
        .bytes()
        .take(6)
        .take_while(u8::is_ascii_hexdigit)
        .count();
    if hex == 0 {
        return at + 1 + rest.chars().next().map_or(0, char::len_utf8);
    }
    let end = at + 1 + hex;
    match b.get(end) {
        Some(b'\r') if b.get(end + 1) == Some(&b'\n') => end + 2,
        Some(&c) if is_space(c) => end + 1,
        _ => end,
    }
}

/// Where the comment that starts at `at` ends: after its `*/`, or at the end of the text.
fn comment_end(b: &[u8], at: usize) -> usize {
    let mut j = at + 2;
    while j + 1 < b.len() {
        if b[j] == b'*' && b[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    b.len()
}

/// Whether `c` can start a name the scanner reads (`url`, `image-set`, `-webkit-image-set`).
fn starts_ident(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_' || c == b'-' || c >= 0x80
}

/// Where the name that starts at `start` ends.
fn ident_end(b: &[u8], start: usize) -> usize {
    let mut j = start;
    while j < b.len() && (starts_ident(b[j]) || b[j].is_ascii_digit()) {
        j += 1;
    }
    j
}

fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'\x0c')
}

fn skip_space(b: &[u8], mut j: usize) -> usize {
    while j < b.len() && is_space(b[j]) {
        j += 1;
    }
    j
}

fn skip_space_and_comments(b: &[u8], mut j: usize) -> usize {
    loop {
        j = skip_space(b, j);
        if b.get(j) == Some(&b'/') && b.get(j + 1) == Some(&b'*') {
            j = comment_end(b, j);
        } else {
            return j;
        }
    }
}
