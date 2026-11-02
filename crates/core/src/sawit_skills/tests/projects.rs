//! A project's block goes in the file its agents read: the one that has it, else the one the
//! project has (CLAUDE.md alone: CLAUDE.md, which Claude Code reads), else a new AGENTS.md.

use std::path::Path;

use crate::sawit_skills::project::{files_for_put, files_with_block};

fn picked(dir: &Path) -> Vec<String> {
    files_for_put(dir)
        .iter()
        .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
        .collect()
}

fn write(dir: &Path, rel: &str, text: &str) {
    let path = dir.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[test]
fn the_block_goes_in_the_file_the_project_has() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path();
    // Nothing there: a new AGENTS.md.
    assert_eq!(picked(dir), ["AGENTS.md"]);
    // One file of them: that one.
    for rel in [
        "CLAUDE.md",
        ".cursorrules",
        ".github/copilot-instructions.md",
    ] {
        let only = tempfile::tempdir().unwrap();
        write(only.path(), rel, "# Mine\n");
        assert_eq!(picked(only.path()), [rel], "{rel}");
    }
    // Several: the first in Intent's order.
    write(dir, ".cursorrules", "rules\n");
    write(dir, "CLAUDE.md", "# Mine\n");
    assert_eq!(picked(dir), ["CLAUDE.md"]);
    write(dir, "AGENTS.md", "# Ours\n");
    assert_eq!(picked(dir), ["AGENTS.md"]);
    assert!(files_with_block(dir).is_empty());
    // The one that has the block wins, wherever it is in the order; each that has one is updated.
    write(dir, ".cursorrules", &crate::sawit_skills::block_text());
    assert_eq!(picked(dir), [".cursorrules"]);
    write(dir, "CLAUDE.md", &crate::sawit_skills::block_text());
    assert_eq!(picked(dir), ["CLAUDE.md", ".cursorrules"]);
}
