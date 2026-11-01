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

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::{App, Context, HighlightStyle, SharedString, Task, Window};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;
use tree_sitter::{Parser, Tree};

use grammars::{Grammar, Highlights};

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

fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let syntaxes = &*SYNTAXES;
    let lower = language.to_lowercase();
    let alias = match lower.as_str() {
        "" | "text" | "plain" | "plaintext" => return None,
        "csharp" => "cs",
        "make" | "makefile" => "Makefile",
        "markdown" | "mdx" => "md",
        "jsonc" => "json",
        "scss" => "css",
        "embedded_template" | "ejs" => "erb",
        "c++" => "cpp",
        "protobuf" => "proto",
        other => other,
    };
    [language, lower.as_str(), alias]
        .into_iter()
        .find_map(|token| syntaxes.find_syntax_by_token(token))
        .filter(|syntax| syntax.name != "Plain Text")
}

/// Colors code injected into a tree-sitter document (a Markdown fence) in a language without
/// a grammar here, with syntect.
fn syntect_fallback(language: &str, code: &str) -> Option<Highlights> {
    find_syntax(language).map(|syntax| parse_all(syntax, code).highlights)
}

/// Where a line starts and the parser state at that point.
#[derive(Clone)]
struct Checkpoint {
    byte: usize,
    parser: ParseState,
    scopes: ScopeStack,
}

#[derive(Default)]
struct Parsed {
    highlights: Highlights,
    checkpoints: Vec<Checkpoint>,
}

/// Parses `text` from `from` (a line start) up to the end of the line that
/// contains `until`, saving a checkpoint every `CHECKPOINT_LINES` lines.
fn parse_lines(text: &str, from: Checkpoint, until: usize) -> Parsed {
    let syntaxes = &*SYNTAXES;
    let Checkpoint {
        byte: start,
        mut parser,
        mut scopes,
    } = from;
    let mut names = HashMap::<Scope, Option<&'static str>>::new();
    let mut out = Parsed::default();
    let mut offset = start;

    let mut push = |range: Range<usize>, scopes: &ScopeStack, highlights: &mut Highlights| {
        if range.is_empty() {
            return;
        }
        // The innermost scope that names something wins, so the quotes of a
        // string color as the string rather than as punctuation.
        let name = scopes
            .scopes
            .iter()
            .rev()
            .find_map(|scope| *names.entry(*scope).or_insert_with(|| semantic_name(*scope)));
        if let Some(name) = name {
            match highlights.last_mut() {
                Some((last, last_name)) if last.end == range.start && *last_name == name => {
                    last.end = range.end;
                }
                _ => highlights.push((range, name)),
            }
        }
    };

    for (index, line) in LinesWithEndings::from(&text[start..]).enumerate() {
        if offset > until {
            break;
        }
        if index % CHECKPOINT_LINES == 0 {
            out.checkpoints.push(Checkpoint {
                byte: offset,
                parser: parser.clone(),
                scopes: scopes.clone(),
            });
        }
        // A line syntect cannot parse keeps the scopes it started with.
        if let Ok(operations) = parser.parse_line(line, syntaxes) {
            let mut cursor = 0;
            for (at, operation) in operations {
                push(offset + cursor..offset + at, &scopes, &mut out.highlights);
                let _ = scopes.apply(&operation);
                cursor = at;
            }
            push(
                offset + cursor..offset + line.len(),
                &scopes,
                &mut out.highlights,
            );
        }
        offset += line.len();
    }
    out
}

