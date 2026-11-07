//! Agent Accounts for agents: Claude Code's accounts, their usage, switching the active one,
//! opening a terminal as one, the running sessions, and shared settings. Never a sign-in (only
//! the user signs in, on the Accounts page) and never a removal (the page asks the user).

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::agent_accounts::{self as aa, Accounts, AgentKind, run, sessions, shared, usage};
use sawit_core::app_control::Command;
use serde_json::json;

use crate::{Sawit, blocking, fail, ok, tri};

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct AccountArgs {
    #[schemars(
        description = "The account: its name, its number in list_agent_accounts, or the start of one name. Default: the active one"
    )]
    pub account: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct UsageArgs {
    #[schemars(
        description = "The account (name, number or start of a name). Default: every account"
    )]
    pub account: Option<String>,
    #[schemars(
        description = "Ask for current numbers first: Claude Code sends one tiny request as the account (Haiku, no tools; a few hundred tokens of its usage). Without it, the last values Claude Code reported"
    )]
    #[serde(default)]
    pub check: bool,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct UseArgs {
    #[schemars(description = "The account to make active (name, number or start of a name)")]
    pub account: String,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct OpenArgs {
    #[schemars(
        description = "The account (name, number or start of a name). Default: the active one"
    )]
    pub account: Option<String>,
    #[schemars(description = "Folder to start in. Default: the project's")]
    pub folder: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct SyncArgs {
    #[schemars(
        description = "status (whether sharing is on) or run (one sync now; every file it changes is backed up first)"
    )]
    pub action: String,
}

fn ctx(s: &Sawit) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        s.paths.root.clone(),
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()),
    )
}

fn pick<'a>(
    all: &'a Accounts,
    what: Option<&str>,
    root: &std::path::Path,
) -> Result<&'a aa::Account, String> {
    match what.map(str::trim).filter(|w| !w.is_empty()) {
        Some(w) => all
            .find(AgentKind::ClaudeCode, w)
            .ok_or_else(|| format!("No account “{w}”: list_agent_accounts lists them.")),
        None => all.active(AgentKind::ClaudeCode, root).ok_or_else(|| {
            "There are no accounts yet: the user adds them on Sawit's Accounts page.".into()
        }),
    }
}

