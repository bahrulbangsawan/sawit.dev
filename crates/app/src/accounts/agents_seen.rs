//! The agents Sawit sees, for the pool's Overview: coding agents running in Sawit's terminals
//! (with the account each runs as, read from its `CLAUDE_CONFIG_DIR` or `CODEX_HOME`, its
//! project, its tab and its activity, B2) and agents using Sawit through MCP (on a canvas or in
//! the browser, with what they did last).

use std::path::{Path, PathBuf};

use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::activity::Activity;
use sawit_core::agent_accounts::{Accounts, AgentKind as AccountKind, codex};
use sawit_core::terminal::AgentKind;

use super::ui::{self, Tone};

/// A pane running a coding agent, as the terminal panel reports it.
#[derive(Clone, Debug)]
pub struct TerminalAgent {
    pub tab: usize,
    pub project: Option<PathBuf>,
    pub kind: AgentKind,
    pub activity: Activity,
    /// The account variable and folder from the agent's environment.
    pub profile: Option<(String, String)>,
}

/// One agent on the Overview.
#[derive(Clone, Debug, PartialEq)]
pub struct SeenAgent {
    pub kind: Option<AgentKind>,
    pub name: String,
    /// Where: "Terminal 2 · sawit.dev", "Canvas", "Browser".
    pub place: String,
    /// The account it runs as, when known.
    pub account: Option<String>,
    pub activity: Activity,
    /// What it did last, for an MCP client ("2 min ago").
    pub note: Option<String>,
}

/// The account a terminal agent runs as: the account whose folder its variable names, else the
/// agent's default account (no variable: the default folder).
pub fn account_of(t: &TerminalAgent, accounts: &Accounts, home: &Path) -> Option<String> {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    match (t.kind, &t.profile) {
        (AgentKind::Codex, Some((var, dir))) if var == "CODEX_HOME" => {
            Some(codex::name_of(Path::new(dir)))
        }
        (AgentKind::Codex, _) => Some("default".into()),
        (AgentKind::ClaudeCode, Some((var, dir))) if var == "CLAUDE_CONFIG_DIR" => {
            let dir = canon(Path::new(dir));
            accounts
                .of(AccountKind::ClaudeCode)
                .into_iter()
                .find(|a| canon(&a.dir) == dir)
                .map(|a| a.name.clone())
        }
        (AgentKind::ClaudeCode, _) => accounts
            .of(AccountKind::ClaudeCode)
            .into_iter()
            .find(|a| a.is_default || a.dir == home.join(".claude"))
            .map(|a| a.name.clone()),
        _ => None,
    }
}

/// The agents of one window: its terminals' and the MCP clients seen in `app_state.agents`.
pub fn collect(
    terminals: &[TerminalAgent],
    mcp: &[serde_json::Value],
    accounts: &Accounts,
    home: &Path,
) -> Vec<SeenAgent> {
    let mut out: Vec<SeenAgent> = terminals
        .iter()
        .map(|t| {
            let project = t
                .project
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| format!(" · {}", n.to_string_lossy()))
                .unwrap_or_default();
            SeenAgent {
                kind: Some(t.kind),
                name: t.kind.name().to_string(),
                place: format!("Terminal {}{project}", t.tab + 1),
                account: account_of(t, accounts, home),
                activity: t.activity,
                note: None,
            }
        })
        .collect();
    for a in mcp {
        let surface = a["surface"].as_str().unwrap_or("app");
        out.push(SeenAgent {
            kind: None,
            name: a["name"].as_str().unwrap_or("Agent").to_string(),
            place: match surface {
                "canvas" => "Canvas".into(),
                "browser" => "Browser".into(),
                s => s.to_string(),
            },
            account: None,
            activity: if a["seconds_ago"].as_u64().is_some_and(|s| s < 5) {
                Activity::Busy
            } else {
                Activity::Idle
            },
            note: a["action"].as_str().map(|s| s.to_string()),
        });
    }
    out
}

impl SeenAgent {
    /// Its row: the activity icon over the agent's logo, the name and place, the account it
    /// runs as, and its state in words.
    pub fn row(&self, ix: usize, window: &Window, cx: &App) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let logo = match self.kind {
            Some(kind) => crate::terminal::agents::logo(kind, px(14.), window, cx),
            None => Icon::new(gpui_kit::assets::IconName::Bot)
                .small()
                .text_color(muted)
                .into_any_element(),
        };
        let (tone, word) = match self.activity {
            Activity::Busy => (Tone::Neutral, "Working"),
            Activity::Attention => (Tone::Warn, "Needs you"),
            Activity::Failed { .. } => (Tone::Bad, "Failed"),
            Activity::Succeeded | Activity::Stopped => (Tone::Neutral, "Ended"),
            Activity::Idle => (
                Tone::Neutral,
                if self.kind.is_some() {
                    "Waiting"
                } else {
                    "Idle"
                },
            ),
        };
        let icon = crate::activity::glyph_of(self.activity);
        let place = match &self.account {
            Some(a) => format!("{} · as {a}", self.place),
            None => self.place.clone(),
        };
        h_flex()
            .id(("seen-agent", ix))
            .role(Role::ListItem)
            .aria_label(format!("{}, {place}, {word}", self.name))
            .px_4()
            .py_2p5()
            .gap_2()
            .child(div().flex_none().child(crate::activity::icon(
                self.activity,
                logo,
                px(14.),
                ("seen-agent-icon", ix),
                cx,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(self.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(match &self.note {
                                Some(n) => format!("{place} · {n}"),
                                None => place,
                            }),
                    ),
            )
            .child(ui::pill(("seen-agent-state", ix), tone, icon, word, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{TerminalAgent, account_of, collect};
    use sawit_core::activity::Activity;
    use sawit_core::agent_accounts::Accounts;
    use sawit_core::terminal::AgentKind;

    #[test]
    fn agents_map_to_their_accounts() {
        let home = std::path::Path::new("/h");
        let mut all = Accounts::default();
        let mut work = super::super::new_account("work", home.join(".claude-work"), false);
        work.id = "w".into();
        all.add(work).unwrap();
        all.add(super::super::new_account(
            "default",
            home.join(".claude"),
            true,
        ))
        .unwrap();
        let t = |kind, profile: Option<(&str, &str)>| TerminalAgent {
            tab: 1,
            project: Some("/p/sawit.dev".into()),
            kind,
            activity: Activity::Busy,
            profile: profile.map(|(a, b)| (a.into(), b.into())),
        };
        let claude_work = t(
            AgentKind::ClaudeCode,
            Some(("CLAUDE_CONFIG_DIR", "/h/.claude-work")),
        );
        assert_eq!(
            account_of(&claude_work, &all, home).as_deref(),
            Some("work")
        );
        assert_eq!(
            account_of(&t(AgentKind::ClaudeCode, None), &all, home).as_deref(),
            Some("default")
        );
        assert_eq!(
            account_of(
                &t(AgentKind::Codex, Some(("CODEX_HOME", "/h/.codex-side"))),
                &all,
                home
            )
            .as_deref(),
            Some("side")
        );
        let mcp = vec![
            serde_json::json!({"name": "Claude Code", "surface": "canvas", "action": "added a sticky", "seconds_ago": 40}),
        ];
        let seen = collect(&[claude_work], &mcp, &all, home);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].place, "Terminal 2 · sawit.dev");
        assert_eq!(seen[1].place, "Canvas");
        assert_eq!(seen[1].activity, Activity::Idle);
    }
}
