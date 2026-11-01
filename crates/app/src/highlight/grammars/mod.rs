//! Tree-sitter, the parser behind the code editors' colors and folds.
//!
//! Each grammar Sawit ships brings its own `highlights.scm` and `injections.scm`, compiled once,
//! off the UI thread, the first time they are needed (`super::prewarm` compiles them all at
//! startup). The names those queries capture (`@keyword`, `@function.method`, `@text.title`) map
//! to the highlight names the theme colors, the same names the syntect backend in `super`
//! produces, so a language keeps its colors whichever backend draws it.
//!
//! Nothing here touches GPUI: it is plain computation over a `&str`, run on whichever thread the
//! caller picks.

use std::ops::{ControlFlow, Range};
use std::sync::OnceLock;

use tree_sitter::{
    Language, Node, ParseOptions, ParseState, Parser, Query, QueryCursor, StreamingIterator, Tree,
};

/// Highlight names, ordered and non-overlapping.
pub(crate) type Highlights = Vec<(Range<usize>, &'static str)>;

/// Colors injected code that no grammar here parses, given its language name and its text: the
/// syntect backend, so a Markdown fence in a language without a grammar keeps its colors.
pub(crate) type Fallback = dyn Fn(&str, &str) -> Option<Highlights>;

/// How deep injections nest: Markdown, the HTML in it, the script in that.
const MAX_INJECTION_DEPTH: u8 = 3;
/// The most injected parses one pass makes. A Markdown file is one per paragraph, list item
/// and heading, so this is about a 20,000-line document; past it, text keeps its host colors.
const MAX_INJECTION_LAYERS: usize = 10_000;
/// How many captures pass between two looks at whether the caller wants the pass stopped.
const STOP_CHECK_EVERY: usize = 1024;

/// A language Sawit parses with tree-sitter, and the queries that come with its grammar.
pub(crate) struct Grammar {
    /// The language token the editors use (`sawit_core::files::language_for`, "sql", "markdown").
    pub(crate) name: &'static str,
    /// Other names for it: file extensions and the names Markdown fences use.
    aliases: &'static [&'static str],
    language: fn() -> Language,
    /// The highlight queries, joined in this order. As in tree-sitter's own highlighter, the
    /// last pattern that captures a node names it, so a grammar that builds on another
    /// (TypeScript on JavaScript) lists the base first.
    highlights: &'static [&'static str],
    injections: &'static str,
    /// Whether the first pattern that captures a node names it instead: Go's and JSON's queries
    /// still list the specific patterns (a method call, an object key) before the catch-all
    /// ones (any field, any string), as tree-sitter's highlighter once read them.
    first_capture_wins: bool,
    compiled: OnceLock<Option<Compiled>>,
}

struct Compiled {
    highlights: Query,
    /// The theme's highlight name for each capture of `highlights`, by capture index.
    names: Vec<Option<&'static str>>,
    injections: Option<Injections>,
}

struct Injections {
    query: Query,
    content: u32,
    language: Option<u32>,
}

impl Grammar {
    const fn new(
        name: &'static str,
        aliases: &'static [&'static str],
        language: fn() -> Language,
        highlights: &'static [&'static str],
        injections: &'static str,
    ) -> Self {
        Self {
            name,
            aliases,
            language,
            highlights,
            injections,
            first_capture_wins: false,
            compiled: OnceLock::new(),
        }
    }

    const fn first_capture_wins(mut self) -> Self {
        self.first_capture_wins = true;
        self
    }
}

