//! The syntect backend: TextMate grammars for the languages without a tree-sitter grammar here.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Task, Window};
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference};
use syntect::util::LinesWithEndings;

use super::grammars::Highlights;
use super::{
    CHECKPOINT_LINES, LOCAL_MAX_BYTES, PARSE_DEBOUNCE, SYNC_MAX_BYTES, SYNTAXES, resolve_styles,
    shift, splice,
};

pub(super) fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
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
pub(super) fn syntect_fallback(language: &str, code: &str) -> Option<Highlights> {
    find_syntax(language).map(|syntax| parse_all(syntax, code).highlights)
}

/// Where a line starts and the parser state at that point.
#[derive(Clone)]
pub(super) struct Checkpoint {
    pub(super) byte: usize,
    parser: ParseState,
    scopes: ScopeStack,
}

#[derive(Default)]
pub(super) struct Parsed {
    pub(super) highlights: Highlights,
    pub(super) checkpoints: Vec<Checkpoint>,
}

/// Parses `text` from `from` (a line start) up to the end of the line that
/// contains `until`, saving a checkpoint every `CHECKPOINT_LINES` lines.
pub(super) fn parse_lines(text: &str, from: Checkpoint, until: usize) -> Parsed {
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

pub(super) fn parse_all(syntax: &SyntaxReference, text: &str) -> Parsed {
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

pub(super) struct SyntectHighlighter {
    language: SharedString,
    syntax: &'static SyntaxReference,
    highlights: Rc<RefCell<Highlights>>,
    checkpoints: Rc<RefCell<Vec<Checkpoint>>>,
    folds: Rc<RefCell<Vec<FoldRange>>>,
    parse_task: Option<Task<()>>,
}

impl SyntectHighlighter {
    pub(super) fn new(language: &str, syntax: &'static SyntaxReference) -> Self {
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
