//! The tree-sitter backend: an incremental parse, colors and folds, mostly off the UI thread.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use gpui_kit::component::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter, Rope,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Task, Window};
use tree_sitter::{Parser, Tree};

use super::grammars::{self, Grammar, Highlights};
use super::syntect_highlighter::syntect_fallback;
use super::{
    LOCAL_MAX_BYTES, LOCAL_PARSE_BUDGET, PARSE_DEBOUNCE, SYNC_MAX_BYTES, resolve_styles, shift,
    splice,
};

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
pub(super) struct Syntax {
    /// The latest parse, edited to match every change since. `None` before the first one, and
    /// after a change the editor can't describe as an edit.
    tree: Option<Tree>,
    pub(super) highlights: Highlights,
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

pub(super) struct TreeSitterHighlighter {
    language: SharedString,
    grammar: &'static Grammar,
    pub(super) syntax: Rc<RefCell<Syntax>>,
    /// Parses small documents, and edits within budget, on the UI thread.
    parser: Parser,
    /// `LOCAL_PARSE_BUDGET`, or more in a test that can't have its parse stopped by a busy machine.
    pub(super) parse_budget: Duration,
    parse_task: Option<Task<()>>,
}

impl TreeSitterHighlighter {
    pub(super) fn new(language: &str, grammar: &'static Grammar) -> Self {
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
