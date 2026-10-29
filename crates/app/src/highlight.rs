//! Syntax highlighting for the code editors, kept off the UI thread.
//!
//! gpui-kit's syntect highlighter reparses the whole document on the UI
//! thread for anything up to 64 KB: 50–130 ms each time a file opens in a
//! release build, close to a second in a debug one, and the same again on
//! every keystroke. This one parses whole documents on the background
//! executor. While typing it reparses only the edited lines, starting from
//! the nearest saved parse state, and lets a background parse fix up the
//! rest of the document.
//!
//! The scope-to-name mapping follows gpui-component's syntect backend
//! (Apache-2.0), so colors match the rest of the app.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::Duration;

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::{App, Context, HighlightStyle, SharedString, Task, Window};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Documents this small are parsed in place: well under a millisecond.
const SYNC_MAX_BYTES: usize = 4 * 1024;
/// A parse state is saved at the start of every this many lines.
const CHECKPOINT_LINES: usize = 32;
/// The most text an edit reparses in place before leaving it to the
/// background parse.
const LOCAL_MAX_BYTES: usize = 16 * 1024;
/// Quiet time after the last edit before the whole document is reparsed.
const PARSE_DEBOUNCE: Duration = Duration::from_millis(100);

/// Highlight names, ordered and non-overlapping.
type Highlights = Vec<(Range<usize>, &'static str)>;

/// Loads the syntax definitions in the background so the first file opened
/// doesn't wait for them.
pub fn prewarm(cx: &App) {
    cx.background_executor()
        .spawn(async {
            LazyLock::force(&SYNTAXES);
        })
        .detach();
}

pub fn factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        find_syntax(language).map(|syntax| Box::new(Highlighter::new(language, syntax)) as _)
    })
}

/// Installs the off-thread highlighter on a code editor.
pub fn install(state: &mut EditorState, cx: &mut Context<EditorState>) {
    state.set_highlighter_factory(factory(), cx);
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
            push(offset + cursor..offset + line.len(), &scopes, &mut out.highlights);
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

struct Highlighter {
    language: SharedString,
    syntax: &'static SyntaxReference,
    highlights: Rc<RefCell<Highlights>>,
    checkpoints: Rc<RefCell<Vec<Checkpoint>>>,
    folds: Rc<RefCell<Vec<FoldRange>>>,
    parse_task: Option<Task<()>>,
}

impl Highlighter {
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
        splice(&mut self.highlights.borrow_mut(), from.byte..end, parsed.highlights);
        checkpoints.extend(parsed.checkpoints.into_iter().skip(1));
    }
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i + 1)
}

impl InputHighlighter for Highlighter {
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
            "class", "enum", "function", "impl", "interface", "module", "namespace", "struct",
            "trait", "type", "let", "var", "const",
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

/// Fold candidates from matching braces outside strings and line comments.
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

#[cfg(test)]
mod tests {
    use super::{find_syntax, parse_all, parse_lines, splice};

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
        assert!(find_syntax("text").is_none());
        assert!(find_syntax("markdown").is_some());
        assert!(find_syntax("sql").is_some());
    }
}
