//! `browser_tabs`: the tabs of the project's browser, for an agent to list, open, close and show;
//! the `browser_*` tools act on one by `tab`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Command;

use crate::app::pretty;
use crate::{Sawit, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct BrowserTabsArgs {
    #[schemars(description = "list (default), open, close or activate")]
    pub action: Option<String>,
    #[schemars(
        description = "The tab to close or activate: its id, title or address (list shows them)"
    )]
    pub tab: Option<String>,
    #[schemars(
        description = "For open: the address the new tab opens (default the project's dev server)"
    )]
    pub url: Option<String>,
    #[schemars(
        description = "Show the tab (open, activate) and bring the browser forward. Default false: the user's tab on show stays, and the agent works on the tab off screen with tab: <id>"
    )]
    #[serde(default)]
    pub show: bool,
    pub project: Option<String>,
}

#[tool_router(router = tabs_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "The tabs of Sawit's browser (one browser per project): list them (id, title, url, the one on show, whether it has its page loaded), open a new tab (behind the user's unless show), close one, or activate one. Act on any tab with the browser_* tools' tab argument; a tab not on show is woken off screen, and the user's tab and keyboard stay put"
    )]
    async fn browser_tabs(&self, Parameters(a): Parameters<BrowserTabsArgs>) -> CallToolResult {
        let command = Command::BrowserTabs {
            action: a.action.unwrap_or_else(|| "list".into()),
            tab: a.tab,
            url: a.url,
            show: a.show,
        };
        let v = tri!(self.app_call(a.project, command).await);
        ok(pretty(&v))
    }
}