/// Every grammar Sawit ships. The big, rarely opened ones (C++ 3.4 MiB, Swift 4.1 MiB, Ruby
/// 2.1 MiB and PHP 1.1 MiB in a release build) are left to syntect.
static GRAMMARS: [Grammar; 17] = [
    Grammar::new(
        "rust",
        &["rs"],
        || tree_sitter_rust::LANGUAGE.into(),
        &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        tree_sitter_rust::INJECTIONS_QUERY,
    ),
    Grammar::new(
        "typescript",
        &["ts", "mts", "cts"],
        || tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
        tree_sitter_javascript::INJECTIONS_QUERY,
    ),
    Grammar::new(
        "tsx",
        &[],
        || tree_sitter_typescript::LANGUAGE_TSX.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
        ],
        tree_sitter_javascript::INJECTIONS_QUERY,
    ),
    Grammar::new(
        "javascript",
        &["js", "jsx", "mjs", "cjs"],
        || tree_sitter_javascript::LANGUAGE.into(),
        &[
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
        ],
        tree_sitter_javascript::INJECTIONS_QUERY,
    ),
    Grammar::new(
        "json",
        &["jsonc"],
        || tree_sitter_json::LANGUAGE.into(),
        &[tree_sitter_json::HIGHLIGHTS_QUERY],
        "",
    )
    .first_capture_wins(),
    Grammar::new(
        "python",
        &["py", "pyi", "python3"],
        || tree_sitter_python::LANGUAGE.into(),
        &[tree_sitter_python::HIGHLIGHTS_QUERY],
        "",
    ),
    Grammar::new(
        "go",
        &["golang"],
        || tree_sitter_go::LANGUAGE.into(),
        &[tree_sitter_go::HIGHLIGHTS_QUERY],
        "",
    )
    .first_capture_wins(),
    Grammar::new(
        "bash",
        &["sh", "shell", "zsh", "shellscript"],
        || tree_sitter_bash::LANGUAGE.into(),
        &[tree_sitter_bash::HIGHLIGHT_QUERY],
        "",
    ),
    Grammar::new(
        "toml",
        &[],
        || tree_sitter_toml_ng::LANGUAGE.into(),
        &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        "",
    ),
    Grammar::new(
        "yaml",
        &["yml"],
        || tree_sitter_yaml::LANGUAGE.into(),
        &[tree_sitter_yaml::HIGHLIGHTS_QUERY],
        "",
    ),
    Grammar::new(
        "markdown",
        &["md"],
        || tree_sitter_md::LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        tree_sitter_md::INJECTION_QUERY_BLOCK,
    ),
    // Only reached through Markdown's injections: each paragraph, heading and list item.
    Grammar::new(
        "markdown_inline",
        &[],
        || tree_sitter_md::INLINE_LANGUAGE.into(),
        &[tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        tree_sitter_md::INJECTION_QUERY_INLINE,
    ),
    Grammar::new(
        "html",
        &["htm"],
        || tree_sitter_html::LANGUAGE.into(),
        &[tree_sitter_html::HIGHLIGHTS_QUERY],
        tree_sitter_html::INJECTIONS_QUERY,
    ),
    Grammar::new(
        "css",
        &[],
        || tree_sitter_css::LANGUAGE.into(),
        &[tree_sitter_css::HIGHLIGHTS_QUERY],
        "",
    ),
    Grammar::new(
        "sql",
        &[
            "psql",
            "pgsql",
            "postgres",
            "postgresql",
            "plpgsql",
            "mysql",
            "sqlite",
        ],
        || tree_sitter_sequel::LANGUAGE.into(),
        &[tree_sitter_sequel::HIGHLIGHTS_QUERY],
        "",
    ),
    Grammar::new(
        "c",
        &["h"],
        || tree_sitter_c::LANGUAGE.into(),
        &[tree_sitter_c::HIGHLIGHT_QUERY],
        "",
    ),
    Grammar::new(
        "java",
        &[],
        || tree_sitter_java::LANGUAGE.into(),
        &[tree_sitter_java::HIGHLIGHTS_QUERY],
        "",
    ),
];

