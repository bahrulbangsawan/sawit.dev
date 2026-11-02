//! The block as text: put into an empty file, at the top of one with text (after its
//! frontmatter), updated in place with every byte around it kept, idempotent, taken out to give
//! the file back, and refused when its markers are damaged.

use super::*;
use crate::sawit_skills::block::{self, Damage, END, START};

const OLD: &str =
    "<!-- sawit:start -->\n<!-- Sawit Skills v0: old -->\nold text\n<!-- sawit:end -->\n";

#[test]
fn an_empty_file_gets_the_block_alone() {
    assert_eq!(block::put("", &ours()).unwrap(), ours());
    assert_eq!(block::take(&ours()).unwrap().as_deref(), Some(""));
}

/// Like TanStack Intent, a file without the block gets it at the top, then a blank line, then the
/// file as it was; taking it out gives back every byte.
#[test]
fn a_file_with_text_gets_it_at_the_top_and_back_exactly() {
    for text in [
        "# Mine\n\nKeep this.\n",
        "no newline at the end",
        "\nstarts with a blank line\n",
        "two blank lines at the end\n\n\n",
        "\n",
    ] {
        let put = block::put(text, &ours()).unwrap();
        assert_eq!(put, format!("{}\n{text}", ours()), "{text:?}");
        assert_eq!(
            block::take(&put).unwrap().as_deref(),
            Some(text),
            "{text:?}"
        );
    }
}

/// A file that starts with frontmatter keeps it first: the block goes right after it.
#[test]
fn frontmatter_stays_first() {
    let text = "---\napplyTo: \"**\"\n---\n# Rules\n";
    let put = block::put(text, &ours()).unwrap();
    assert_eq!(
        put,
        format!("---\napplyTo: \"**\"\n---\n{}\n# Rules\n", ours())
    );
    assert_eq!(block::take(&put).unwrap().as_deref(), Some(text));
}

/// An older block is replaced where it is, and nothing before or after it changes by a byte; a
/// second put changes nothing.
#[test]
fn an_update_replaces_only_the_block() {
    let before = "# Top\r\nmine\r\n\r\n";
    let after = "\r\n## Bottom\r\nalso mine";
    let text = format!("{before}{}{after}", OLD.replace('\n', "\r\n"));
    let put = block::put(&text, &ours()).unwrap();
    // The file's CRLF line endings carry into the block.
    let expected = format!("{before}{}{after}", ours().replace('\n', "\r\n"));
    assert_eq!(put, expected);
    assert_eq!(block::put(&put, &ours()).unwrap(), put);
    // In the middle of the file, the block goes and only it.
    assert_eq!(
        block::take(&put).unwrap().as_deref(),
        Some(format!("{before}{after}").as_str())
    );
}

/// Two whole blocks (pasted twice): the first is updated, the second taken out; a removal takes
/// both. Markers in a fenced code block are text, not a block.
#[test]
fn duplicates_join_and_fenced_markers_are_text() {
    let text = format!("a\n{OLD}b\n{OLD}c\n");
    let put = block::put(&text, &ours()).unwrap();
    assert_eq!(put, format!("a\n{}b\nc\n", ours()));
    assert_eq!(block::find(&put).unwrap().len(), 1);
    assert_eq!(block::take(&text).unwrap().as_deref(), Some("a\nb\nc\n"));

    let shown = format!("# The format\n```md\n{START}\n…\n{END}\n```\n");
    assert!(block::find(&shown).unwrap().is_empty());
    assert_eq!(block::take(&shown).unwrap(), None);
    let put = block::put(&shown, &ours()).unwrap();
    assert_eq!(put, format!("{}\n{shown}", ours()));
    assert_eq!(block::take(&put).unwrap().as_deref(), Some(shown.as_str()));
}

/// Damaged markers: Sawit can't tell where its block ends, so neither put nor take changes the
/// file, and the reason names the line.
#[test]
fn damaged_markers_are_refused() {
    let cases = [
        (format!("x\n{START}\nno end\n"), Damage::Unclosed(2)),
        (format!("x\ny\n{END}\n"), Damage::Unopened(3)),
        (format!("{START}\n{START}\n{END}\n"), Damage::Nested(2)),
        (format!("{START}\na\n{END}\n{END}\n"), Damage::Unopened(4)),
    ];
    for (text, damage) in cases {
        assert_eq!(block::put(&text, &ours()), Err(damage.clone()), "{text:?}");
        assert_eq!(block::take(&text), Err(damage.clone()), "{text:?}");
        let said = damage.describe("AGENTS.md");
        assert!(said.starts_with("AGENTS.md has a"), "{said}");
        assert!(said.contains("Sawit changes nothing there"), "{said}");
    }
}

/// The block is short, carries this Sawit's version, names every skill of the pack, says the
/// sawit MCP server has the tools, and what to do without it.
#[test]
fn the_block_says_what_to_prefer_and_where_the_tools_are() {
    let text = ours();
    assert!(text.starts_with(&format!("{START}\n")) && text.ends_with(&format!("{END}\n")));
    let lines = text.lines().count();
    assert!((10..=16).contains(&lines), "{lines} lines");
    assert_eq!(version_of(&text), Some(VERSION));
    assert_eq!(version_of(OLD), Some(0));
    let pack = crate::skills::get(crate::skills::SAWIT_PACK).unwrap();
    for skill in pack.skills {
        assert!(text.contains(&format!("`{skill}`")), "{skill}");
    }
    for word in [
        "Docker",
        "Chrome",
        "Playwright",
        "MCP server `sawit`",
        "isn't connected",
    ] {
        assert!(text.contains(word), "{word}");
    }
}
