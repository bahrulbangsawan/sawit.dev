//! Text search in a folder (ripgrep when installed, else grep).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::search;

use crate::{Sawit, existing_dir, fail, ok, tri};

const LIMIT: usize = 100;
const MAX_LIMIT: usize = 1000;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchArgs {
    #[schemars(description = "Folder to search (absolute path)")]
    pub dir: String,
    #[schemars(
        description = "Text to find (literal, case-insensitive unless it has capitals)"
    )]
    pub query: String,
    #[schemars(description = "Maximum matches (default 100, max 1000)")]
    pub limit: Option<usize>,
}

impl Sawit {
    async fn search_files_tool(&self, a: SearchArgs) -> CallToolResult {
        let dir = tri!(existing_dir(&a.dir));
        if a.query.is_empty() {
            return fail("The search text is empty.");
        }
        let limit = a.limit.unwrap_or(LIMIT).clamp(1, MAX_LIMIT);
        match search::find(&dir, &a.query, limit).await {
            Ok(hits) if hits.is_empty() => ok(format!("No matches for “{}”.", a.query)),
            Ok(hits) => {
                let mut out: Vec<String> = hits
                    .iter()
                    .map(|h| {
                        let path = h.path.strip_prefix(&dir).unwrap_or(&h.path);
                        format!("{}:{}: {}", path.display(), h.line, h.text.trim())
                    })
                    .collect();
                if hits.len() >= limit {
                    out.push(format!("(stopped at {limit} matches)"));
                }
                ok(out.join("\n"))
            }
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = files_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Search text in the files of a folder (skips what .gitignore ignores when ripgrep is installed). Returns path:line: text"
    )]
    async fn search_files(&self, Parameters(a): Parameters<SearchArgs>) -> CallToolResult {
        self.search_files_tool(a).await
    }
}