/// The grammar for a language token or one of its aliases, any case.
pub(crate) fn grammar(language: &str) -> Option<&'static Grammar> {
    let lower = language.trim().to_ascii_lowercase();
    GRAMMARS
        .iter()
        .find(|grammar| grammar.name == lower || grammar.aliases.contains(&lower.as_str()))
}

#[cfg(test)]
fn names() -> impl Iterator<Item = &'static str> {
    GRAMMARS.iter().map(|grammar| grammar.name)
}

/// Compiles every grammar's queries: tens of milliseconds for the big ones, so `super::prewarm`
/// does it on the background executor at startup.
pub(crate) fn compile_all() {
    for grammar in &GRAMMARS {
        grammar.compiled();
    }
}

/// Whether every grammar's queries are compiled, so a parse never compiles one on the UI thread.
pub(crate) fn ready() -> bool {
    GRAMMARS
        .iter()
        .all(|grammar| grammar.compiled.get().is_some())
}

impl Grammar {
    fn compiled(&self) -> Option<&Compiled> {
        self.compiled
            .get_or_init(|| match self.compile() {
                Ok(compiled) => Some(compiled),
                Err(error) => {
                    log::warn!("highlight: {}'s queries don't compile: {error}", self.name);
                    None
                }
            })
            .as_ref()
    }

    fn compile(&self) -> Result<Compiled, tree_sitter::QueryError> {
        let language = (self.language)();
        let mut source = self.highlights.join("\n");
        if self.name == "sql" {
            // Its query matches numbers with Lua's `%d`, which tree-sitter's regex reads as a
            // literal "%d": every number would color as a string.
            source = source.replace("%d", "\\\\d");
        }
        let highlights = Query::new(&language, &source)?;
        let names = highlights
            .capture_names()
            .iter()
            .map(|name| theme_name(name))
            .collect();
        let injections = if self.injections.is_empty() {
            None
        } else {
            let query = Query::new(&language, self.injections)?;
            query
                .capture_index_for_name("injection.content")
                .map(|content| Injections {
                    language: query.capture_index_for_name("injection.language"),
                    content,
                    query,
                })
        };
        Ok(Compiled {
            highlights,
            names,
            injections,
        })
    }

