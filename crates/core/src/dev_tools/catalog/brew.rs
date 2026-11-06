//! Command-line tools from Homebrew formulas: the GitHub CLI.
//! Each copy found in Homebrew's Cellar is upgraded with `brew upgrade` of the formula its
//! install receipt names, and removed with `brew uninstall`; a copy installed another way is
//! detected and left alone ([`Tool::formula`]).

use super::super::{Category, Latest, Tool};
use super::tool;

/// The GitHub CLI, from Homebrew's core (`brew info gh`: homebrew/core, bottled). Its `.pkg`
/// from cli.github.com puts `gh` in /usr/local/bin, MacPorts in /opt/local/bin, conda in its
/// environment's bin: those copies are found and never upgraded or removed by Sawit, as they
/// aren't in Homebrew's Cellar. `gh --version` prints "gh version 2.103.3 (2026-10-26)" and
/// the release's URL. Signing in is `gh auth login`, which asks questions, so it runs in a
/// terminal; Sawit's own GitHub sign-in is separate, and its token is never copied into gh.
pub(super) const GH: Tool = Tool {
    formula: Some("gh"),
    latest: Latest::GitHub("cli/cli"),
    upgrade: Some("brew upgrade gh"),
    uninstall: Some("brew uninstall gh"),
    needs: &["homebrew"],
    ..tool(
        "gh",
        "GitHub CLI",
        "GitHub from the terminal: pull requests, issues, releases and Actions with gh.",
        Category::Other,
        "gh",
        "brew install gh",
    )
};
