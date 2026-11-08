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

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct PoolArgs {
    #[schemars(
        description = "overview (every account: status, room left, limits, resets, and the pick), detailed (the same grouped by agent or urgency), compare (the account with the most room now, why, the next best, what holds each back), timeline (each account's limit windows and resets in the history), wallet (prices, monthly total, renewals in the next 30 days, top-ups and their expiry) or analytics (pace forecasts, busiest hour, plan fit, suggestions to move work). Default overview"
    )]
    pub view: Option<String>,
    #[schemars(
        description = "Only this agent's accounts: claude-code or codex. Default: every agent"
    )]
    pub provider: Option<String>,
    #[schemars(description = "Days of history for timeline and analytics (30 or 90). Default 30")]
    pub days: Option<u32>,
}

pub(crate) fn ctx(s: &Sawit) -> (std::path::PathBuf, std::path::PathBuf) {
    (
        s.paths.root.clone(),
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()),
    )
}

pub(crate) fn pick<'a>(
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
        description = "The account pool across coding agents (Claude Code and Codex accounts on this Mac), as the Accounts page's pool views show it, as JSON. Ask before long work: view compare names the account with the most room left right now and why. Readings are what each agent reported itself (Claude Code's status line, Codex's session logs); nothing is estimated and no sign-in is read. Works without the app"
    )]
    async fn account_pool(&self, Parameters(a): Parameters<PoolArgs>) -> CallToolResult {
        use sawit_core::agent_accounts::pool::{self, LimitKind, Provider};
        use sawit_core::agent_accounts::pool_data::{self, Known};
        let (root, home) = ctx(self);
        let view = a.view.unwrap_or_else(|| "overview".into());
        let provider = match a.provider.as_deref() {
            None | Some("") => None,
            Some(p) => match Provider::from_id(p) {
                Some(p) => Some(p),
                None => return fail(format!("No agent “{p}”: claude-code or codex.")),
            },
        };
        let days = a.days.unwrap_or(30).clamp(1, 120) as f64;
        let now = sawit_core::github::now() as f64;
        // Read only: the app records readings; a second writer would store them twice.
        let data = tri!(
            tri!(
                blocking(move || pool_data::collect(
                    &root,
                    &home,
                    &Known::nothing(),
                    now,
                    days,
                    false
                ))
                .await
            )
            .map_err(|e| e.to_string())
        );
        let accounts: Vec<_> = data
            .accounts
            .iter()
            .filter(|x| provider.is_none_or(|p| x.provider == p))
            .cloned()
            .collect();
        let name = |id: &str| accounts.iter().find(|x| x.id == id).map(|x| x.name.clone());
        let out = match view.as_str() {
            "overview" | "detailed" => {
                let mut o = pool::overview_json(&accounts, now);
                if view == "detailed" {
                    let mut by: std::collections::BTreeMap<&str, Vec<serde_json::Value>> = Default::default();
                    for (x, row) in accounts.iter().zip(o["accounts"].as_array().cloned().unwrap_or_default()) {
                        by.entry(x.provider.id()).or_default().push(row);
                    }
                    o["by_provider"] = json!(by);
                    let mut urgent = accounts.clone();
                    urgent.sort_by_key(|x| pool::status(x, now).urgency());
                    o["by_urgency"] = json!(urgent.iter().map(|x| &x.name).collect::<Vec<_>>());
                }
                o["notices"] = json!(data.notices.iter().take(10).collect::<Vec<_>>());
                o
            }
            "compare" => match pool::recommend(&accounts, None, now) {
                Some(r) => json!({
                    "recommended": name(&r.account),
                    "room_left_pct": r.room,
                    "holding_limit": r.holding.map(|k| k.id()),
                    "why": r.reason,
                    "next_best": r.next.as_ref().map(|(id, room)| json!({"name": name(id), "room_left_pct": room})),
                    "skipped": r.skipped.iter().map(|(n, why)| json!({"name": n, "why": why})).collect::<Vec<_>>(),
                    "note": "Room is a share of each plan: the same percentage of a bigger plan is more work.",
                }),
                None => json!({"recommended": null, "why": "No account has reported its limits yet."}),
            },
            "timeline" => json!(accounts.iter().map(|x| {
                let r = data.readings.get(&x.id).cloned().unwrap_or_default();
                json!({
                    "name": x.name,
                    "windows": LimitKind::ALL.iter().flat_map(|k| pool::windows(&r, *k)).collect::<Vec<_>>(),
                    "recent_resets": LimitKind::ALL.iter().flat_map(|k| pool::resets_between(&pool::windows(&r, *k), now - 7.0 * 86400.0, now)).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>()),
            "wallet" => {
                let w = &data.wallet;
                let (month, year) = w.totals();
                let offset = sawit_core::date::local_offset(now as i64);
                json!({
                    "currency": w.currency,
                    "per_month": month,
                    "per_year": year,
                    "prices": accounts.iter().map(|x| json!({"name": x.name, "per_month": w.monthly(&x.id), "price": w.prices.get(&x.id)})).collect::<Vec<_>>(),
                    "renewals_next_30_days": w.renewals(now as i64, 30, offset).into_iter().map(|(id, at, amount)| json!({"name": name(&id), "at": at, "amount": amount})).collect::<Vec<_>>(),
                    "topups": w.topups,
                    "expiring_soon": w.expiring(now as i64),
                    "topups_by_month": w.topups_by_month(now as i64, 6, offset),
                    "removed": w.removed,
                })
            }
            "analytics" => {
                let rows: Vec<_> = accounts.iter().map(|x| {
                    let r = data.readings.get(&x.id).cloned().unwrap_or_default();
                    let heat = pool::heatmap(&r, LimitKind::FiveHour, &|t| sawit_core::date::local_offset(t as i64));
                    json!({
                        "name": x.name,
                        "forecast": x.current(now).filter_map(|l| pool::forecast(l, now)).collect::<Vec<_>>(),
                        "busiest": heat.busiest().map(|(d, h, _)| format!("{} {h:02}:00", sawit_core::date::WEEKDAYS[d])),
                        "plan_fit": pool::plan_fit(&pool::windows(&r, LimitKind::Weekly), data.wallet.monthly(&x.id)),
                    })
                }).collect();
                json!({"accounts": rows, "move_work": pool::move_work(&accounts, now).into_iter().map(|m| json!({"from": name(&m.from), "to": name(&m.to), "why": m.why})).collect::<Vec<_>>()})
            }
            other => return fail(format!("No view “{other}”: overview, detailed, compare, timeline, wallet or analytics.")),
        };
        ok(serde_json::to_string_pretty(&out).unwrap_or_default())
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
