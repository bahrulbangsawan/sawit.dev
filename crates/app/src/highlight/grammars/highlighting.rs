//! One highlighting pass: a tree's captures and those of the languages injected into it, as runs.

use std::ops::Range;

use tree_sitter::{Node, Parser, QueryCursor, StreamingIterator, Tree};

use super::{Fallback, Grammar, Highlights, Injections, grammar, parse};

/// How deep injections nest: Markdown, the HTML in it, the script in that.
const MAX_INJECTION_DEPTH: u8 = 3;
/// The most injected parses one pass makes. A Markdown file is one per paragraph, list item
/// and heading, so this is about a 20,000-line document; past it, text keeps its host colors.
const MAX_INJECTION_LAYERS: usize = 10_000;
/// How many captures pass between two looks at whether the caller wants the pass stopped.
const STOP_CHECK_EVERY: usize = 1024;

impl Grammar {
    /// The highlight names of `text` inside `range`, from `tree` (its parse) and from the
    /// languages injected into it (a Markdown fence, an HTML `<script>`, a Rust macro's body).
    pub(crate) fn highlight(
        &self,
        tree: &Tree,
        text: &str,
        range: Range<usize>,
        fallback: &Fallback,
        stop: &dyn Fn() -> bool,
    ) -> Highlights {
        let mut spans = Vec::new();
        let mut pass = Pass {
            text,
            parser: None,
            fallback,
            stop,
            layers: 0,
        };
        self.collect(tree.root_node(), &range, 0, &mut spans, &mut pass);
        flatten(spans, &range)
    }

    /// Adds the spans `root` (a tree of this grammar) captures inside `range`, then those of
    /// what is injected into it.
    fn collect(
        &self,
        root: Node,
        range: &Range<usize>,
        depth: u8,
        spans: &mut Vec<Span>,
        pass: &mut Pass,
    ) {
        let Some(compiled) = self.compiled() else {
            return;
        };
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());
        let mut captures = cursor.captures(&compiled.highlights, root, pass.text.as_bytes());
        // The spans of the nodes that start where the current capture does, so that another
        // pattern capturing one of them renames its span rather than adding a second one (or,
        // with `first_capture_wins`, leaves it).
        let mut start = usize::MAX;
        let mut here: Vec<(usize, usize)> = Vec::new();
        let mut seen = 0;
        while let Some((found, index)) = captures.next() {
            seen += 1;
            if seen % STOP_CHECK_EVERY == 0 && (pass.stop)() {
                return;
            }
            let capture = found.captures[*index];
            let Some(name) = compiled.names[capture.index as usize] else {
                continue;
            };
            let node = capture.node;
            let name = refine(name, node.kind());
            if node.start_byte() != start {
                start = node.start_byte();
                here.clear();
            }
            match here.iter().find(|(id, _)| *id == node.id()) {
                Some(&(_, at)) => {
                    if !self.first_capture_wins {
                        spans[at].name = name;
                    }
                }
                None => {
                    here.push((node.id(), spans.len()));
                    spans.push(Span {
                        range: node.byte_range(),
                        name,
                        depth,
                    });
                }
            }
        }
        if depth < MAX_INJECTION_DEPTH
            && let Some(injections) = &compiled.injections
        {
            inject(injections, root, range, depth, spans, pass);
        }
    }

    /// Parses `ranges` of the text as this language and adds the spans its tree captures inside
    /// `range`.
    fn layer(
        &self,
        ranges: &[tree_sitter::Range],
        range: &Range<usize>,
        depth: u8,
        spans: &mut Vec<Span>,
        pass: &mut Pass,
    ) {
        let (Some(first), Some(last)) = (ranges.first(), ranges.last()) else {
            return;
        };
        let bounds = first.start_byte.max(range.start)..last.end_byte.min(range.end);
        if bounds.is_empty() || pass.layers >= MAX_INJECTION_LAYERS {
            return;
        }
        pass.layers += 1;
        let parser = pass.parser.get_or_insert_with(Parser::new);
        if parser.set_language(&(self.language)()).is_err()
            || parser.set_included_ranges(ranges).is_err()
        {
            return;
        }
        let Some(tree) = parse(parser, pass.text, None, pass.stop) else {
            return;
        };
        self.collect(tree.root_node(), &bounds, depth, spans, pass);
    }
}

