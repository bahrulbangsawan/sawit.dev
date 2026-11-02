//! Where Sawit Skills stand on this Mac, for the footer's button and the Skills page: not
//! installed, installed and current, or something to update (this Sawit ships a newer version,
//! an agent found since has none, a client lacks Sawit's MCP server).

use std::path::{Path, PathBuf};

use super::agents::{AgentFile, Skipped, agent_files};
use super::files::{self, Holds};
use crate::dev_tools::find_program;
use crate::mcp_catalog::{self, DEFAULT_AWS_REGION};
use crate::mcp_clients::{self, Status};
use crate::skills::{self, SAWIT_PACK};

/// What the button says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Nothing of it is anywhere: "Install Skills".
    NotInstalled,
    /// Some of it is there, and an install would change something: "Update Skills".
    Update,
    /// Every agent has this Sawit's skills and block, and every client Sawit's MCP server:
    /// "Skills Installed".
    Installed,
}

/// Sawit Skills on this Mac, part by part.
#[derive(Debug, Clone)]
pub struct State {
    pub level: Level,
    /// Each agent's skills folder, its label and the pack's status there; `None` when a folder of
    /// a skill's name there isn't Sawit's (an install leaves that agent out).
    pub skills: Vec<(String, PathBuf, Option<skills::Status>)>,
    /// Each agent's instruction file and what it holds.
    pub files: Vec<(AgentFile, Holds)>,
    /// The clients that have no entry for Sawit's MCP server, which an install adds.
    pub mcp_missing: Vec<String>,
    pub skipped: Vec<Skipped>,
}

impl State {
    /// "3 of 4 agents have the skills; 2 of 4 files have the block": what the button's tooltip
    /// says.
    pub fn detail(&self) -> String {
        let skills = self
            .skills
            .iter()
            .filter(|(_, _, s)| matches!(s, Some(skills::Status::Installed { .. })))
            .count();
        let files = self.files.iter().filter(|(_, h)| h.current()).count();
        let mut out = format!(
            "Skills current in {skills} of {} agents; the block current in {files} of {} instruction files",
            self.skills.len(),
            self.files.len()
        );
        if !self.mcp_missing.is_empty() {
            out.push_str(&format!(
                "; Sawit's MCP server missing in {}",
                self.mcp_missing.join(", ")
            ));
        }
        out
    }
}

/// Where Sawit Skills stand: the agents and clients found on this Mac (from `HOME`), and
/// `server`, the `sawit-mcp` an install would add. Reads files: run it off the main thread.
pub fn state(server: &Path) -> State {
    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let pack = skills::get(SAWIT_PACK).expect("the catalog has the Sawit pack");
    let report = skills::report();
    let skills: Vec<(String, PathBuf, Option<skills::Status>)> = report
        .targets
        .iter()
        .zip(&report.installed)
        .map(|(t, i)| {
            let theirs = skills::not_sawits(pack.skills, &[(t, i)]).is_some();
            let status = (!theirs).then(|| skills::status(pack, i));
            (t.label.clone(), t.dir.clone(), status)
        })
        .collect();
    let clients = mcp_clients::targets();
    let (agent_files, skipped) = agent_files(&home, &clients);
    let files: Vec<(AgentFile, Holds)> = agent_files
        .into_iter()
        .map(|f| {
            let holds = files::holds(&f.path);
            (f, holds)
        })
        .collect();
    let mut mcp_missing = Vec::new();
    if server.is_file()
        && let Some(sawit) = mcp_catalog::get(mcp_clients::SERVER_NAME)
    {
        let spec = sawit.spec(server, DEFAULT_AWS_REGION, find_program);
        mcp_missing = clients
            .iter()
            .filter(|t| mcp_clients::status(t, &spec) == Status::NotInstalled)
            .map(super::agents::label_of)
            .collect();
    }
    let level = level(&skills, &files, &mcp_missing);
    State {
        level,
        skills,
        files,
        mcp_missing,
        skipped,
    }
}

/// Not installed while no agent has any of it; installed when an install would change nothing it
/// can (a folder or file Sawit leaves alone doesn't count); else an update.
fn level(
    skills: &[(String, PathBuf, Option<skills::Status>)],
    files: &[(AgentFile, Holds)],
    mcp_missing: &[String],
) -> Level {
    let any = skills.iter().any(|(_, _, s)| s.is_some_and(|s| s.any()))
        || files.iter().any(|(_, h)| h.any());
    if !any {
        return Level::NotInstalled;
    }
    let skills_current = skills
        .iter()
        .all(|(_, _, s)| s.is_none_or(|s| matches!(s, skills::Status::Installed { .. })));
    let files_current = files
        .iter()
        .all(|(_, h)| h.current() || matches!(h, Holds::Damaged(_) | Holds::Unreadable(_)));
    if skills_current && files_current && mcp_missing.is_empty() {
        Level::Installed
    } else {
        Level::Update
    }
}

#[cfg(test)]
pub(super) fn level_for(
    skills: &[(String, PathBuf, Option<skills::Status>)],
    files: &[(AgentFile, Holds)],
    mcp_missing: &[String],
) -> Level {
    level(skills, files, mcp_missing)
}