#[tool_router(router = agent_accounts_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List Claude Code's accounts in Sawit (the Accounts page; Agent Accounts, formerly Claude Pool): number, name, folder (CLAUDE_CONFIG_DIR, unset for the default), whether it is the active one new `claude` sessions use, whether usage tracking is on, and the 5-hour and 7-day usage Claude Code last reported, as JSON. Never a sign-in or token. Works without the app"
    )]
    async fn list_agent_accounts(&self) -> CallToolResult {
        let (root, _) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let active = all
            .active(AgentKind::ClaudeCode, &root)
            .map(|a| a.id.clone());
        let now = sawit_core::github::now() as f64;
        let cur = |w: Option<usage::Window>| {
            w.filter(|w| w.resets_at > now)
                .map(|w| w.used_percentage.round())
        };
        let list: Vec<_> = all
            .of(AgentKind::ClaudeCode)
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let u = usage::load_snapshot(&root, &a.id);
                json!({
                    "number": i + 1, "name": a.name, "dir": a.dir, "default": a.is_default,
                    "active": Some(&a.id) == active.as_ref(), "tracking": a.tracking,
                    "five_hour": u.as_ref().and_then(|u| cur(u.five_hour)),
                    "seven_day": u.as_ref().and_then(|u| cur(u.seven_day)),
                    "observed_at": u.and_then(|u| u.observed_at),
                })
            })
            .collect();
        ok(serde_json::to_string_pretty(&list).unwrap_or_default())
    }

    #[tool(
        description = "An account's (or every account's) 5-hour and 7-day usage as Claude Code reported it: percentage, reset time and when it was observed. check: true asks for current numbers first (one tiny Haiku request as the account, a few hundred tokens of its usage). Works without the app"
    )]
    async fn agent_account_usage(&self, Parameters(a): Parameters<UsageArgs>) -> CallToolResult {
        let (root, home) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let list: Vec<aa::Account> = match &a.account {
            Some(w) => vec![tri!(pick(&all, Some(w), &root)).clone()],
            None => all.of(AgentKind::ClaudeCode).into_iter().cloned().collect(),
        };
        let mut out = Vec::new();
        for acc in list {
            let snap = if a.check {
                let Some(bin) = run::find_claude(&home) else {
                    return fail("Claude Code isn't installed.");
                };
                let (root, home, acc2) = (root.clone(), home.clone(), acc.clone());
                tri!(
                    tri!(blocking(move || run::check_usage(&bin, &acc2, &home, &root)).await)
                        .map_err(|e| e.to_string())
                )
            } else {
                match usage::load_snapshot(&root, &acc.id) {
                    Some(s) => s,
                    None => {
                        out.push(json!({ "name": acc.name, "usage": null, "note": "no data yet" }));
                        continue;
                    }
                }
            };
            out.push(json!({ "name": acc.name, "five_hour": snap.five_hour, "seven_day": snap.seven_day, "observed_at": snap.observed_at }));
        }
        ok(serde_json::to_string_pretty(&out).unwrap_or_default())
    }

    #[tool(
        description = "Make a Claude Code account the active one: new `claude` sessions started with Sawit's shell block (or sawit account open) use it. Running sessions keep theirs: a session can't change account. Works without the app"
    )]
    async fn use_agent_account(&self, Parameters(a): Parameters<UseArgs>) -> CallToolResult {
        let (root, _) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let acc = tri!(pick(&all, Some(&a.account), &root));
        tri!(aa::write_active(&root, acc).map_err(|e| e.to_string()));
        ok(format!(
            "New claude sessions use {} ({}).",
            acc.name,
            acc.dir.display()
        ))
    }

    #[tool(
        description = "Open a new Sawit terminal tab running `claude` as a Claude Code account (CLAUDE_CONFIG_DIR set to its folder, unset for the default), in `folder` or the project's. Needs the app running"
    )]
    async fn open_agent_account(&self, Parameters(a): Parameters<OpenArgs>) -> CallToolResult {
        let (root, _) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let acc = tri!(pick(&all, a.account.as_deref(), &root));
        let command = match acc.env_value() {
            Some(d) => format!(
                "CLAUDE_CONFIG_DIR={} command claude",
                sawit_core::terminal::shell_quote(d)
            ),
            None => "unset CLAUDE_CONFIG_DIR; command claude".into(),
        };
        let cwd = match a
            .folder
            .as_deref()
            .filter(|f| !f.trim().is_empty())
            .map(crate::existing_dir)
            .transpose()
        {
            Ok(c) => c,
            Err(e) => return fail(e),
        };
        tri!(
            self.app_call(
                None,
                Command::TerminalRun {
                    command,
                    cwd,
                    new_tab: true,
                    close_when_done: false,
                }
            )
            .await
        );
        ok(format!("Opened a terminal running claude as {}.", acc.name))
    }

    #[tool(
        description = "The running Claude Code sessions of every account (from Claude Code's own registry, checked against the live processes): account, pid, folder, state (busy, idle), version. Works without the app"
    )]
    async fn agent_account_sessions(&self) -> CallToolResult {
        let (root, _) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        let list: Vec<_> = all
            .of(AgentKind::ClaudeCode)
            .iter()
            .flat_map(|acc| {
                sessions::sessions(acc, &sessions::process_alive).into_iter().map(move |s| {
                    json!({ "account": acc.name, "pid": s.pid, "folder": s.cwd, "status": s.status, "version": s.version })
                })
            })
            .collect();
        ok(serde_json::to_string_pretty(&list).unwrap_or_default())
    }

    #[tool(
        description = "Shared settings across Claude Code accounts (MCP servers, skills, plugins, hooks, permission rules; sign-ins and the status line stay per account). action status: whether sharing is on. action run: one sync now, each changed file backed up first; it runs only while the user has sharing on. Works without the app"
    )]
    async fn sync_agent_settings(&self, Parameters(a): Parameters<SyncArgs>) -> CallToolResult {
        let (root, home) = ctx(self);
        let all = tri!(Accounts::load(&root).map_err(|e| e.to_string()));
        match a.action.as_str() {
            "status" => ok(json!({ "sharing": all.settings.share_extensions, "accounts": all.of(AgentKind::ClaudeCode).len() }).to_string()),
            "run" if !all.settings.share_extensions => fail("Shared settings are off: the user turns them on in Accounts › Settings."),
            "run" => {
                let accounts: Vec<aa::Account> = all.of(AgentKind::ClaudeCode).into_iter().cloned().collect();
                let report = tri!(tri!(blocking(move || {
                    let refs: Vec<&aa::Account> = accounts.iter().collect();
                    shared::sync(&root, &home, &refs, sawit_core::github::now())
                })
                .await)
                .map_err(|e| e.to_string()));
                ok(if report.changes.is_empty() { "Everything was already the same.".into() } else { report.changes.join("\n") })
            }
            other => fail(format!("action is status or run, not {other:?}.")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(r: &CallToolResult) -> String {
        serde_json::to_value(r).unwrap()["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    /// The tools on a temp data folder with made-up accounts: listed, switched, never a secret.
    #[tokio::test]
    async fn accounts_are_listed_and_switched() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sawit");
        let home = dir.path().join("home");
        let mut all = Accounts::default();
        for (name, d, default) in [
            ("default", ".claude", true),
            ("work", ".claude-work", false),
        ] {
            all.add(aa::Account {
                id: aa::new_id(),
                kind: AgentKind::ClaudeCode,
                name: name.into(),
                dir: home.join(d),
                is_default: default,
                color: "#000000".into(),
                order: 0,
                tracking: false,
                original_status_line: None,
                created_at: 0,
            })
            .unwrap();
        }
        all.save(&root).unwrap();
        let sawit = Sawit::new(sawit_core::instances::AppPaths { root: root.clone() });
        let listed: serde_json::Value =
            serde_json::from_str(&text(&sawit.list_agent_accounts().await)).unwrap();
        assert_eq!(listed[1]["name"], "work");
        assert_eq!(listed[0]["active"], true);
        let r = sawit
            .use_agent_account(Parameters(UseArgs {
                account: "2".into(),
            }))
            .await;
        assert!(text(&r).contains("work"));
        assert_eq!(
            aa::read_active(&root, AgentKind::ClaudeCode),
            Some(home.join(".claude-work"))
        );
        let r = sawit
            .use_agent_account(Parameters(UseArgs {
                account: "nobody".into(),
            }))
            .await;
        assert_eq!(r.is_error, Some(true));
        let r = sawit
            .sync_agent_settings(Parameters(SyncArgs {
                action: "run".into(),
            }))
            .await;
        assert_eq!(r.is_error, Some(true), "sharing is off");
    }
}
