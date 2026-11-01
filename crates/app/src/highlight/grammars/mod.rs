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

mod folds;
mod highlighting;

use std::ops::{ControlFlow, Range};
use std::sync::OnceLock;

use tree_sitter::{Language, ParseOptions, ParseState, Parser, Query, Tree};

pub(crate) use folds::fold_lines;
pub(crate) use highlighting::changed_lines;

/// Highlight names, ordered and non-overlapping.
pub(crate) type Highlights = Vec<(Range<usize>, &'static str)>;

/// Colors injected code that no grammar here parses, given its language name and its text: the
/// syntect backend, so a Markdown fence in a language without a grammar keeps its colors.
pub(crate) type Fallback = dyn Fn(&str, &str) -> Option<Highlights>;

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

impl PartialEq for Grammar {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
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
mod tests;
