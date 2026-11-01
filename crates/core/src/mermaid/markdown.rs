//! Mermaid fences in Markdown, and the document with each drawn as an image or its error.

use std::ops::Range;

use super::Diagram;

// --- Markdown --------------------------------------------------------------

/// A fenced code block tagged `mermaid` in a Markdown document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    /// Byte range of the whole block, fences included.
    pub range: Range<usize>,
    pub source: String,
}

/// Top-level ```` ```mermaid ```` / `~~~mermaid` blocks, in order. Fences
/// inside other code blocks are skipped; an unclosed block runs to the end.
pub fn fences(markdown: &str) -> Vec<Fence> {
    struct Open {
        start: usize,
        marker: char,
        len: usize,
        mermaid: bool,
        body: usize,
    }
    let mut found = Vec::new();
    let mut open: Option<Open> = None;
    let mut offset = 0;
    for line in markdown.split_inclusive('\n') {
        let line_start = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let indent = text.len() - text.trim_start_matches(' ').len();
        let trimmed = &text[indent..];
        let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'));
        let run = marker.map_or(0, |m| trimmed.len() - trimmed.trim_start_matches(m).len());
        let fence = marker.filter(|_| indent <= 3 && run >= 3);
        match (&open, fence) {
            (None, Some(marker)) => {
                let info = trimmed[run..].trim();
                // Not a fence: a backtick fence's info string has no backticks.
                if marker == '`' && info.contains('`') {
                    continue;
                }
                let lang = info.split_whitespace().next().unwrap_or_default();
                open = Some(Open {
                    start: line_start,
                    marker,
                    len: run,
                    mermaid: lang.eq_ignore_ascii_case("mermaid"),
                    body: offset,
                });
            }
            (Some(o), Some(marker))
                if marker == o.marker && run >= o.len && trimmed[run..].trim().is_empty() =>
            {
                if o.mermaid {
                    found.push(Fence {
                        range: o.start..offset,
                        source: markdown[o.body..line_start].to_string(),
                    });
                }
                open = None;
            }
            _ => {}
        }
    }
    if let Some(o) = open.filter(|o| o.mermaid) {
        found.push(Fence {
            range: o.start..markdown.len(),
            source: markdown[o.body..].to_string(),
        });
    }
    found
}

/// Image URL prefix for diagram `<index>` of a rewritten document.
pub const IMAGE_SCHEME: &str = "sawit-mermaid:";
/// Link URL prefix that saves diagram `<index>` as `.svg`.
pub const SAVE_SCHEME: &str = "sawit-mermaid-save:";

/// Replaces each fence with the diagram image (`sawit-mermaid:<index>`) and
/// a "Save Diagram as SVG…" link (`sawit-mermaid-save:<index>`), or with Pele's
/// error when it couldn't be drawn. `results` lines up with `fences`.
pub fn rewrite_markdown(
    markdown: &str,
    fences: &[Fence],
    results: &[Result<Diagram, String>],
) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut copied = 0;
    for (ix, (fence, result)) in fences.iter().zip(results).enumerate() {
        out.push_str(&markdown[copied..fence.range.start]);
        match result {
            Ok(_) => out.push_str(&format!(
                "![Diagram]({IMAGE_SCHEME}{ix})\n\n[Save Diagram as SVG…]({SAVE_SCHEME}{ix})\n"
            )),
            Err(e) => {
                let ticks = "`".repeat(longest_run(e, '`').max(2) + 1);
                out.push_str(&format!(
                    "**The diagram can't be drawn:**\n\n{ticks}text\n{}\n{ticks}\n",
                    e.trim_end()
                ));
            }
        }
        out.push('\n');
        copied = fence.range.end;
    }
    out.push_str(&markdown[copied..]);
    out
}

fn longest_run(s: &str, c: char) -> usize {
    s.split(|x| x != c).map(str::len).max().unwrap_or(0)
}

/// The diagram index in a `sawit-mermaid:` / `sawit-mermaid-save:` URL.
pub fn index_in(url: &str, scheme: &str) -> Option<usize> {
    url.strip_prefix(scheme)?.parse().ok()
}
