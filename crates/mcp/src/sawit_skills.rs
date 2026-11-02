//! `install_sawit_skills`: what the footer's Install Skills button does, for agents. Globally, the
//! Sawit pack's skills into every agent, the block into each agent's own instruction file and
//! Sawit's MCP server into each client that lacks it; for a project, the block into its
//! instruction file the way TanStack Intent picks it. Or all of that taken out again.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::sawit_skills::{self, Scope};

use crate::{Sawit, agent_project, existing_dir, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstallSawitSkillsArgs {
    #[schemars(
        description = "\"global\" (default): every agent on this Mac, the skills, each agent's own instruction file (~/.claude/CLAUDE.md, ~/.codex/AGENTS.md, ~/.gemini/GEMINI.md, …) and Sawit's MCP server where a client lacks it. \"project\": only the block in one project's instruction file: the one of AGENTS.md, CLAUDE.md, .cursorrules and .github/copilot-instructions.md that has it, else the one the project has (the first, if several), else a new AGENTS.md"
    )]
    pub scope: Option<String>,
    #[schemars(
        description = "For scope project: the project folder (absolute, or ~/…). Default: the project your working folder is in"
    )]
    pub project: Option<String>,
    #[schemars(
        description = "true takes Sawit Skills out instead: the pack's skills (Sawit's own copies only), the block (a file deleted only when Sawit made it and nothing else is left) and the MCP entries an install added; an entry that was there before stays. Default false"
    )]
    pub remove: Option<bool>,
    #[schemars(
        description = "Must be true: this writes the user's agent instruction files and skills folders (and, globally, AI clients' MCP configs)"
    )]
    pub confirm: bool,
}

impl Sawit {
    pub async fn install_sawit_skills_tool(&self, a: InstallSawitSkillsArgs) -> CallToolResult {
        if !a.confirm {
            return fail(
                "Refused: this writes the user's agent instruction files, skills folders and AI clients' MCP configs. Ask the user, then pass confirm=true.",
            );
        }
        let scope = match a.scope.as_deref().map(str::trim) {
            None | Some("" | "global") => Scope::Global,
            Some("project") => {
                let dir: PathBuf = match a.project.as_deref() {
                    Some(p) if !p.trim().is_empty() => tri!(existing_dir(p)),
                    _ => tri!(agent_project(None).filter(|d| d.is_dir()).ok_or(
                        "Name the project folder: `project`, an absolute path.".to_string()
                    )),
                };
                Scope::Project(dir)
            }
            Some(other) => {
                return fail(format!(
                    "scope is \"global\" or \"project\", not \"{other}\"."
                ));
            }
        };
        let removing = a.remove.unwrap_or(false);
        // This `sawit-mcp`: what the entries it adds run, as install_mcp_server writes them.
        let server = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("sawit-mcp"));
        let outcome = if removing {
            sawit_skills::remove(&scope, &self.paths).await
        } else {
            sawit_skills::install(&scope, &self.paths, &server).await
        };
        let outcome = tri!(outcome);
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let mut text = vec![outcome.summary(removing)];
        text.extend(outcome.lines(&home));
        if !removing && outcome.changes() > 0 {
            text.push("Start a new session of those agents to pick it up.".into());
        }
        if outcome.failures() > 0 {
            fail(text.join("\n"))
        } else {
            ok(text.join("\n"))
        }
    }
}

#[tool_router(router = sawit_skills_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Install Sawit Skills, as the footer's Install Skills button does: the Sawit pack's skills (sawit, sawit-databases, sawit-browser-testing, sawit-canvas, sawit-terminal) into every agent's skills folder, a short block between <!-- sawit:start --> and <!-- sawit:end --> into each agent's own instruction file (Claude Code per profile, Codex, Gemini CLI, opencode, …), and Sawit's MCP server into each AI client that has no entry for it. scope project puts only the block into one project's instruction file. Updates what is there in place, keeps everything outside the block, copies a file into Sawit's data folder before first changing it, and says every path it changed. remove=true takes it out again, MCP entries included, but only those it added. Writes the user's files: needs confirm=true. Works without the app"
    )]
    async fn install_sawit_skills(
        &self,
        Parameters(a): Parameters<InstallSawitSkillsArgs>,
    ) -> CallToolResult {
        self.install_sawit_skills_tool(a).await
    }
}
