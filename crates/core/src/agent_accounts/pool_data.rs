//! The pool read from disk in one go, for the Accounts page's pool tabs and `sawit-mcp`: Claude
//! Code's accounts with what their status lines reported, Codex's accounts with what their
//! session logs recorded, each reading stored in the history, status changes noted as notices,
//! then the history, the notices and the wallet.

use std::collections::BTreeMap;
use std::path::Path;

use super::pool::{self, Limit, LimitKind, PoolAccount, Provider, Reading};
use super::readings::{self, Notice};
use super::wallet::{self, Wallet};
use super::{Accounts, AgentKind, codex, usage};
use crate::Result;

/// Everything the pool's views show.
#[derive(Debug, Clone, Default)]
pub struct PoolData {
    pub accounts: Vec<PoolAccount>,
    /// Each account's readings over the period asked for, oldest first.
    pub readings: BTreeMap<String, Vec<Reading>>,
    pub notices: Vec<Notice>,
    pub wallet: Wallet,
}

/// What the app knows of an account that the files don't say: its sign-in and plan.
pub struct Known<'a> {
    pub signed_in: &'a dyn Fn(&str) -> Option<bool>,
    pub plan: &'a dyn Fn(&str) -> Option<String>,
}

impl Known<'_> {
    /// Nothing known (`sawit-mcp`, which reads files only).
    pub fn nothing() -> Known<'static> {
        Known {
            signed_in: &|_| None,
            plan: &|_| None,
        }
    }
}

/// The id of the Codex account in folder `dir`: `codex:<name>`.
pub fn codex_id(dir: &Path) -> String {
    format!("codex:{}", codex::name_of(dir))
}

/// Claude Code's accounts as pool accounts, from their latest usage snapshots.
pub fn claude_accounts(root: &Path, all: &Accounts, known: &Known) -> Vec<PoolAccount> {
    let active = all
        .active(AgentKind::ClaudeCode, root)
        .map(|a| a.id.clone());
    all.of(AgentKind::ClaudeCode)
        .into_iter()
        .map(|a| {
            let snap = usage::load_snapshot(root, &a.id);
            let limits = snap
                .as_ref()
                .map(|s| {
                    [
                        (LimitKind::FiveHour, s.five_hour),
                        (LimitKind::Weekly, s.seven_day),
                    ]
                    .into_iter()
                    .filter_map(|(kind, w)| {
                        w.map(|w| Limit {
                            kind,
                            used_pct: w.used_percentage,
                            resets_at: w.resets_at,
                        })
                    })
                    .collect()
                })
                .unwrap_or_default();
            PoolAccount {
                id: a.id.clone(),
                provider: Provider::ClaudeCode,
                name: a.name.clone(),
                color: a.color.clone(),
                plan: (known.plan)(&a.id),
                signed_in: (known.signed_in)(&a.id),
                limits,
                observed_at: snap.and_then(|s| s.observed_at),
                refresh_error: None,
                active: active.as_deref() == Some(a.id.as_str()),
            }
        })
        .collect()
}

/// Codex's accounts (its `CODEX_HOME` folders in `home`), from their newest session logs.
pub fn codex_accounts(home: &Path) -> Vec<PoolAccount> {
    let env_home = std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from);
    codex::discover(home)
        .into_iter()
        .enumerate()
        .map(|(ix, dir)| {
            let read = codex::read(&dir);
            PoolAccount {
                id: codex_id(&dir),
                provider: Provider::Codex,
                name: codex::name_of(&dir),
                color: ["#B48EDC", "#E5658A", "#F2B84B"][ix % 3].into(),
                plan: read.as_ref().and_then(|r| r.plan.clone()),
                signed_in: dir.join("auth.json").exists().then_some(true),
                limits: read.as_ref().map(|r| r.limits.clone()).unwrap_or_default(),
                observed_at: read.map(|r| r.observed_at),
                refresh_error: None,
                active: match &env_home {
                    Some(h) => *h == dir,
                    None => ix == 0,
                },
            }
        })
        .collect()
}

/// The pool now: every account, each new reading stored, status changes noted, then the last
/// `days` of history, the newest notices and the wallet. `record` false reads without writing
/// (a second reader, such as `sawit-mcp` while the app runs, records nothing twice).
pub fn collect(
    root: &Path,
    home: &Path,
    known: &Known,
    now: f64,
    days: f64,
    record: bool,
) -> Result<PoolData> {
    let all = Accounts::load(root)?;
    let mut accounts = claude_accounts(root, &all, known);
    accounts.extend(codex_accounts(home));
    let db = readings::open(root)?;
    if record {
        for a in &accounts {
            // The status line's own history first, once: it reaches back 8 days.
            if a.provider == Provider::ClaudeCode && readings::readings(&db, &a.id, 0.0)?.is_empty()
            {
                readings::import_history(&db, &a.id, &usage::load_history(root, &a.id))?;
            }
            if let Some(t) = a.observed_at {
                readings::record(&db, &a.id, a.provider, &a.limits, t)?;
            }
            readings::note_status(&db, &a.id, &a.name, pool::status(a, now), now)?;
        }
        readings::prune(&db, now)?;
    }
    let mut history = BTreeMap::new();
    for a in &accounts {
        history.insert(
            a.id.clone(),
            readings::readings(&db, &a.id, now - days * 86400.0)?,
        );
    }
    Ok(PoolData {
        accounts,
        readings: history,
        notices: readings::notices(&db, 50)?,
        wallet: wallet::load(root),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pool_reads_claude_and_codex_and_keeps_history() {
        let tmp = tempfile::tempdir().unwrap();
        let (root, home) = (tmp.path().join("sawit"), tmp.path().join("home"));
        std::fs::create_dir_all(home.join(".claude-work")).unwrap();
        let mut all = Accounts::default();
        let mut a = super::super::tests::account("work", &home.join(".claude-work"), false);
        a.id = "w1".into();
        all.add(a).unwrap();
        all.save(&root).unwrap();
        let now = 1_000_000.0;
        let snap = usage::Snapshot {
            profile_id: "w1".into(),
            observed_at: Some(now - 30.0),
            five_hour: Some(usage::Window {
                used_percentage: 95.0,
                resets_at: now + 3600.0,
            }),
            ..Default::default()
        };
        std::fs::create_dir_all(usage::usage_dir(&root)).unwrap();
        std::fs::write(
            usage::snapshot_path(&root, "w1"),
            serde_json::to_string(&snap).unwrap(),
        )
        .unwrap();
        let sessions = home.join(".codex/sessions/2025/11/07");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("rollout-a.jsonl"),
            r#"{"timestamp":"1970-01-12T13:46:00Z","payload":{"rate_limits":{"primary":{"used_percent":20,"window_minutes":300,"resets_in_seconds":3600}}}}"#,
        )
        .unwrap();
        let data = collect(&root, &home, &Known::nothing(), now, 30.0, true).unwrap();
        assert_eq!(data.accounts.len(), 2);
        assert_eq!(
            pool::status(&data.accounts[0], now),
            pool::Status::AlmostOut
        );
        assert_eq!(data.accounts[1].id, "codex:default");
        assert_eq!(data.readings["w1"].len(), 1);
        let pick = pool::recommend(&data.accounts, None, now).unwrap();
        assert_eq!(pick.account, "codex:default");
        // Reading again stores nothing new, and a read-only pass writes nothing.
        let again = collect(&root, &home, &Known::nothing(), now + 10.0, 30.0, false).unwrap();
        assert_eq!(again.readings["w1"].len(), 1);
    }
}