/// One highlighting pass over a document and what it injects.
struct Pass<'a> {
    text: &'a str,
    /// Parses the injected layers, one after another; made for the first.
    parser: Option<Parser>,
    fallback: &'a Fallback,
    stop: &'a dyn Fn() -> bool,
    layers: usize,
}

/// A captured node and its highlight name, before nested captures are flattened.
struct Span {
    range: Range<usize>,
    name: &'static str,
    /// 0 for the document's own language, one more for each injection it sits in.
    depth: u8,
}

/// Adds the spans of the languages `injections` finds in `root` inside `range`.
fn inject(
    injections: &Injections,
    root: Node,
    range: &Range<usize>,
    depth: u8,
    spans: &mut Vec<Span>,
    pass: &mut Pass,
) {
    let text = pass.text;
    let mut cursor = QueryCursor::new();
    cursor.set_byte_range(range.clone());
    let mut matches = cursor.matches(&injections.query, root, text.as_bytes());
    // Matches marked `injection.combined` parse as one document per language, like the pieces
    // of a tagged template literal.
    let mut combined: Vec<(&'static Grammar, Vec<tree_sitter::Range>)> = Vec::new();
    let mut found = Vec::new();
    while let Some(matched) = matches.next() {
        let mut language = None;
        let mut content = None;
        for capture in matched.captures {
            if capture.index == injections.content {
                content = Some(capture.node);
            } else if Some(capture.index) == injections.language {
                language = capture.node.utf8_text(text.as_bytes()).ok();
            }
        }
        let mut whole = false;
        let mut combine = false;
        for property in injections.query.property_settings(matched.pattern_index) {
            match property.key.as_ref() {
                "injection.language" if language.is_none() => {
                    language = property.value.as_deref();
                }
                "injection.include-children" => whole = true,
                "injection.combined" => combine = true,
                _ => {}
            }
        }
        if let (Some(language), Some(content)) = (language, content) {
            found.push((language, content, whole, combine));
        }
    }
    drop(matches);
    for (language, content, whole, combine) in found {
        let ranges = content_ranges(content, whole);
        if ranges.is_empty() {
            continue;
        }
        match grammar(language) {
            Some(grammar) if combine => match combined.iter_mut().find(|(g, _)| *g == grammar) {
                Some((_, all)) => all.extend(ranges),
                None => combined.push((grammar, ranges)),
            },
            Some(grammar) => grammar.layer(&ranges, range, depth + 1, spans, pass),
            None if !combine => {
                let at = content.start_byte();
                let code = text.get(content.byte_range()).unwrap_or_default();
                let Some(highlights) = (pass.fallback)(language, code) else {
                    continue;
                };
                spans.extend(highlights.into_iter().map(|(span, name)| Span {
                    range: span.start + at..span.end + at,
                    name,
                    depth: depth + 1,
                }));
            }
            None => {}
        }
    }
    for (grammar, mut ranges) in combined {
        ranges.sort_by_key(|range| range.start_byte);
        ranges.dedup_by(|next, kept| next.start_byte < kept.end_byte);
        grammar.layer(&ranges, range, depth + 1, spans, pass);
    }
}

/// The parts of `node` an injection parses: all of it when the query says
/// `injection.include-children`, else the text between its named children. Its anonymous
/// children stay in: Markdown's block grammar lexes the `*` and `` ` `` of a paragraph as
/// tokens of its own, and the inline grammar has to see them (as in tree-sitter-md's own
/// parser).
fn content_ranges(node: Node, whole: bool) -> Vec<tree_sitter::Range> {
    let all = node.range();
    if whole {
        return vec![all];
    }
    let mut ranges = Vec::new();
    let (mut start_byte, mut start_point) = (all.start_byte, all.start_point);
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let child = child.range();
        if child.start_byte > start_byte {
            ranges.push(tree_sitter::Range {
                start_byte,
                start_point,
                end_byte: child.start_byte,
                end_point: child.start_point,
            });
        }
        (start_byte, start_point) = (child.end_byte, child.end_point);
    }
    if all.end_byte > start_byte {
        ranges.push(tree_sitter::Range {
            start_byte,
            start_point,
            end_byte: all.end_byte,
            end_point: all.end_point,
        });
    }
    ranges
}