    /// Parses `text`, reusing `old` (edited to match it) where it still applies. `stop` is asked
    /// now and then; a parse it stops gives `None`.
    pub(crate) fn parse(
        &self,
        parser: &mut Parser,
        text: &str,
        old: Option<&Tree>,
        stop: &dyn Fn() -> bool,
    ) -> Option<Tree> {
        parser.set_language(&(self.language)()).ok()?;
        parser.set_included_ranges(&[]).ok()?;
        parse(parser, text, old, stop)
    }

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

fn parse(
    parser: &mut Parser,
    text: &str,
    old: Option<&Tree>,
    stop: &dyn Fn() -> bool,
) -> Option<Tree> {
    let bytes = text.as_bytes();
    let mut progress = |_: &ParseState| {
        if stop() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let tree = parser.parse_with_options(
        &mut |at, _| bytes.get(at..).unwrap_or_default(),
        old,
        Some(ParseOptions::new().progress_callback(&mut progress)),
    );
    if tree.is_none() {
        // A stopped parse would otherwise resume on the next call, with other text.
        parser.reset();
    }
    tree
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

impl PartialEq for Grammar {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
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

/// Fold candidates as `(start line, end line)`, read from the tree: every node that spans more
/// than its first line, from the line it starts on to its last line when that line closes it
/// (`}`, `]`, `end`, `</div>`), else to the line after (a Python body, a Markdown section), so
/// folding hides everything between. Only nodes that touch `range` are visited.
pub(crate) fn fold_lines(tree: &Tree, text: &str, range: Range<usize>) -> Vec<(usize, usize)> {
    fn visit(node: Node, text: &str, range: &Range<usize>, out: &mut Vec<(usize, usize)>) {
        if node.end_byte() < range.start || node.start_byte() > range.end {
            return;
        }
        let start = node.start_position().row;
        // A node on one line has nothing inside it that spans lines either.
        if last_row(node) == start {
            return;
        }
        if !wraps_first_child(node) {
            let end = fold_end(node, text);
            if end >= start + 2 {
                out.push((start, end));
            }
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            visit(child, text, range, out);
        }
    }
    let mut out = Vec::new();
    let root = tree.root_node();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        visit(child, text, &range, &mut out);
    }
    // Outer nodes come first, so the line keeps the widest fold that starts on it.
    out.sort_by_key(|&(start, _)| start);
    out.dedup_by_key(|&mut (start, _)| start);
    out
}

/// Whether `node` starts where its first named child does and that child spans lines too, like
/// a Python body that starts at its first statement: the child folds instead.
fn wraps_first_child(node: Node) -> bool {
    node.named_child(0).is_some_and(|child| {
        child.start_byte() == node.start_byte() && last_row(child) > child.start_position().row
    })
}

/// The last line with text of `node`: a Markdown heading ends after its line break, at the
/// start of the next line, and still sits on one line.
fn last_row(node: Node) -> usize {
    let end = node.end_position();
    if end.column == 0 && end.row > node.start_position().row {
        end.row - 1
    } else {
        end.row
    }
}

fn fold_end(node: Node, text: &str) -> usize {
    let end = node.end_position();
    if end.column == 0 {
        // The node ends with its last line's break.
        return end.row;
    }
    let line = text
        .get(node.end_byte() - end.column..node.end_byte())
        .unwrap_or_default()
        .trim_start();
    if closes(line) { end.row } else { end.row + 1 }
}

/// Whether a block's last line starts with what closes it.
fn closes(line: &str) -> bool {
    const SYMBOLS: [&str; 10] = [
        "}", "]", ")", "</", "*/", "-->", "```", "~~~", "\"\"\"", "'''",
    ];
    const WORDS: [&str; 4] = ["end", "fi", "done", "esac"];
    SYMBOLS.iter().any(|symbol| line.starts_with(symbol))
        || WORDS.iter().any(|word| {
            line.strip_prefix(word)
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
        })
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

/// The theme's highlight name for a capture of a grammar's `highlights.scm`, or `None` for what
/// it doesn't color (`@none`, `@spell`, helper captures). The names are the ones the syntect
/// backend produces (see `super::semantic_name`), so both color a language alike.
fn theme_name(capture: &str) -> Option<&'static str> {
    let name = match capture {
        "comment.documentation" | "comment.doc" => "comment.doc",
        "string.escape" | "escape" | "character.special" => "string.escape",
        "string.regex" | "string.regexp" | "string.special.regex" => "string.regex",
        // Map keys (JSON) and record fields (SQL, Swift) are properties.
        "string.special.key" | "field" | "variable.member" => "property",
        // Ruby's `:symbol`, TOML's dates and CSS's `#fff` are constants, as in syntect.
        "string.special" | "string.special.symbol" => "constant",
        "float" | "number.float" => "number",
        // `self`, `this`, `super`.
        "variable.builtin" => "variable.special",
        // SQL's AND, OR, NOT.
        "keyword.operator" => "operator",
        "keyword.directive" => "preproc",
        "type.qualifier" => "keyword",
        "punctuation.bracket" => "punctuation.bracket",
        "punctuation.delimiter" | "delimiter" => "punctuation.delimiter",
        // Markdown, by its older (`text.*`) and newer (`markup.*`) names.
        "text.title" | "markup.heading" => "title",
        "text.literal" | "markup.raw" => "text.literal",
        "text.uri" | "markup.link.url" => "link_uri",
        "text.reference" | "markup.link.label" | "markup.link.text" => "link_text",
        "text.emphasis" | "markup.italic" => "emphasis",
        "text.strong" | "markup.strong" => "emphasis.strong",
        "markup.list" => "punctuation.list_marker",
        _ => match capture.split('.').next().unwrap_or_default() {
            "comment" => "comment",
            "string" | "character" => "string",
            "number" => "number",
            "boolean" => "boolean",
            "constant" => "constant",
            "constructor" => "constructor",
            "operator" => "operator",
            "keyword" | "conditional" | "repeat" | "storageclass" | "include" | "exception" => {
                "keyword"
            }
            "function" | "method" => "function",
            "type" | "module" => "type",
            "tag" => "tag",
            "attribute" => "attribute",
            "label" => "label",
            "property" => "property",
            "variable" | "parameter" => "variable",
            "punctuation" => "punctuation",
            "embedded" => "embedded",
            _ => return None,
        },
    };
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_fallback(_: &str, _: &str) -> Option<Highlights> {
        None
    }

    fn never() -> bool {
        false
    }

    fn parse_new(language: &str, text: &str) -> (&'static Grammar, Tree) {
        let grammar = grammar(language).unwrap_or_else(|| panic!("no grammar for {language}"));
        let tree = grammar
            .parse(&mut Parser::new(), text, None, &never)
            .unwrap();
        (grammar, tree)
    }

    fn highlights(language: &str, text: &str) -> Highlights {
        let (grammar, tree) = parse_new(language, text);
        grammar.highlight(&tree, text, 0..text.len(), &no_fallback, &never)
    }

    /// The highlight name at the first byte of `needle` in `text`, which must be in it.
    fn name_at(highlights: &Highlights, text: &str, needle: &str) -> Option<&'static str> {
        let at = text
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} isn't in {text:?}"));
        highlights
            .iter()
            .find(|(range, _)| range.contains(&at))
            .map(|(_, name)| *name)
    }

    /// The highlight name of every byte: equal however the runs are split.
    fn per_byte(highlights: &Highlights, len: usize) -> Vec<Option<&'static str>> {
        let mut names = vec![None; len];
        for (range, name) in highlights {
            for slot in &mut names[range.clone()] {
                *slot = Some(*name);
            }
        }
        names
    }

