//! The account pool across agents: every account's limits (5-hour, weekly, credits), a status
//! for each, which account has the most room now and why, the limit windows over time, a pace
//! forecast, when work happens (weekday by hour), how well each plan fits, and where to move
//! work. Pure functions over readings, so the views, `sawit-mcp` and tests share one answer.
//!
//! Readings come from what each agent itself reports on this Mac: Claude Code's status line
//! (`usage.rs`) and Codex's session logs (`codex.rs`). Nothing is estimated that wasn't read;
//! a limit an agent doesn't report is "not reported".

mod history;

use serde::{Deserialize, Serialize};

pub use history::{
    FitVerdict, Forecast, Heatmap, PlanFit, Reading, WindowSpan, forecast, heatmap, plan_fit,
    resets_between, windows,
};

/// An agent (or service) whose limits Sawit reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    ClaudeCode,
    Codex,
}

impl Provider {
    pub const ALL: &[Provider] = &[Provider::ClaudeCode, Provider::Codex];

    pub fn label(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "Claude Code",
            Provider::Codex => "Codex",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Provider::ClaudeCode => "claude-code",
            Provider::Codex => "codex",
        }
    }

    pub fn from_id(id: &str) -> Option<Provider> {
        Provider::ALL.iter().copied().find(|p| p.id() == id)
    }
}

/// Which limit a reading is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LimitKind {
    /// A rolling window of about five hours.
    FiveHour,
    /// A weekly window.
    Weekly,
}

impl LimitKind {
    pub const ALL: &[LimitKind] = &[LimitKind::FiveHour, LimitKind::Weekly];

    pub fn label(self) -> &'static str {
        match self {
            LimitKind::FiveHour => "5-hour",
            LimitKind::Weekly => "Weekly",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            LimitKind::FiveHour => "5h",
            LimitKind::Weekly => "7d",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            LimitKind::FiveHour => "five-hour",
            LimitKind::Weekly => "weekly",
        }
    }

    pub fn from_id(id: &str) -> Option<LimitKind> {
        LimitKind::ALL.iter().copied().find(|k| k.id() == id)
    }

    /// The window's length in seconds.
    pub fn window_secs(self) -> f64 {
        match self {
            LimitKind::FiveHour => 5.0 * 3600.0,
            LimitKind::Weekly => 7.0 * 86400.0,
        }
    }
}

/// One limit as last read: the share used and when its window resets (Unix seconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Limit {
    pub kind: LimitKind,
    pub used_pct: f64,
    pub resets_at: f64,
}

impl Limit {
    /// The share left, 0–100.
    pub fn left_pct(&self) -> f64 {
        (100.0 - self.used_pct).clamp(0.0, 100.0)
    }

    /// Whether its window is still running at `now` (else the reading is from before a reset).
    pub fn current(&self, now: f64) -> bool {
        self.resets_at > now
    }

    /// When its window began.
    pub fn starts_at(&self) -> f64 {
        self.resets_at - self.kind.window_secs()
    }
}

/// One account of the pool as the views see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PoolAccount {
    pub id: String,
    pub provider: Provider,
    pub name: String,
    pub color: String,
    pub plan: Option<String>,
    /// Signed in (`Some(true)`), signed out, or not known.
    pub signed_in: Option<bool>,
    /// The limits as last read, and when (Unix seconds).
    pub limits: Vec<Limit>,
    pub observed_at: Option<f64>,
    /// The last read failed, with why.
    pub refresh_error: Option<String>,
    /// The account new sessions of its agent start with.
    pub active: bool,
}

impl PoolAccount {
    pub fn limit(&self, kind: LimitKind) -> Option<&Limit> {
        self.limits.iter().find(|l| l.kind == kind)
    }

    /// The limits whose windows still run at `now`.
    pub fn current(&self, now: f64) -> impl Iterator<Item = &Limit> {
        self.limits.iter().filter(move |l| l.current(now))
    }

