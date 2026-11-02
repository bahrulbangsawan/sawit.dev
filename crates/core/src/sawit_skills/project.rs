//! A project's instruction file: the one its agents read. Of AGENTS.md, CLAUDE.md, .cursorrules and
//! .github/copilot-instructions.md, the one that has the block (as TanStack Intent picks it), else
//! the one the project has (a CLAUDE.md-only project gets it in CLAUDE.md, which Claude Code
//! reads; with several, the first in that order), else AGENTS.md, made when no such file is there.

use std::path::{Path, PathBuf};

use super::files::{self, Holds};

/// The files a project's block can be in, in the order they are looked at.
pub const PROJECT_FILES: [&str; 4] = [
    "AGENTS.md",
    "CLAUDE.md",
    ".cursorrules",
    ".github/copilot-instructions.md",
];

/// Where the block goes in `project`: each of [`PROJECT_FILES`] that has one (a damaged one too,
/// so the damage is said); else the first of them the project has; else AGENTS.md.
pub fn files_for_put(project: &Path) -> Vec<PathBuf> {
    let having = files_with_block(project);
    if !having.is_empty() {
        return having;
    }
    let there = PROJECT_FILES
        .iter()
        .map(|rel| project.join(rel))
        .find(|path| path.is_file());
    vec![there.unwrap_or_else(|| project.join(PROJECT_FILES[0]))]
}

/// Each of [`PROJECT_FILES`] in `project` that has a block, or markers of one.
pub fn files_with_block(project: &Path) -> Vec<PathBuf> {
    PROJECT_FILES
        .iter()
        .map(|rel| project.join(rel))
        .filter(|path| matches!(files::holds(path), h if h.any() || matches!(h, Holds::Damaged(_))))
        .collect()
}