    fn assert_colors(language: &str, text: &str, expected: &[(&str, &str)]) {
        let highlights = highlights(language, text);
        for (needle, name) in expected {
            assert_eq!(
                name_at(&highlights, text, needle),
                Some(*name),
                "{language}: {needle:?} in {highlights:?}"
            );
        }
        // Ordered and non-overlapping, as `resolve_styles` needs.
        for pair in highlights.windows(2) {
            assert!(pair[0].0.end <= pair[1].0.start, "{language}: {pair:?}");
        }
    }

    #[test]
    fn every_grammar_compiles_its_queries() {
        for name in names() {
            let grammar = grammar(name).unwrap();
            assert!(grammar.compiled().is_some(), "{name}");
        }
        assert!(ready());
    }

    #[test]
    fn aliases_find_their_grammar() {
        for (alias, name) in [
            ("rs", "rust"),
            ("TS", "typescript"),
            ("jsx", "javascript"),
            ("zsh", "bash"),
            ("yml", "yaml"),
            ("md", "markdown"),
            ("psql", "sql"),
            ("py", "python"),
        ] {
            assert_eq!(grammar(alias).map(|g| g.name), Some(name), "{alias}");
        }
        for unknown in ["text", "kotlin", "nim", "", "cpp", "swift"] {
            assert!(grammar(unknown).is_none(), "{unknown}");
        }
    }

    #[test]
    fn rust_colors() {
        assert_colors(
            "rust",
            "/// Docs.\nfn main() {\n    let s = \"hi\\n\"; // note\n    let n: u32 = 4;\n    println!(\"{s}\");\n    self.go(true);\n}\n",
            &[
                ("/// Docs", "comment.doc"),
                ("fn", "keyword"),
                ("main", "function"),
                ("\"hi", "string"),
                ("\\n", "string.escape"),
                ("// note", "comment"),
                ("u32", "type"),
                ("4", "number"),
                ("println", "function"),
                ("self", "variable.special"),
                ("go", "function"),
                ("true", "boolean"),
            ],
        );
    }