    /// The limit with the least room now: the one that holds the account back.
    pub fn binding(&self, now: f64) -> Option<&Limit> {
        self.current(now)
            .min_by(|a, b| a.left_pct().total_cmp(&b.left_pct()))
    }

    /// The room left now (0–100): the tightest current limit's, 100 when every window reset
    /// since the last reading, `None` when nothing was ever read.
    pub fn room(&self, now: f64) -> Option<f64> {
        if self.limits.is_empty() {
            return None;
        }
        Some(self.binding(now).map(Limit::left_pct).unwrap_or(100.0))
    }
}

/// A reading older than this is out of date.
pub const STALE_SECS: f64 = 15.0 * 60.0;

/// An account's status, from the most to the least room. Every status has words and an icon in
/// the views, never a colour alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    LimitReached,
    AlmostOut,
    RunningLow,
    SignInExpired,
    RefreshFailed,
    OutOfDate,
    NotReported,
    PlentyLeft,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::PlentyLeft => "Plenty left",
            Status::RunningLow => "Running low",
            Status::AlmostOut => "Almost out",
            Status::LimitReached => "Limit reached",
            Status::NotReported => "Not reported",
            Status::OutOfDate => "Out of date",
            Status::RefreshFailed => "Refresh failed",
            Status::SignInExpired => "Sign-in expired",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Status::PlentyLeft => "plenty-left",
            Status::RunningLow => "running-low",
            Status::AlmostOut => "almost-out",
            Status::LimitReached => "limit-reached",
            Status::NotReported => "not-reported",
            Status::OutOfDate => "out-of-date",
            Status::RefreshFailed => "refresh-failed",
            Status::SignInExpired => "sign-in-expired",
        }
    }

    /// How urgent, for "By urgency": a smaller number first.
    pub fn urgency(self) -> u8 {
        self as u8
    }
}

/// The status of `a` at `now`: sign-in and read failures first, then the room left (under 25%
/// running low, under 10% almost out, none left reached), then whether the reading is current.
pub fn status(a: &PoolAccount, now: f64) -> Status {
    if a.signed_in == Some(false) {
        return Status::SignInExpired;
    }
    if a.refresh_error.is_some() {
        return Status::RefreshFailed;
    }
    let Some(room) = a.room(now) else {
        return Status::NotReported;
    };
    let level = match room {
        r if r <= 0.0 => Status::LimitReached,
        r if r < 10.0 => Status::AlmostOut,
        r if r < 25.0 => Status::RunningLow,
        _ => Status::PlentyLeft,
    };
    // A limit reached stays reached until its reset, however old the reading.
    if level != Status::LimitReached && a.observed_at.is_some_and(|t| now - t > STALE_SECS) {
        return Status::OutOfDate;
    }
    level
}

/// The pick for "use this account now".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Recommendation {
    pub account: String,
    pub room: f64,
    /// The limit holding it back, if any is current.
    pub holding: Option<LimitKind>,
    pub reason: String,
    /// The runner-up, with its room.
    pub next: Option<(String, f64)>,
    /// Accounts left out, with why (signed out, not reported).
    pub skipped: Vec<(String, &'static str)>,
}