/// Turns nested captures into ordered, non-overlapping runs inside `range`: the inner capture
/// colors its text, the one around it the rest. An injected language sits inside its host.
fn flatten(mut spans: Vec<Span>, range: &Range<usize>) -> Highlights {
    // Outer spans first; for the same text, the host before what is injected into it, then
    // capture order, so what comes later paints over what came before.
    spans.sort_by(|a, b| {
        (a.range.start, b.range.end, a.depth).cmp(&(b.range.start, a.range.end, b.depth))
    });
    let mut out = Highlights::new();
    let push = |out: &mut Highlights, from: usize, to: usize, name: &'static str| {
        if from >= to {
            return;
        }
        match out.last_mut() {
            Some((last, last_name)) if last.end == from && *last_name == name => last.end = to,
            _ => out.push((from..to, name)),
        }
    };
    // The spans around the text at `at`, innermost last, each cut at the end of the one it is in.
    let mut open: Vec<(usize, &'static str)> = Vec::new();
    let mut at = range.start;
    for span in spans {
        let start = span.range.start.max(range.start);
        while let Some(&(end, name)) = open.last()
            && end <= start
        {
            push(&mut out, at, end, name);
            at = at.max(end);
            open.pop();
        }
        if let Some(&(_, name)) = open.last() {
            push(&mut out, at, start, name);
        }
        at = at.max(start);
        let end = span
            .range
            .end
            .min(range.end)
            .min(open.last().map_or(usize::MAX, |&(end, _)| end));
        if end > start {
            open.push((end, span.name));
        }
    }
    while let Some((end, name)) = open.pop() {
        push(&mut out, at, end, name);
        at = at.max(end);
    }
    out
}

/// The parts of `text` whose colors may differ between `old` (edited to match `text`) and `new`
/// (its reparse): where the trees differ, and the edits themselves, as whole lines, merged.
/// `None` when that comes to more than `limit` bytes, which is left to a full pass.
pub(crate) fn changed_lines(
    old: &Tree,
    new: &Tree,
    edits: &[Range<usize>],
    text: &str,
    limit: usize,
) -> Option<Vec<Range<usize>>> {
    // Bytes, not `str` slices: a line break is one byte, so its neighbours are char boundaries
    // whatever offset the search starts from.
    let bytes = text.as_bytes();
    let line_start = |at: usize| {
        bytes[..at.min(bytes.len())]
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |i| i + 1)
    };
    let line_end = |at: usize| {
        let at = at.min(bytes.len());
        bytes[at..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |i| at + i + 1)
    };
    let mut ranges: Vec<Range<usize>> = old
        .changed_ranges(new)
        .map(|changed| changed.start_byte..changed.end_byte)
        .chain(edits.iter().cloned())
        .map(|range| line_start(range.start)..line_end(range.end.max(range.start)))
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => merged.push(range),
        }
    }
    let total: usize = merged.iter().map(|range| range.len()).sum();
    (total <= limit).then_some(merged)
}

/// A name the node's kind makes more precise: literals some grammars capture as builtin
/// constants (Rust's `true` and `4`) are booleans and numbers, a JavaScript regex captured as a
/// special string is a regex.
fn refine(name: &'static str, kind: &str) -> &'static str {
    match kind {
        "true" | "false" | "boolean" | "boolean_literal" if name == "constant" => "boolean",
        "integer_literal" | "float_literal" if name == "constant" => "number",
        "regex" if name == "constant" || name == "string" => "string.regex",
        _ => name,
    }
}