    #[test]
    fn typescript_colors() {
        assert_colors(
            "typescript",
            "interface Point { x: number }\nexport const p: Point = { x: 1 };\nfunction f(a: string) { return `${a}`; }\n",
            &[
                ("interface", "keyword"),
                ("Point", "type"),
                ("number", "type"),
                ("const", "keyword"),
                ("1", "number"),
                ("f(", "function"),
                ("`", "string"),
            ],
        );
    }

    #[test]
    fn tsx_colors() {
        assert_colors(
            "tsx",
            "const el = <div className=\"a\">{count}</div>;\n",
            &[
                ("const", "keyword"),
                ("div", "tag"),
                ("className", "attribute"),
                ("\"a\"", "string"),
            ],
        );
    }

    #[test]
    fn javascript_colors() {
        assert_colors(
            "javascript",
            "import x from \"y\";\nfunction f() { return /ab+c/.test(null) ? <A /> : this; }\n",
            &[
                ("import", "keyword"),
                ("\"y\"", "string"),
                ("function", "keyword"),
                ("f(", "function"),
                ("ab+c", "string.regex"),
                ("null", "constant"),
                ("this", "variable.special"),
            ],
        );
    }

    #[test]
    fn json_colors() {
        assert_colors(
            "json",
            "{\"a\": [1, true, null, \"s\"]}\n",
            &[
                ("\"a\"", "property"),
                ("1", "number"),
                ("true", "boolean"),
                ("null", "constant"),
                ("\"s\"", "string"),
            ],
        );
    }

    #[test]
    fn python_colors() {
        assert_colors(
            "python",
            "def f(x):\n    return \"s\"  # c\n\nclass Point:\n    MAX = None\n",
            &[
                ("def", "keyword"),
                ("f(", "function"),
                ("\"s\"", "string"),
                ("# c", "comment"),
                ("class", "keyword"),
                ("Point", "constructor"),
                ("MAX", "constant"),
                ("None", "constant"),
            ],
        );
    }

    #[test]
    fn go_colors() {
        assert_colors(
            "go",
            "package main\n\nfunc main() {\n\tfmt.Println(\"x\", 1)\n}\n",
            &[
                ("package", "keyword"),
                ("func", "keyword"),
                ("Println", "function"),
                ("\"x\"", "string"),
                ("1", "number"),
            ],
        );
    }

    #[test]
    fn bash_colors() {
        assert_colors(
            "bash",
            "# c\nexport PATH=\"$HOME/bin\"\nif [ -f x ]; then echo hi; fi\n",
            &[
                ("# c", "comment"),
                ("export", "keyword"),
                ("\"$HOME", "string"),
                ("if", "keyword"),
                ("echo", "function"),
            ],
        );
    }

    #[test]
    fn toml_colors() {
        assert_colors(
            "toml",
            "[package]\nname = \"x\"\nn = 1\nok = true # c\na.b = 2\n",
            // tree-sitter-toml colors keys as types, a dotted key's parts as properties.
            &[
                ("package", "type"),
                ("name", "type"),
                ("b = 2", "property"),
                ("\"x\"", "string"),
                ("1", "number"),
                ("true", "boolean"),
                ("# c", "comment"),
            ],
        );
    }

    #[test]
    fn yaml_colors() {
        assert_colors(
            "yaml",
            "key: value\nn: 1\nb: true\n# c\n",
            &[
                ("key", "property"),
                ("1", "number"),
                ("true", "boolean"),
                ("# c", "comment"),
            ],
        );
    }

