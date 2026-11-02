//! The managed block in an instruction file, as text: finding it between its markers, putting it
//! in (in place of the one there, else at the file's top as TanStack Intent does, after a
//! frontmatter block when the file starts with one) and taking it out, with every byte outside it
//! kept. Markers are whole lines; in a fenced code block they are text, not
//! markers, so a file that shows the format isn't read as holding a block.

use std::ops::Range;

/// The line a block starts with.
pub const START: &str = "<!-- sawit:start -->";
/// The line a block ends with.
pub const END: &str = "<!-- sawit:end -->";

/// What is wrong with a file's markers: Sawit can't tell where its block ends, so it changes
/// nothing there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Damage {
    /// A start marker (at this 1-based line) with no end marker after it.
    Unclosed(usize),
    /// An end marker with no start marker before it.
    Unopened(usize),
    /// A start marker inside a block.
    Nested(usize),
}

impl Damage {
    /// What to tell the user, for the file `name`.
    pub fn describe(&self, name: &str) -> String {
        let fix = "Sawit changes nothing there: fix or delete its sawit markers, then try again.";
        match self {
            Damage::Unclosed(line) => {
                format!("{name} has a {START} marker on line {line} with no {END} after it. {fix}")
            }
            Damage::Unopened(line) => {
                format!("{name} has a {END} marker on line {line} with no {START} before it. {fix}")
            }
            Damage::Nested(line) => format!(
                "{name} has a second {START} marker on line {line}, inside the block. {fix}"
            ),
        }
    }
}

/// One line of a text: its bytes' range, with its line ending.
struct Line {
    range: Range<usize>,
    /// What a marker would read: the line without its ending and the spaces around it.
    text: String,
}

fn lines(text: &str) -> Vec<Line> {
    let mut at = 0;
    text.split_inclusive('\n')
        .map(|line| {
            let range = at..at + line.len();
            at += line.len();
            Line {
                range,
                text: line.trim().to_string(),
            }
        })
        .collect()
}

/// The fence a line opens or closes (` or ~, and how many), when it is one.
fn fence(line: &str) -> Option<(char, usize)> {
    let mark = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = line.chars().take_while(|c| *c == mark).count();
    (n >= 3).then_some((mark, n))
}

/// The blocks of `text` in order, each the byte range from its start marker's line to the end of
/// its end marker's line (its line ending included), or what is wrong with its markers.
pub fn find(text: &str) -> Result<Vec<Range<usize>>, Damage> {
    let mut blocks = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let mut fenced: Option<((char, usize), usize)> = None;
    for (ix, line) in lines(text).iter().enumerate() {
        let number = ix + 1;
        if open.is_none() {
            match (fenced, fence(&line.text)) {
                (Some(((mark, n), _)), Some((m, k))) if m == mark && k >= n => fenced = None,
                (None, Some(f)) => fenced = Some((f, number)),
                _ => {}
            }
            if fenced.is_some() || fence(&line.text).is_some() {
                continue;
            }
        }
        match (line.text.as_str(), open) {
            (START, None) => open = Some((line.range.start, number)),
            (START, Some(_)) => return Err(Damage::Nested(number)),
            (END, Some((start, _))) => {
                blocks.push(start..line.range.end);
                open = None;
            }
            (END, None) => return Err(Damage::Unopened(number)),
            _ => {}
        }
    }
    if let Some((_, line)) = open {
        return Err(Damage::Unclosed(line));
    }
    Ok(blocks)
}

/// Where a block goes in a file that has none: its top, or after the frontmatter it starts with
/// (a `---` line, then up to the next `---` or `...` line).
fn top(text: &str) -> usize {
    let lines = lines(text);
    match lines.first() {
        Some(first) if first.text == "---" => lines[1..]
            .iter()
            .find(|l| l.text == "---" || l.text == "...")
            .map_or(0, |close| close.range.end),
        _ => 0,
    }
}

/// The line ending `text` uses: CRLF when it has one, else LF.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// `block` (its lines, from [`START`] to [`END`]) with `eol` ending each line.
fn with_endings(block: &str, eol: &str) -> String {
    block.lines().map(|line| format!("{line}{eol}")).collect()
}

/// `text` with `block` in it (its lines from [`START`] to [`END`], ending in `\n` or not): in
/// place of the first block, any other block taken out; else at the top ([`top`]), a blank line
/// between it and what follows. Every byte outside the blocks stays; the block's lines end as the
/// file's do.
pub fn put(text: &str, block: &str) -> Result<String, Damage> {
    let eol = newline(text);
    let block = with_endings(block, eol);
    let blocks = find(text)?;
    let Some(first) = blocks.first() else {
        let (head, rest) = text.split_at(top(text));
        let mut out = String::with_capacity(text.len() + block.len() + 4);
        out.push_str(head);
        if !head.is_empty() && !head.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&block);
        if !rest.is_empty() {
            out.push_str(eol);
        }
        out.push_str(rest);
        return Ok(out);
    };
    let mut out = String::with_capacity(text.len() + block.len());
    out.push_str(&text[..first.start]);
    out.push_str(&block);
    let mut at = first.end;
    for later in &blocks[1..] {
        out.push_str(&text[at..later.start]);
        at = later.end;
    }
    out.push_str(&text[at..]);
    Ok(out)
}

/// `text` without its blocks, or `None` when it has none. A block at the top ([`top`]) takes the
/// blank line after it along, the one [`put`] puts there, so taking out what was put in gives the
/// file back as it was.
pub fn take(text: &str) -> Result<Option<String>, Damage> {
    let blocks = find(text)?;
    if blocks.is_empty() {
        return Ok(None);
    }
    let head = top(text);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for block in &blocks {
        out.push_str(&text[at..block.start]);
        at = block.end;
        if block.start == head
            && let Some(blank) = ["\r\n", "\n"].iter().find(|b| text[at..].starts_with(**b))
        {
            at += blank.len();
        }
    }
    out.push_str(&text[at..]);
    Ok(Some(out))
}

/// The text of `text`'s first block, its lines ending in `\n`: what to hold beside this Sawit's.
pub fn first(text: &str) -> Result<Option<String>, Damage> {
    Ok(find(text)?
        .first()
        .map(|range| text[range.clone()].replace("\r\n", "\n")))
}