/// The account with the most room right now among `accounts` (of any provider, or of
/// `provider` only), and why. Signed-out and never-read accounts are skipped with the reason.
/// Room is a share of each plan, so a bigger plan with less room may still do more work.
pub fn recommend(
    accounts: &[PoolAccount],
    provider: Option<Provider>,
    now: f64,
) -> Option<Recommendation> {
    let mut skipped = Vec::new();
    let mut ranked: Vec<(&PoolAccount, f64)> = Vec::new();
    for a in accounts
        .iter()
        .filter(|a| provider.is_none_or(|p| a.provider == p))
    {
        match (status(a, now), a.room(now)) {
            (Status::SignInExpired, _) => skipped.push((a.name.clone(), "signed out")),
            (_, None) => skipped.push((a.name.clone(), "no limits reported yet")),
            (_, Some(room)) => ranked.push((a, room)),
        }
    }
    // Most room first; on a tie, the window that resets sooner frees up first, then the name.
    ranked.sort_by(|(a, ra), (b, rb)| {
        rb.total_cmp(ra)
            .then_with(|| {
                let reset =
                    |x: &PoolAccount| x.binding(now).map(|l| l.resets_at).unwrap_or(f64::MAX);
                reset(a).total_cmp(&reset(b))
            })
            .then_with(|| a.name.cmp(&b.name))
    });
    let (best, room) = *ranked.first()?;
    let holding = best.binding(now).map(|l| l.kind);
    let reason = match best.binding(now) {
        Some(l) if room >= 100.0 => format!(
            "Every limit has room; the {} window is untouched",
            l.kind.label().to_lowercase()
        ),
        Some(l) => format!(
            "{:.0}% of its {} limit left, the most of any account",
            room,
            l.kind.label().to_lowercase()
        ),
        None => "Every window reset since its last reading".into(),
    };
    Some(Recommendation {
        account: best.id.clone(),
        room,
        holding,
        reason,
        next: ranked.get(1).map(|(a, r)| (a.id.clone(), *r)),
        skipped,
    })
}

/// "Move work from `from` to `to`": an account that is running out (or forecast to) and one of
/// the same provider with room.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MoveWork {
    pub from: String,
    pub to: String,
    pub why: String,
}

/// Suggestions to move work: for each account at or near its limit, or forecast to run out
/// before a reset, the account of the same provider with the most room (at least 40% left).
pub fn move_work(accounts: &[PoolAccount], now: f64) -> Vec<MoveWork> {
    let mut out = Vec::new();
    for a in accounts {
        let tight = matches!(
            status(a, now),
            Status::LimitReached | Status::AlmostOut | Status::RunningLow
        );
        let runs_out = a
            .current(now)
            .filter_map(|l| forecast(l, now))
            .find_map(|f| f.runs_out_at.map(|t| (f.kind, t)));
        if !tight && runs_out.is_none() {
            continue;
        }
        let same: Vec<PoolAccount> = accounts
            .iter()
            .filter(|b| b.provider == a.provider && b.id != a.id)
            .cloned()
            .collect();
        let Some(pick) = recommend(&same, None, now).filter(|r| r.room >= 40.0) else {
            continue;
        };
        let to = accounts
            .iter()
            .find(|b| b.id == pick.account)
            .map(|b| b.name.clone())
            .unwrap_or_default();
        let why = match (tight, runs_out) {
            (true, _) => format!(
                "{} has {:.0}% left; {} has {:.0}%",
                a.name,
                a.room(now).unwrap_or(0.0),
                to,
                pick.room
            ),
            (false, Some((kind, _))) => format!(
                "At its pace {} runs out of its {} limit before the reset; {} has {:.0}% left",
                a.name,
                kind.label().to_lowercase(),
                to,
                pick.room
            ),
            _ => continue,
        };
        out.push(MoveWork {
            from: a.id.clone(),
            to: pick.account,
            why,
        });
    }
    out
}

/// The overview as `sawit-mcp` and `app_state` give it: every account with its status, room,
/// limits and resets, and the pick.
pub fn overview_json(accounts: &[PoolAccount], now: f64) -> serde_json::Value {
    let rows: Vec<serde_json::Value> = accounts
        .iter()
        .map(|a| {
            let st = status(a, now);
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "provider": a.provider.id(),
                "plan": a.plan,
                "active": a.active,
                "status": st.id(),
                "status_label": st.label(),
                "room_left_pct": a.room(now),
                "holding_limit": a.binding(now).map(|l| l.kind.id()),
                "limits": a.limits.iter().map(|l| serde_json::json!({
                    "limit": l.kind.id(),
                    "used_pct": l.used_pct,
                    "left_pct": l.left_pct(),
                    "resets_at": l.resets_at,
                    "current": l.current(now),
                })).collect::<Vec<_>>(),
                "observed_at": a.observed_at,
            })
        })
        .collect();
    serde_json::json!({
        "accounts": rows,
        "recommended": recommend(accounts, None, now),
    })
}

#[cfg(test)]
mod tests;