fn parse_all(syntax: &SyntaxReference, text: &str) -> Parsed {
    parse_lines(
        text,
        Checkpoint {
            byte: 0,
            parser: ParseState::new(syntax),
            scopes: ScopeStack::new(),
        },
        text.len(),
    )
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

struct SyntectHighlighter {
    language: SharedString,
    syntax: &'static SyntaxReference,
    highlights: Rc<RefCell<Highlights>>,
    checkpoints: Rc<RefCell<Vec<Checkpoint>>>,
    folds: Rc<RefCell<Vec<FoldRange>>>,
    parse_task: Option<Task<()>>,
}

impl SyntectHighlighter {
    fn new(language: &str, syntax: &'static SyntaxReference) -> Self {
        Self {
            language: language.to_owned().into(),
            syntax,
            highlights: Rc::default(),
            checkpoints: Rc::default(),
            folds: Rc::default(),
            parse_task: None,
        }
    }

    /// Reparses the lines an edit touched, from the last checkpoint before
    /// it. Lines after keep their (shifted) colors until the full parse.
    fn reparse_edited(&self, edit: &InputEdit, text: &str) {
        let mut checkpoints = self.checkpoints.borrow_mut();
        // States saved after the edit started describe the old text.
        checkpoints.retain(|c| c.byte <= edit.start_byte);
        let Some(from) = checkpoints.last().cloned() else {
            return;
        };
        let until = edit.new_end_byte.min(text.len());
        if until.saturating_sub(from.byte) > LOCAL_MAX_BYTES {
            return;
        }
        let parsed = parse_lines(text, from.clone(), until);
        let end = parsed
            .highlights
            .last()
            .map_or(from.byte, |(r, _)| r.end)
            .max(line_end(text, until));
        splice(
            &mut self.highlights.borrow_mut(),
            from.byte..end,
            parsed.highlights,
        );
        checkpoints.extend(parsed.checkpoints.into_iter().skip(1));
    }
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i + 1)
}

impl InputHighlighter for SyntectHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        let text = text.to_string();
        if text.len() <= SYNC_MAX_BYTES {
            self.parse_task = None;
            let parsed = parse_all(self.syntax, &text);
            *self.highlights.borrow_mut() = parsed.highlights;
            *self.checkpoints.borrow_mut() = parsed.checkpoints;
            *self.folds.borrow_mut() = if folding {
                brace_fold_ranges(&text)
            } else {
                Vec::new()
            };
            return;
        }

        if let Some(edit) = &edit {
            shift(&mut self.highlights.borrow_mut(), edit);
            self.reparse_edited(edit, &text);
        }

        let syntax = self.syntax;
        let highlights = self.highlights.clone();
        let checkpoints = self.checkpoints.clone();
        let folds = self.folds.clone();
        // A freshly loaded document parses right away; edits wait for a pause.
        let debounce = edit.is_some();
        // Replacing the task drops the previous one, so a stale parse never
        // lands on newer text.
        self.parse_task = Some(cx.spawn_in(window, async move |entity, cx| {
            if debounce {
                cx.background_executor().timer(PARSE_DEBOUNCE).await;
            }
            let (parsed, folded) = cx
                .background_executor()
                .spawn(async move {
                    let folded = if folding {
                        brace_fold_ranges(&text)
                    } else {
                        Vec::new()
                    };
                    (parse_all(syntax, &text), folded)
                })
                .await;
            *highlights.borrow_mut() = parsed.highlights;
            *checkpoints.borrow_mut() = parsed.checkpoints;
            *folds.borrow_mut() = folded.clone();
            // Also repaints with the new colors.
            let _ = entity.update(cx, |state, cx| {
                state.apply_highlighter_fold_candidates(folded, cx);
            });
        }));
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        resolve_styles(&self.highlights.borrow(), range, resolver)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.folds.borrow().clone()
    }
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

