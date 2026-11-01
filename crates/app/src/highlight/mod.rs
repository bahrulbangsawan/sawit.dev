//! Syntax highlighting and folding for the code editors, kept off the UI thread.
//!
//! Two backends sit behind one factory. Languages Sawit ships a grammar for (see
//! [`grammars`]) parse with tree-sitter: Rust, TypeScript, TSX, JavaScript, JSON, Python, Go,
//! Bash, TOML, YAML, Markdown, HTML, CSS, SQL, C and Java. Everything else syntect knows
//! (Dockerfile, Makefile, Kotlin, C++, Swift, Ruby, PHP, about 200 in all) keeps the syntect
//! parser, so nothing that was colored before goes plain.
//!
//! gpui-kit's own highlighters parse on the UI thread: its syntect one reparses the whole
//! document for anything up to 64 KB (50–130 ms each time a file opens in a release build,
//! close to a second in a debug one, and the same again on every keystroke), and its
//! tree-sitter one parses up to 256 KB there and runs the highlight query on every repaint.
//! Both backends here parse documents on the background executor and hand the editor colors
//! computed once per parse. While typing, the edited lines are recolored at once (tree-sitter
//! reparses with the old tree within a small budget, syntect from the nearest saved parse
//! state) and a background parse fixes up the rest of the document after a pause.
//!
//! Tree-sitter is also the cheaper parse. In a release build (on an M3 busy with other builds,
//! best of five), a 30 KB Rust file took 4.5 ms to parse and 10 ms to color against 109 ms for
//! syntect, `workspace.rs` (160 KB) 22 + 42 ms against 590 ms, a 1.1 MB file 154 + 303 ms
//! against 4.3 s, and the 47 KB `browser_design.js` 8 + 11 ms against 310 ms. Compiling every
//! grammar's queries takes about 220 ms, once, in `prewarm`.
//!
//! Both map what they parse to the highlight names of gpui-component's theme (the scope
//! mapping follows its syntect backend, Apache-2.0), so colors match the rest of the app.

mod grammars;
mod syntect_highlighter;
mod tree_sitter_highlighter;

use std::ops::Range;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

use gpui_kit::component::input::{
    EditorState, HighlightStyleResolver, InputEdit, InputHighlighterFactory,
};
use gpui_kit::{App, Context, HighlightStyle};
use syntect::parsing::{SyntaxReference, SyntaxSet};

use grammars::{Grammar, Highlights};
use syntect_highlighter::{SyntectHighlighter, find_syntax};
use tree_sitter_highlighter::TreeSitterHighlighter;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Documents this small are parsed in place: well under a millisecond.
const SYNC_MAX_BYTES: usize = 4 * 1024;
/// A syntect parse state is saved at the start of every this many lines.
const CHECKPOINT_LINES: usize = 32;
/// The most text an edit recolors in place before leaving it to the background parse.
const LOCAL_MAX_BYTES: usize = 16 * 1024;
/// The longest an edit's tree-sitter reparse may run on the UI thread. A keystroke's reparse
/// reuses the old tree: 0.1–1 ms in Rust, JavaScript and JSON files up to 160 KB, 1.8 ms in a
/// 1.1 MB one. One that takes longer (an opening `/*` that turns the rest of the file into a
/// comment, a Markdown file's block structure) stops here and waits for the background parse.
const LOCAL_PARSE_BUDGET: Duration = Duration::from_millis(2);
/// Quiet time after the last edit before the whole document is reparsed.
const PARSE_DEBOUNCE: Duration = Duration::from_millis(100);

/// Loads the syntax definitions and compiles the grammars' queries in the background, so the
/// first file opened doesn't wait for them.
pub fn prewarm(cx: &App) {
    cx.background_executor()
        .spawn(async {
            LazyLock::force(&SYNTAXES);
            grammars::compile_all();
        })
        .detach();
}

pub fn factory() -> InputHighlighterFactory {
    Rc::new(|language| match backend(language)? {
        Backend::TreeSitter(grammar) => {
            Some(Box::new(TreeSitterHighlighter::new(language, grammar)) as _)
        }
        Backend::Syntect(syntax) => Some(Box::new(SyntectHighlighter::new(language, syntax)) as _),
    })
}

/// Installs the off-thread highlighter on a code editor.
pub fn install(state: &mut EditorState, cx: &mut Context<EditorState>) {
    state.set_highlighter_factory(factory(), cx);
}

enum Backend {
    TreeSitter(&'static Grammar),
    Syntect(&'static SyntaxReference),
}

/// Tree-sitter for the languages Sawit ships a grammar for, syntect for any other it knows,
/// nothing for plain text.
fn backend(language: &str) -> Option<Backend> {
    match grammars::grammar(language) {
        Some(grammar) => Some(Backend::TreeSitter(grammar)),
        None => find_syntax(language).map(Backend::Syntect),
    }
}

/// Moves the highlights after an edit by its length and drops the ones it
/// touched.
fn shift(highlights: &mut Highlights, edit: &InputEdit) {
    let delta = edit.new_end_byte as isize - edit.old_end_byte as isize;
    highlights.retain_mut(|(range, _)| {
        if range.end <= edit.start_byte {
            true
        } else if range.start >= edit.old_end_byte {
            range.start = range.start.saturating_add_signed(delta);
            range.end = range.end.saturating_add_signed(delta);
            true
        } else {
            false
        }
    });
}

/// Replaces the highlights inside `range` with `fresh` (all inside it).
fn splice(highlights: &mut Highlights, range: Range<usize>, fresh: Highlights) {
    let mut out = Highlights::with_capacity(highlights.len() + fresh.len());
    let mut fresh = Some(fresh);
    for (span, name) in highlights.drain(..) {
        if span.end <= range.start {
            out.push((span, name));
            continue;
        }
        if span.start < range.start {
            out.push((span.start..range.start, name));
        }
        if let Some(fresh) = fresh.take() {
            out.extend(fresh);
        }
        if span.end > range.end {
            out.push((span.start.max(range.end)..span.end, name));
        }
    }
    if let Some(fresh) = fresh {
        out.extend(fresh);
    }
    *highlights = out;
}

/// Turns the highlights overlapping `range` into gap-free style runs.
fn resolve_styles(
    highlights: &[(Range<usize>, &'static str)],
    range: &Range<usize>,
    resolver: &dyn HighlightStyleResolver,
) -> Vec<(Range<usize>, HighlightStyle)> {
    let first = highlights.partition_point(|(highlight, _)| highlight.end <= range.start);
    let mut runs = Vec::new();
    let mut cursor = range.start;
    for (highlight, name) in &highlights[first..] {
        if highlight.start >= range.end {
            break;
        }
        let start = highlight.start.max(range.start);
        let end = highlight.end.min(range.end);
        if start >= end || end <= cursor {
            continue;
        }
        if cursor < start {
            runs.push((cursor..start, HighlightStyle::default()));
        }
        runs.push((start..end, resolver.style(name).unwrap_or_default()));
        cursor = end;
    }
    if cursor < range.end {
        runs.push((cursor..range.end, HighlightStyle::default()));
    }
    runs
}

#[cfg(test)]
mod tests;
