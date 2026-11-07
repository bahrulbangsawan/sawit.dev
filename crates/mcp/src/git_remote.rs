//! The Git tab's remote buttons for agents: fetch, pull (fast-forward only) and push (never
//! forced), with the user's own git credentials first and Sawit's GitHub sign-in for an HTTPS
//! remote on github.com that nothing else answers for.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use sawit_core::git;

use crate::git::{RepoArgs, status_text};
use crate::{Sawit, fail, ok, tri};

#[derive(Clone, Copy)]
enum Remote {
    Fetch,
    Pull,
    Push,
}

impl Sawit {
    /// fetch, pull or push: the Git tab's buttons. The user's own git credentials go first; for
    /// an HTTPS remote on github.com nothing answers for, Sawit's GitHub sign-in, if there is one.
    async fn git_remote_tool(&self, repo: &str, action: Remote) -> CallToolResult {
        let repo = tri!(Self::repo(repo).await);
        let token = crate::blocking(crate::shell_sync::token)
            .await
            .ok()
            .and_then(Result::ok);
        let token = token.as_deref();
        let done = match action {
            Remote::Fetch => git::fetch(&repo, token).await,
            Remote::Pull => git::pull(&repo, token).await,
            Remote::Push => {
                let upstream = match git::status(&repo).await {
                    Ok(s) => s.upstream.is_some(),
                    Err(e) => return fail(e.to_string()),
                };
                git::push(&repo, upstream, token).await
            }
        };
        let verb = match action {
            Remote::Fetch => "Fetched",
            Remote::Pull => "Pulled",
            Remote::Push => "Pushed",
        };
        match done {
            Ok(out) => {
                let status = git::status(&repo)
                    .await
                    .map(|s| status_text(&s))
                    .unwrap_or_default();
                let out = out.trim();
                ok(if out.is_empty() {
                    format!("{verb}.\n{status}")
                } else {
                    format!("{verb}.\n{out}\n{status}")
                })
            }
            Err(e) => fail(e.to_string()),
        }
    }
}

#[tool_router(router = git_remote_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Fetch every remote (git fetch --all --prune), as the Git tab's Fetch does; then the status. Uses the user's git credentials, else Sawit's GitHub sign-in for github.com"
    )]
    async fn git_fetch(&self, Parameters(a): Parameters<RepoArgs>) -> CallToolResult {
        self.git_remote_tool(&a.repo, Remote::Fetch).await
    }

    #[tool(
        description = "Pull the current branch, fast-forward only (a diverged branch is left for an explicit merge), as the Git tab's Pull does; then the status"
    )]
    async fn git_pull(&self, Parameters(a): Parameters<RepoArgs>) -> CallToolResult {
        self.git_remote_tool(&a.repo, Remote::Pull).await
    }

    #[tool(
        description = "Push the current branch, setting its upstream on origin the first time, as the Git tab's Push does; then the status. Never forces"
    )]
    async fn git_push(&self, Parameters(a): Parameters<RepoArgs>) -> CallToolResult {
        self.git_remote_tool(&a.repo, Remote::Push).await
    }
}