/// The theme's highlight name for a TextMate scope, or `None` to let an
/// enclosing scope decide.
fn semantic_name(scope: Scope) -> Option<&'static str> {
    let scope = scope.build_string();
    let is = |prefix: &str| {
        scope == prefix
            || scope
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    let name = if is("punctuation.definition.string")
        || is("punctuation.definition.comment")
        || is("invalid")
        || is("meta")
        || is("source")
        || is("text")
    {
        return None;
    } else if is("comment.block.documentation") || is("comment.line.documentation") {
        "comment.doc"
    } else if is("comment") {
        "comment"
    } else if is("constant.character.escape") {
        "string.escape"
    } else if is("string.regexp") {
        "string.regex"
    } else if is("string") {
        "string"
    } else if is("constant.numeric") {
        "number"
    } else if is("constant.language.boolean") {
        "boolean"
    } else if is("constant") || is("support.constant") || is("entity.name.constant") {
        "constant"
    } else if is("keyword.operator") {
        "operator"
    } else if is("keyword.control.import") || is("meta.preprocessor") {
        "preproc"
    } else if is("keyword") || is("storage.modifier") {
        "keyword"
    } else if is("storage.type") {
        // `fn`, `class` and `let` are declarations, `int` and `bool` are types.
        if [
            "class",
            "enum",
            "function",
            "impl",
            "interface",
            "module",
            "namespace",
            "struct",
            "trait",
            "type",
            "let",
            "var",
            "const",
        ]
        .iter()
        .any(|word| scope.contains(word))
        {
            "keyword"
        } else {
            "type"
        }
    } else if is("entity.name.function")
        || is("support.function")
        || is("variable.function")
        || is("meta.function-call")
    {
        "function"
    } else if is("entity.name.type")
        || is("entity.name.class")
        || is("entity.name.struct")
        || is("entity.name.enum")
        || is("entity.name.trait")
        || is("entity.name.interface")
        || is("entity.other.inherited-class")
        || is("support.type")
        || is("support.class")
    {
        "type"
    } else if is("entity.name.tag") {
        "tag"
    } else if is("entity.other.attribute-name") {
        "attribute"
    } else if is("entity.name.label") {
        "label"
    } else if is("entity.name.section") || is("markup.heading") {
        "title"
    } else if is("variable.language") {
        "variable.special"
    } else if is("variable.other.member")
        || is("variable.other.property")
        || is("support.variable.property")
        || is("meta.mapping.key")
    {
        "property"
    } else if is("variable") {
        "variable"
    } else if is("markup.bold") {
        "emphasis.strong"
    } else if is("markup.italic") {
        "emphasis"
    } else if is("markup.underline.link") {
        "link_uri"
    } else if is("markup.raw") {
        "text.literal"
    } else if is("markup.list") {
        "punctuation.list_marker"
    } else if is("punctuation.section") || is("punctuation.definition.block") {
        "punctuation.bracket"
    } else if is("punctuation.separator") || is("punctuation.terminator") {
        "punctuation.delimiter"
    } else if is("punctuation") {
        "punctuation"
    } else {
        return None;
    };
    Some(name)
}

/// Fold candidates from matching braces outside strings and line comments, for
/// languages without a tree-sitter grammar.
fn brace_fold_ranges(text: &str) -> Vec<FoldRange> {
    let mut starts = Vec::new();
    let mut ranges = Vec::new();
    for (line_number, line) in text.lines().enumerate() {
        let mut chars = line.chars().peekable();
        let mut quote = None;
        let mut escaped = false;
        while let Some(character) = chars.next() {
            match quote {
                Some(open) => {
                    if escaped {
                        escaped = false;
                    } else if character == '\\' {
                        escaped = true;
                    } else if character == open {
                        quote = None;
                    }
                }
                None => match character {
                    '/' if chars.peek() == Some(&'/') => break,
                    '#' => break,
                    '"' | '\'' | '`' => quote = Some(character),
                    '{' => starts.push(line_number),
                    '}' => {
                        if let Some(start) = starts.pop()
                            && line_number.saturating_sub(start) >= 2
                        {
                            ranges.push(FoldRange::new(start, line_number));
                        }
                    }
                    _ => {}
                },
            }
        }
    }
    ranges.sort_by_key(|range| range.start_line);
    ranges.dedup_by_key(|range| range.start_line);
    ranges
}

// --- tree-sitter -------------------------------------------------------------------------------

fn to_tree_sitter(edit: &InputEdit) -> tree_sitter::InputEdit {
    let point =
        |point: gpui_kit::component::input::Point| tree_sitter::Point::new(point.row, point.column);
    tree_sitter::InputEdit {
        start_byte: edit.start_byte,
        old_end_byte: edit.old_end_byte,
        new_end_byte: edit.new_end_byte,
        start_position: point(edit.start_position),
        old_end_position: point(edit.old_end_position),
        new_end_position: point(edit.new_end_position),
    }
}

/// Sets its flag when dropped: a background parse whose task was replaced stops early.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A tree-sitter parse of a document and what the editor reads from it.
#[derive(Default)]
struct Syntax {
    /// The latest parse, edited to match every change since. `None` before the first one, and
    /// after a change the editor can't describe as an edit.
    tree: Option<Tree>,
    highlights: Highlights,
    folds: Vec<FoldRange>,
    /// An edit's reparse ran out of `LOCAL_PARSE_BUDGET` since this parse: the next edits leave
    /// recoloring to the background parse rather than spend the budget again for nothing.
    over_budget: bool,
}

