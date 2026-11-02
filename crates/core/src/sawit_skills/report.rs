//! What an install or a removal did, per part, with every path: what the footer's dialog, the
//! `install_sawit_skills` tool and `sawit skills` say.

use std::path::{Path, PathBuf};

use super::agents::Skipped;
use super::files::Change;

/// The Sawit pack in one agent's skills folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillsOutcome {
    pub label: String,
    pub dir: PathBuf,
    /// What it holds now ("installed", "already current", "removed"), or why not.
    pub result: Result<String, String>,
}

/// The block in one instruction file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileOutcome {
    pub label: String,
    pub path: PathBuf,
    pub result: Result<Change, String>,
}

/// Sawit's MCP server in one client's config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpOutcome {
    pub label: String,
    pub config: PathBuf,
    /// "added", "already there", "left alone: …", or why not.
    pub result: Result<McpDone, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpDone {
    Added,
    Present,
    /// It has an entry named sawit that points at another `sawit-mcp` (what it runs): left alone.
    Elsewhere(String),
    /// Its config is YAML or a script, which Sawit doesn't edit: the entry to paste.
    ByHand,
    /// A removal took out the entry an install added.
    Removed,
    /// A removal left this entry named sawit: why (it was there before, it now points elsewhere).
    Kept(String),
}

/// Everything one run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    pub skills: Vec<SkillsOutcome>,
    pub files: Vec<FileOutcome>,
    pub mcp: Vec<McpOutcome>,
    /// Agents found without a file-based instruction file of their own, and why.
    pub skipped: Vec<Skipped>,
    /// Anything else to say: copies kept, the MCP server left in place on removal.
    pub notes: Vec<String>,
}

impl Outcome {
    /// How many parts failed.
    pub fn failures(&self) -> usize {
        self.skills.iter().filter(|s| s.result.is_err()).count()
            + self.files.iter().filter(|f| f.result.is_err()).count()
            + self.mcp.iter().filter(|m| m.result.is_err()).count()
    }

    /// How many files, folders and configs it changed.
    pub fn changes(&self) -> usize {
        let skills = self
            .skills
            .iter()
            .filter(|s| matches!(s.result.as_deref(), Ok("installed" | "updated" | "removed")))
            .count();
        let files = self
            .files
            .iter()
            .filter(|f| f.result.as_ref().is_ok_and(|c| c.done.changed()))
            .count();
        let mcp = self
            .mcp
            .iter()
            .filter(|m| matches!(m.result, Ok(McpDone::Added | McpDone::Removed)))
            .count();
        skills + files + mcp
    }

    /// Every part, one line each with its path (the home folder as `~`), under a heading per part.
    pub fn lines(&self, home: &Path) -> Vec<String> {
        let at = |p: &Path| tilde(p, home);
        let mark = |ok: bool, changed: bool| match (ok, changed) {
            (false, _) => "✗",
            (true, true) => "✓",
            (true, false) => "·",
        };
        let mut out = Vec::new();
        if !self.skills.is_empty() {
            out.push("Skills:".to_string());
            for s in &self.skills {
                let (ok, text) = match &s.result {
                    Ok(t) => (true, t.clone()),
                    Err(e) => (false, e.clone()),
                };
                let changed = matches!(text.as_str(), "installed" | "updated" | "removed");
                out.push(format!(
                    "  {} {}: {} ({text})",
                    mark(ok, changed),
                    s.label,
                    at(&s.dir)
                ));
            }
        }
        if !self.files.is_empty() {
            out.push("Instruction files:".to_string());
            for f in &self.files {
                let line = match &f.result {
                    Ok(change) => {
                        let copy = change
                            .backup
                            .as_ref()
                            .map(|b| format!("; its first copy: {}", at(b)))
                            .unwrap_or_default();
                        format!(
                            "  {} {}: {} ({}{copy})",
                            mark(true, change.done.changed()),
                            f.label,
                            at(&f.path),
                            change.done.verb()
                        )
                    }
                    Err(e) => format!("  ✗ {}: {} ({e})", f.label, at(&f.path)),
                };
                out.push(line);
            }
        }
        if !self.mcp.is_empty() {
            out.push("Sawit's MCP server:".to_string());
            for m in &self.mcp {
                let (ok, changed, text) = match &m.result {
                    Ok(McpDone::Added) => (true, true, "added".to_string()),
                    Ok(McpDone::Present) => (true, false, "already there".to_string()),
                    Ok(McpDone::Elsewhere(to)) => (
                        true,
                        false,
                        format!("left alone: its sawit entry runs {to}"),
                    ),
                    Ok(McpDone::Removed) => (
                        true,
                        true,
                        "removed: Install Skills had added it".to_string(),
                    ),
                    Ok(McpDone::Kept(why)) => (true, false, format!("stays: {why}")),
                    Ok(McpDone::ByHand) => (
                        true,
                        false,
                        "add it by hand: Sawit doesn't edit this config (the MCP Servers page has the entry)"
                            .to_string(),
                    ),
                    Err(e) => (false, false, e.clone()),
                };
                out.push(format!(
                    "  {} {}: {} ({text})",
                    mark(ok, changed),
                    m.label,
                    at(&m.config)
                ));
            }
        }
        if !self.skipped.is_empty() {
            out.push("No instruction file written:".to_string());
            for s in &self.skipped {
                out.push(format!("  · {}: {}", s.label, s.why));
            }
        }
        out.extend(self.notes.iter().cloned());
        out
    }

    /// One sentence: what changed, or that nothing needed to.
    pub fn summary(&self, removing: bool) -> String {
        let (changes, failures) = (self.changes(), self.failures());
        let verb = if removing { "removed" } else { "installed" };
        let nowhere = self.skills.is_empty() && self.files.is_empty() && self.mcp.is_empty();
        match (changes, failures) {
            (0, 0) if nowhere && !removing => {
                "No coding agent or AI client found on this Mac: nothing to install into.".into()
            }
            (0, 0) if removing => "Nothing of Sawit Skills was there to remove.".into(),
            (0, 0) => "Sawit Skills are already current everywhere.".into(),
            (n, 0) => format!("Sawit Skills {verb}: {} changed.", plural(n)),
            (n, f) => format!(
                "Sawit Skills {verb} with {f} {}: {} changed.",
                if f == 1 { "problem" } else { "problems" },
                plural(n)
            ),
        }
    }
}

fn plural(n: usize) -> String {
    format!("{n} {}", if n == 1 { "place" } else { "places" })
}

/// `path` with the home folder written `~`.
pub fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

impl From<(String, PathBuf, Result<Change, String>)> for FileOutcome {
    fn from((label, path, result): (String, PathBuf, Result<Change, String>)) -> Self {
        FileOutcome {
            label,
            path,
            result,
        }
    }
}