    #[test]
    fn markdown_colors_its_inline_text_and_fenced_code() {
        assert_colors(
            "markdown",
            "# Title\n\nSome *em*, **strong**, `code` and [a link](http://x).\n\n```rust\nfn x() {}\n```\n",
            &[
                ("Title", "title"),
                ("em*", "emphasis"),
                ("strong**", "emphasis.strong"),
                ("a link", "link_text"),
                ("http://x", "link_uri"),
                ("fn x", "keyword"),
            ],
        );
    }

    #[test]
    fn html_colors_its_scripts_and_styles() {
        assert_colors(
            "html",
            "<div class=\"a\"><script>let x = 1;</script><style>p { color: red; }</style></div>\n",
            &[
                ("div", "tag"),
                ("class", "attribute"),
                ("a\">", "string"),
                ("let", "keyword"),
                ("color", "property"),
            ],
        );
    }

    #[test]
    fn css_colors() {
        assert_colors(
            "css",
            ".a { color: red; margin: 0; }\n@media screen { p {} }\n",
            &[
                ("color", "property"),
                ("0;", "number"),
                ("@media", "keyword"),
                ("p {", "tag"),
            ],
        );
    }

    #[test]
    fn sql_colors() {
        assert_colors(
            "sql",
            "SELECT id, name FROM users WHERE id = 1 AND name = 'x'; -- c\n",
            &[
                ("SELECT", "keyword"),
                ("FROM", "keyword"),
                ("users", "type"),
                ("1", "number"),
                ("AND", "operator"),
                ("'x'", "string"),
                ("-- c", "comment"),
            ],
        );
    }

    #[test]
    fn c_colors() {
        assert_colors(
            "c",
            "#include <stdio.h>\nint main(void) { return 0; } // c\n",
            &[
                ("#include", "keyword"),
                ("int", "type"),
                ("main", "function"),
                ("return", "keyword"),
                ("0", "number"),
                ("// c", "comment"),
            ],
        );
    }

    #[test]
    fn java_colors() {
        assert_colors(
            "java",
            "class A { void f() { String s = \"x\"; } }\n",
            &[
                ("class", "keyword"),
                ("f(", "function"),
                ("String", "type"),
                ("\"x\"", "string"),
            ],
        );
    }

    /// A fence in a language without a grammar is colored by the fallback.
    #[test]
    fn markdown_fences_without_a_grammar_use_the_fallback() {
        let text = "```kotlin\nval x = 1\n```\n";
        let (grammar, tree) = parse_new("markdown", text);
        let fallback = |language: &str, code: &str| {
            (language == "kotlin").then(|| vec![(code.find("val").unwrap()..3, "keyword")])
        };
        let highlights = grammar.highlight(&tree, text, 0..text.len(), &fallback, &never);
        assert_eq!(name_at(&highlights, text, "val"), Some("keyword"));
    }

