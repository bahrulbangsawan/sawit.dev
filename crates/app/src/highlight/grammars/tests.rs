//! Tests of the grammars: their queries, each language's colors, edits, stops and folds.

use super::folds::closes;
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
            let fresh = grammar.highlight(&reparsed, &after, range.clone(), &no_fallback, &never);
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
