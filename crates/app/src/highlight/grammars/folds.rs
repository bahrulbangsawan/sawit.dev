//! Fold candidates read from a tree: the lines a node spans, up to the line that closes it.

use std::ops::Range;

use tree_sitter::{Node, Tree};

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
pub(super) fn closes(line: &str) -> bool {
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