impl Syntax {
    /// Parses `text`, reusing `old` where it still applies, and reads its colors and (when the
    /// editor folds) fold ranges. `None` when `stop` stopped it.
    fn parse(
        grammar: &Grammar,
        parser: &mut Parser,
        text: &str,
        old: Option<&Tree>,
        folding: bool,
        stop: &dyn Fn() -> bool,
    ) -> Option<Self> {
        let tree = grammar.parse(parser, text, old, stop)?;
        let highlights = grammar.highlight(&tree, text, 0..text.len(), &syntect_fallback, stop);
        if stop() {
            return None;
        }
        let folds = if folding {
            fold_ranges(&tree, text, 0..text.len())
        } else {
            Vec::new()
        };
        Some(Self {
            tree: Some(tree),
            highlights,
            folds,
            over_budget: false,
        })
    }
}

fn fold_ranges(tree: &Tree, text: &str, range: Range<usize>) -> Vec<FoldRange> {
    grammars::fold_lines(tree, text, range)
        .into_iter()
        .map(|(start, end)| FoldRange::new(start, end))
        .collect()
}

struct TreeSitterHighlighter {
    language: SharedString,
    grammar: &'static Grammar,
    syntax: Rc<RefCell<Syntax>>,
    /// Parses small documents, and edits within budget, on the UI thread.
    parser: Parser,
    /// `LOCAL_PARSE_BUDGET`, or more in a test that can't have its parse stopped by a busy machine.
    parse_budget: Duration,
    parse_task: Option<Task<()>>,
}

impl TreeSitterHighlighter {
    fn new(language: &str, grammar: &'static Grammar) -> Self {
        Self {
            language: language.to_owned().into(),
            grammar,
            syntax: Rc::default(),
            parser: Parser::new(),
            parse_budget: LOCAL_PARSE_BUDGET,
            parse_task: None,
        }
    }

    /// Brings the colors up to date with `text`, reached from the last parsed text by `edits`
    /// (none when the change is unknown, as when a file loads).
    fn reparse(
        &mut self,
        edits: &[InputEdit],
        text: &Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        // A parse still running describes older text.
        self.parse_task = None;
        let text = text.to_string();
        let grammar = self.grammar;
        let mut syntax = self.syntax.borrow_mut();
        if edits.is_empty() {
            syntax.tree = None;
        }
        for edit in edits {
            if let Some(tree) = &mut syntax.tree {
                tree.edit(&to_tree_sitter(edit));
            }
            shift(&mut syntax.highlights, edit);
        }

        // Compiling a grammar's queries can take tens of milliseconds, so nothing parses here
        // until `prewarm` has compiled them all.
        let in_place = grammars::ready();
        if in_place && text.len() <= SYNC_MAX_BYTES {
            let old = syntax.tree.take();
            *syntax = Syntax::parse(
                grammar,
                &mut self.parser,
                &text,
                old.as_ref(),
                folding,
                &|| false,
            )
            .unwrap_or_default();
            return;
        }
        if in_place
            && !edits.is_empty()
            && !syntax.over_budget
            && let Some(old) = syntax.tree.clone()
        {
            let budget = self.parse_budget;
            recolor_edited_lines(
                grammar,
                &mut self.parser,
                budget,
                &mut syntax,
                &old,
                edits,
                &text,
            );
        }

        let old_tree = syntax.tree.clone();
        drop(syntax);
        let shared = self.syntax.clone();
        // A freshly loaded document parses right away; edits wait for a pause.
        let debounce = !edits.is_empty();
        self.parse_task = Some(cx.spawn_in(window, async move |entity, cx| {
            let cancel = Arc::new(AtomicBool::new(false));
            let _cancel_when_dropped = CancelOnDrop(cancel.clone());
            if debounce {
                cx.background_executor().timer(PARSE_DEBOUNCE).await;
            }
            let parsed = cx
                .background_executor()
                .spawn(async move {
                    let stop = || cancel.load(Ordering::Relaxed);
                    let mut parser = Parser::new();
                    Syntax::parse(
                        grammar,
                        &mut parser,
                        &text,
                        old_tree.as_ref(),
                        folding,
                        &stop,
                    )
                })
                .await;
            let Some(parsed) = parsed else {
                return;
            };
            let folds = parsed.folds.clone();
            *shared.borrow_mut() = parsed;
            // Also repaints with the new colors.
            let _ = entity.update(cx, |state, cx| {
                state.apply_highlighter_fold_candidates(folds, cx);
            });
        }));
    }
}

