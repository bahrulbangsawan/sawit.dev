//! Tests of picking a backend, and of each backend's parse on and off the UI thread.

use super::syntect_highlighter::{find_syntax, parse_all, parse_lines, syntect_fallback};
use super::{Backend, backend, splice};

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
    let highlights = syntect_fallback("kotlin", "fun main() {}\n").unwrap();
    assert_eq!(highlights.first().map(|(_, name)| *name), Some("keyword"));
    let grammar = super::grammars::grammar("markdown").unwrap();
    let tree = grammar
        .parse(&mut tree_sitter::Parser::new(), text, None, &|| false)
        .unwrap();
    let highlights = grammar.highlight(&tree, text, 0..text.len(), &syntect_fallback, &|| false);
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
    let rope = gpui_kit::component::input::Rope::from(text);
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
        grammar.highlight(&tree, text, 0..text.len(), &syntect_fallback, &|| false)
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