    /// An edit parsed with the old tree gives the colors of a parse from scratch, and so does
    /// recoloring only the lines it changed.
    #[test]
    fn incremental_edits_match_a_full_parse() {
        let before: String = (0..300)
            .map(|i| format!("fn f{i}() -> u32 {{ let s = \"x{i}\"; {i} }} // c\n"))
            .collect();
        let at = before.find("fn f150").unwrap();
        for insert in ["/* open ", "let y = 2; ", "\"", "}\n"] {
            let after = format!("{}{insert}{}", &before[..at], &before[at..]);
            let (grammar, old_tree) = parse_new("rust", &before);
            let old_highlights =
                grammar.highlight(&old_tree, &before, 0..before.len(), &no_fallback, &never);

            let mut edited = old_tree.clone();
            let line = before[..at].matches('\n').count();
            edited.edit(&tree_sitter::InputEdit {
                start_byte: at,
                old_end_byte: at,
                new_end_byte: at + insert.len(),
                start_position: tree_sitter::Point::new(line, 0),
                old_end_position: tree_sitter::Point::new(line, 0),
                new_end_position: tree_sitter::Point::new(
                    line + insert.matches('\n').count(),
                    insert.len() - insert.rfind('\n').map_or(0, |i| i + 1),
                ),
            });
            let mut parser = Parser::new();
            let reparsed = grammar
                .parse(&mut parser, &after, Some(&edited), &never)
                .unwrap();
            let fresh = parse_new("rust", &after).1;
            let full = grammar.highlight(&fresh, &after, 0..after.len(), &no_fallback, &never);
            let incremental =
                grammar.highlight(&reparsed, &after, 0..after.len(), &no_fallback, &never);
            assert_eq!(incremental, full, "{insert:?}");

            // Shift the old colors past the edit, then recolor the changed lines only.
            let mut local = old_highlights;
            crate::highlight::shift(
                &mut local,
                &gpui_kit::component::input::InputEdit {
                    start_byte: at,
                    old_end_byte: at,
                    new_end_byte: at + insert.len(),
                    start_position: gpui_kit::component::input::Point::new(line, 0),
                    old_end_position: gpui_kit::component::input::Point::new(line, 0),
                    new_end_position: gpui_kit::component::input::Point::new(line, 0),
                },
            );
            let inserted = at..at + insert.len();
            let lines = changed_lines(
                &edited,
                &reparsed,
                std::slice::from_ref(&inserted),
                &after,
                usize::MAX,
            )
            .unwrap();
            for range in lines {
                let fresh =
                    grammar.highlight(&reparsed, &after, range.clone(), &no_fallback, &never);
                crate::highlight::splice(&mut local, range, fresh);
            }
            assert_eq!(
                per_byte(&local, after.len()),
                per_byte(&full, after.len()),
                "{insert:?}"
            );
        }
    }

    #[test]
    fn a_stopped_parse_gives_nothing() {
        let grammar = grammar("rust").unwrap();
        let text = "fn f() {}\n".repeat(10_000);
        assert!(
            grammar
                .parse(&mut Parser::new(), &text, None, &|| true)
                .is_none()
        );
    }

    fn folds(language: &str, text: &str) -> Vec<(usize, usize)> {
        let (_, tree) = parse_new(language, text);
        fold_lines(&tree, text, 0..text.len())
    }

    #[test]
    fn rust_folds_keep_the_closing_brace_in_view() {
        let text = "fn f() {\n    let x = 1;\n    if x > 0 {\n        g();\n    }\n}\n\nstruct S {\n    a: u8,\n}\n";
        assert_eq!(folds("rust", text), vec![(0, 5), (2, 4), (7, 9)]);
    }

    #[test]
    fn typescript_folds() {
        let text = "export function f() {\n  const o = {\n    a: 1,\n  };\n}\n";
        assert_eq!(folds("typescript", text), vec![(0, 4), (1, 3)]);
    }

    #[test]
    fn python_folds_hide_the_whole_body() {
        let text = "def f(x):\n    if x:\n        return 1\n    return 2\n\nclass A:\n    pass\n";
        assert_eq!(folds("python", text), vec![(0, 4), (1, 3), (5, 7)]);
    }

    #[test]
    fn json_folds() {
        let text = "{\n  \"a\": {\n    \"b\": 1\n  },\n  \"c\": [\n    1,\n    2\n  ]\n}\n";
        assert_eq!(folds("json", text), vec![(0, 8), (1, 3), (4, 7)]);
    }

    #[test]
    fn markdown_folds_sections_up_to_the_next_heading() {
        let text = "# One\n\nText.\n\n## Two\n\nMore.\n\n# Three\n\nEnd.\n";
        assert_eq!(folds("markdown", text), vec![(0, 8), (4, 8), (8, 11)]);
    }

    #[test]
    fn closing_words_need_a_word_boundary() {
        assert!(closes("end"));
        assert!(closes("fi"));
        assert!(closes("};"));
        assert!(!closes("endpoint = 1"));
        assert!(!closes("return x"));
    }
}