/// Reparses with the edited tree, if that fits `budget`, and recolors the lines whose syntax
/// changed. Lines it leaves keep their (shifted) colors until the full parse.
fn recolor_edited_lines(
    grammar: &Grammar,
    parser: &mut Parser,
    budget: Duration,
    syntax: &mut Syntax,
    old: &Tree,
    edits: &[InputEdit],
    text: &str,
) {
    let deadline = Instant::now() + budget;
    let Some(tree) = grammar.parse(parser, text, Some(old), &|| Instant::now() > deadline) else {
        syntax.over_budget = true;
        return;
    };
    let edited: Vec<Range<usize>> = edits
        .iter()
        .map(|edit| edit.start_byte.min(text.len())..edit.new_end_byte.min(text.len()))
        .collect();
    if let Some(lines) = grammars::changed_lines(old, &tree, &edited, text, LOCAL_MAX_BYTES) {
        for range in lines {
            let fresh = grammar.highlight(&tree, text, range.clone(), &syntect_fallback, &|| false);
            splice(&mut syntax.highlights, range, fresh);
        }
    }
    syntax.tree = Some(tree);
}

impl InputHighlighter for TreeSitterHighlighter {
    fn language(&self) -> SharedString {
        self.language.clone()
    }

    fn update(
        &mut self,
        edit: Option<InputEdit>,
        text: &Rope,
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        self.reparse(edit.as_slice(), text, folding, window, cx);
    }

    /// Takes every edit of a multi-cursor change into the tree, then parses once.
    fn update_batch(
        &mut self,
        edits: &[(InputEdit, Rope)],
        folding: bool,
        window: &mut Window,
        cx: &mut Context<EditorState>,
    ) {
        let Some((_, text)) = edits.last() else {
            return;
        };
        let edits: Vec<InputEdit> = edits.iter().map(|(edit, _)| *edit).collect();
        self.reparse(&edits, text, folding, window, cx);
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        resolve_styles(&self.syntax.borrow().highlights, range, resolver)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        self.syntax.borrow().folds.clone()
    }

    /// Folds from the tree around an edit, which `update` has already taken in.
    fn fold_ranges_for_edit(&self, range: Range<usize>, text: &Rope) -> Vec<FoldRange> {
        match &self.syntax.borrow().tree {
            Some(tree) => fold_ranges(tree, &text.to_string(), range),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Backend, backend, find_syntax, parse_all, parse_lines, splice};

    /// Reparsing from a checkpoint gives the same colors as a full parse.
    #[test]
    fn local_reparse_matches_full_parse() {
        let syntax = find_syntax("rust").unwrap();
        let text: String = (0..200)
            .map(|i| format!("fn f{i}() -> u32 {{ let s = \"x{i}\"; {i} }} // c\n"))
            .collect();
        let full = parse_all(syntax, &text);
        let from = full.checkpoints[3].clone();
        let until = from.byte + 10;
        let local = parse_lines(&text, from.clone(), until);
        let end = local.highlights.last().unwrap().0.end;
        let mut spliced = full.highlights.clone();
        splice(&mut spliced, from.byte..end, local.highlights);
        assert_eq!(spliced, full.highlights);
    }

    #[test]
    fn plain_text_has_no_highlighter() {
        assert!(backend("text").is_none());
        assert!(backend("").is_none());
        assert!(find_syntax("text").is_none());
        assert!(find_syntax("markdown").is_some());
        assert!(find_syntax("sql").is_some());
    }

    /// The editors' language tokens (`sawit_core::files::language_for`) reach tree-sitter when
    /// Sawit ships the grammar, and syntect otherwise.
    #[test]
    fn each_language_uses_its_backend() {
        for language in [
            "rust",
            "typescript",
            "tsx",
            "javascript",
            "json",
            "python",
            "go",
            "bash",
            "toml",
            "yaml",
            "markdown",
            "html",
            "css",
            "sql",
            "c",
            "java",
        ] {
            assert!(
                matches!(backend(language), Some(Backend::TreeSitter(_))),
                "{language}"
            );
        }
        for language in [
            "dockerfile",
            "make",
            "kotlin",
            "cpp",
            "csharp",
            "ruby",
            "php",
            "swift",
            "lua",
            "scss",
            "xml",
            "diff",
        ] {
            assert!(
                matches!(backend(language), Some(Backend::Syntect(_))),
                "{language}"
            );
        }
        // Unknown tokens: neither backend knows them, so the editor shows plain text.
        assert!(backend("nim-but-not-really").is_none());
    }

    /// Markdown code fenced in a language without a grammar is colored by syntect.
    #[test]
    fn markdown_fences_without_a_grammar_fall_back_to_syntect() {
        let text = "```kotlin\nfun main() {}\n```\n";
        let highlights = super::syntect_fallback("kotlin", "fun main() {}\n").unwrap();
        assert_eq!(highlights.first().map(|(_, name)| *name), Some("keyword"));
        let grammar = super::grammars::grammar("markdown").unwrap();
        let tree = grammar
            .parse(&mut tree_sitter::Parser::new(), text, None, &|| false)
            .unwrap();
        let highlights = grammar.highlight(
            &tree,
            text,
            0..text.len(),
            &super::syntect_fallback,
            &|| false,
        );
        let at = text.find("fun").unwrap();
        assert!(
            highlights
                .iter()
                .any(|(range, name)| range.contains(&at) && *name == "keyword"),
            "{highlights:?}"
        );
    }

    fn update(
        cx: &mut gpui_kit::TestAppContext,
        window: gpui_kit::AnyWindowHandle,
        state: &gpui_kit::Entity<gpui_kit::component::input::EditorState>,
        highlighter: &mut super::TreeSitterHighlighter,
        edit: Option<gpui_kit::component::input::InputEdit>,
        text: &str,
    ) {
        use gpui_kit::AppContext as _;
        use gpui_kit::component::input::InputHighlighter as _;
        let rope = super::Rope::from(text);
        cx.update_window(window, |_, window, cx| {
            state.update(cx, |_, cx| {
                highlighter.update(edit, &rope, false, window, cx);
            });
        })
        .unwrap();
    }

    /// A document too big to parse in place is colored by a background parse. An edit recolors
    /// its own lines at once, and the background parse after it gives the colors of a parse from
    /// scratch.
    #[gpui_kit::test]
    fn large_documents_parse_in_the_background_and_edits_recolor_at_once(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        use gpui_kit::AppContext as _;
        use gpui_kit::component::input::{EditorState, InputEdit, Point};

        cx.update(gpui_kit::init);
        super::grammars::compile_all();
        let grammar = super::grammars::grammar("rust").unwrap();
        let full = |text: &str| {
            let tree = grammar
                .parse(&mut tree_sitter::Parser::new(), text, None, &|| false)
                .unwrap();
            grammar.highlight(
                &tree,
                text,
                0..text.len(),
                &super::syntect_fallback,
                &|| false,
            )
        };
        let before: String = (0..400)
            .map(|i| format!("fn f{i}() -> u32 {{ let s = \"x{i}\"; {i} }}\n"))
            .collect();
        assert!(before.len() > super::SYNC_MAX_BYTES);
        let (window, state) = cx.update(|cx| {
            gpui_kit::open_window(Default::default(), cx, |window, cx| {
                cx.new(|cx| EditorState::new(window, cx))
            })
            .unwrap()
        });
        let mut highlighter = super::TreeSitterHighlighter::new("rust", grammar);
        // The edit below must be recolored in place however busy the machine running the test.
        highlighter.parse_budget = std::time::Duration::from_secs(10);

        update(cx, window, &state, &mut highlighter, None, &before);
        assert!(
            highlighter.syntax.borrow().highlights.is_empty(),
            "parsed on the UI thread"
        );
        cx.run_until_parked();
        assert_eq!(highlighter.syntax.borrow().highlights, full(&before));

        let line = 200;
        let at = before.match_indices('\n').nth(line - 1).unwrap().0 + 1;
        let insert = "pub ";
        let after = format!("{}{insert}{}", &before[..at], &before[at..]);
        let edit = InputEdit {
            start_byte: at,
            old_end_byte: at,
            new_end_byte: at + insert.len(),
            start_position: Point::new(line, 0),
            old_end_position: Point::new(line, 0),
            new_end_position: Point::new(line, insert.len()),
        };
        update(cx, window, &state, &mut highlighter, Some(edit), &after);
        let name_at = |at: usize| {
            highlighter
                .syntax
                .borrow()
                .highlights
                .iter()
                .find(|(range, _)| range.contains(&at))
                .map(|(_, name)| *name)
        };
        assert_eq!(name_at(at), Some("keyword"), "the edited line waits");
        assert_eq!(name_at(at + insert.len()), Some("keyword"));

        cx.executor().advance_clock(super::PARSE_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(highlighter.syntax.borrow().highlights, full(&after));
    }
}
