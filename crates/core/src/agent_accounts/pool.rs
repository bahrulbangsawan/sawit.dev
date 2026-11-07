//! The account pool across agents: every account's limits (5-hour, weekly, credits), a status
//! for each, which account has the most room now and why, the limit windows over time, a pace
//! forecast, when work happens (weekday by hour), how well each plan fits, and where to move
//! work. Pure functions over readings, so the views, `sawit-mcp` and tests share one answer.
//!
//! Readings come from what each agent itself reports on this Mac: Claude Code's status line
//! (`usage.rs`) and Codex's session logs (`codex.rs`). Nothing is estimated that wasn't read;
//! a limit an agent doesn't report is "not reported".

use serde::{Deserialize, Serialize};

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

/// One reading of one limit at one time (a row of the readings store).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub t: f64,
    pub kind: LimitKind,
    pub used_pct: f64,
    pub resets_at: f64,
}

/// A limit window over time: when it ran, how full it got, and whether it hit the limit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct WindowSpan {
    pub kind: LimitKind,
    pub start: f64,
    pub end: f64,
    pub peak_pct: f64,
    pub hit_limit: bool,
    /// When the limit was first hit, if it was.
    pub hit_at: Option<f64>,
}

/// Readings within this many seconds of the same reset time are one window (the reset moves a
/// little between reports).
const SAME_RESET: f64 = 120.0;

/// The windows of `kind` the readings show, oldest first: one per reset time.
pub fn windows(readings: &[Reading], kind: LimitKind) -> Vec<WindowSpan> {
    let mut rows: Vec<&Reading> = readings.iter().filter(|r| r.kind == kind).collect();
    rows.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut out: Vec<WindowSpan> = Vec::new();
    for r in rows {
        match out.last_mut() {
            Some(w) if (w.end - r.resets_at).abs() <= SAME_RESET => {
                if r.used_pct > w.peak_pct {
                    w.peak_pct = r.used_pct;
                }
                if r.used_pct >= 100.0 && w.hit_at.is_none() {
                    w.hit_at = Some(r.t);
                }
                w.hit_limit |= r.used_pct >= 100.0;
            }
            _ => out.push(WindowSpan {
                kind,
                start: r.resets_at - kind.window_secs(),
                end: r.resets_at,
                peak_pct: r.used_pct,
                hit_limit: r.used_pct >= 100.0,
                hit_at: (r.used_pct >= 100.0).then_some(r.t),
            }),
        }
    }
    out
}

/// The windows that reset in `[from, to)`.
pub fn resets_between(spans: &[WindowSpan], from: f64, to: f64) -> Vec<WindowSpan> {
    spans
        .iter()
        .copied()
        .filter(|w| w.end >= from && w.end < to)
        .collect()
}

/// Where a window is heading at its pace so far.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Forecast {
    pub kind: LimitKind,
    /// The share of the window gone by, 0–100.
    pub elapsed_pct: f64,
    pub used_pct: f64,
    /// Used against an even pace: above 1 uses faster than the window allows.
    pub pace: f64,
    /// The share used at the reset if the pace holds.
    pub projected_pct: f64,
    /// When it runs out at this pace, if before the reset.
    pub runs_out_at: Option<f64>,
}

/// The forecast of `limit` at `now`, if its window runs and has gone on long enough to judge
/// (5% of it).
pub fn forecast(limit: &Limit, now: f64) -> Option<Forecast> {
    if !limit.current(now) {
        return None;
    }
    let len = limit.kind.window_secs();
    let elapsed = (now - limit.starts_at()).clamp(0.0, len);
    if elapsed < len * 0.05 {
        return None;
    }
    let elapsed_pct = elapsed / len * 100.0;
    let rate = limit.used_pct / elapsed; // percent per second
    let projected_pct = limit.used_pct + rate * (limit.resets_at - now);
    let runs_out_at = (rate > 0.0 && limit.used_pct < 100.0)
        .then(|| now + (100.0 - limit.used_pct) / rate)
        .filter(|t| *t < limit.resets_at);
    Some(Forecast {
        kind: limit.kind,
        elapsed_pct,
        used_pct: limit.used_pct,
        pace: limit.used_pct / elapsed_pct,
        projected_pct,
        runs_out_at,
    })
}

/// When work happens: the rise in use per weekday (0 is Monday) and hour, in local time, and
/// how often the limit was hit in each slot.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Heatmap {
    /// `[weekday][hour]`: percentage points used.
    pub used: [[f64; 24]; 7],
    /// `[weekday][hour]`: readings at the limit.
    pub locked: [[u32; 24]; 7],
}

impl Heatmap {
    /// The busiest slot: (weekday, hour, points).
    pub fn busiest(&self) -> Option<(usize, usize, f64)> {
        let mut best = None;
        for (d, row) in self.used.iter().enumerate() {
            for (h, v) in row.iter().enumerate() {
                if *v > 0.0 && best.is_none_or(|(_, _, b)| *v > b) {
                    best = Some((d, h, *v));
                }
            }
        }
        best
    }

    pub fn max(&self) -> f64 {
        self.used.iter().flatten().copied().fold(0.0, f64::max)
    }
}

/// The heatmap of the readings of `kind`, with `offset` giving each time's local offset from
/// UTC in seconds. Use is the rise between consecutive readings of one window.
pub fn heatmap(readings: &[Reading], kind: LimitKind, offset: &dyn Fn(f64) -> i64) -> Heatmap {
    let mut map = Heatmap {
        used: [[0.0; 24]; 7],
        locked: [[0; 24]; 7],
    };
    let mut rows: Vec<&Reading> = readings.iter().filter(|r| r.kind == kind).collect();
    rows.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut prev: Option<&Reading> = None;
    for r in rows {
        let l = crate::date::in_zone(r.t as i64, offset(r.t));
        let (d, h) = (l.weekday as usize, l.hour as usize);
        if let Some(p) = prev
            && (p.resets_at - r.resets_at).abs() <= SAME_RESET
            && r.used_pct > p.used_pct
        {
            map.used[d][h] += r.used_pct - p.used_pct;
        } else if prev.is_none_or(|p| (p.resets_at - r.resets_at).abs() > SAME_RESET) {
            // A new window: what it shows at its first reading was used since it opened.
            map.used[d][h] += r.used_pct;
        }
        if r.used_pct >= 100.0 {
            map.locked[d][h] += 1;
        }
        prev = Some(r);
    }
    map
}

/// How well a plan fits, from its windows over a period.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlanFit {
    pub windows: usize,
    pub hits: usize,
    pub avg_peak_pct: f64,
    pub verdict: FitVerdict,
    /// Price × the share of the limit not used, per month, when the price is known.
    pub unused_per_month: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FitVerdict {
    /// It hits its limit often: a bigger plan or a second account would help.
    TooSmall,
    Fits,
    /// It rarely gets past a third: a smaller plan would do.
    CouldDownsize,
    /// Barely used: it could go.
    CouldCancel,
    /// Too few windows to say.
    TooEarly,
}

impl FitVerdict {
    pub fn label(self) -> &'static str {
        match self {
            FitVerdict::TooSmall => "Often at its limit",
            FitVerdict::Fits => "Fits",
            FitVerdict::CouldDownsize => "Could downsize",
            FitVerdict::CouldCancel => "Could cancel",
            FitVerdict::TooEarly => "Too early to say",
        }
    }
}

/// The plan fit of an account's weekly windows (the limit a plan is sized by), with its monthly
/// price if known.
pub fn plan_fit(weekly: &[WindowSpan], monthly_price: Option<f64>) -> PlanFit {
    let n = weekly.len();
    let hits = weekly.iter().filter(|w| w.hit_limit).count();
    let avg = if n == 0 {
        0.0
    } else {
        weekly.iter().map(|w| w.peak_pct.min(100.0)).sum::<f64>() / n as f64
    };
    let verdict = match (n, avg) {
        (0..2, _) => FitVerdict::TooEarly,
        _ if hits * 3 >= n => FitVerdict::TooSmall,
        (_, a) if a < 10.0 => FitVerdict::CouldCancel,
        (_, a) if a < 35.0 => FitVerdict::CouldDownsize,
        _ => FitVerdict::Fits,
    };
    PlanFit {
        windows: n,
        hits,
        avg_peak_pct: avg,
        verdict,
        unused_per_month: monthly_price
            .filter(|_| n > 0)
            .map(|p| p * (1.0 - avg / 100.0)),
    }
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
mod tests {
    use super::*;

    const NOW: f64 = 1_000_000.0;

    fn acct(name: &str, provider: Provider, limits: &[(LimitKind, f64, f64)]) -> PoolAccount {
        PoolAccount {
            id: name.into(),
            provider,
            name: name.into(),
            color: "#D97757".into(),
            plan: None,
            signed_in: Some(true),
            limits: limits
                .iter()
                .map(|&(kind, used_pct, resets_in)| Limit {
                    kind,
                    used_pct,
                    resets_at: NOW + resets_in,
                })
                .collect(),
            observed_at: Some(NOW - 60.0),
            refresh_error: None,
            active: false,
        }
    }

    #[test]
    fn statuses_follow_the_tightest_limit() {
        use LimitKind::*;
        let s = |l: &[(LimitKind, f64, f64)]| status(&acct("a", Provider::ClaudeCode, l), NOW);
        assert_eq!(
            s(&[(FiveHour, 20.0, 3600.0), (Weekly, 40.0, 86400.0)]),
            Status::PlentyLeft
        );
        assert_eq!(s(&[(FiveHour, 80.0, 3600.0)]), Status::RunningLow);
        assert_eq!(s(&[(FiveHour, 95.0, 3600.0)]), Status::AlmostOut);
        assert_eq!(s(&[(Weekly, 100.0, 3600.0)]), Status::LimitReached);
        // A window that reset since the reading has room again.
        assert_eq!(s(&[(FiveHour, 100.0, -10.0)]), Status::PlentyLeft);
        assert_eq!(s(&[]), Status::NotReported);
        let mut old = acct("a", Provider::ClaudeCode, &[(FiveHour, 20.0, 3600.0)]);
        old.observed_at = Some(NOW - 2.0 * STALE_SECS);
        assert_eq!(status(&old, NOW), Status::OutOfDate);
        old.limits[0].used_pct = 100.0;
        assert_eq!(
            status(&old, NOW),
            Status::LimitReached,
            "reached stays reached"
        );
        old.signed_in = Some(false);
        assert_eq!(status(&old, NOW), Status::SignInExpired);
        let mut failed = acct("a", Provider::Codex, &[]);
        failed.refresh_error = Some("unreadable".into());
        assert_eq!(status(&failed, NOW), Status::RefreshFailed);
    }

    #[test]
    fn the_pick_has_the_most_room_and_says_why() {
        use LimitKind::*;
        let mut out = acct("out", Provider::ClaudeCode, &[(FiveHour, 10.0, 3600.0)]);
        out.signed_in = Some(false);
        let all = vec![
            acct(
                "work",
                Provider::ClaudeCode,
                &[(FiveHour, 30.0, 3600.0), (Weekly, 70.0, 86400.0)],
            ),
            acct(
                "side",
                Provider::ClaudeCode,
                &[(FiveHour, 50.0, 3600.0), (Weekly, 45.0, 86400.0)],
            ),
            acct("codex", Provider::Codex, &[]),
            out,
        ];
        let r = recommend(&all, None, NOW).unwrap();
        assert_eq!(r.account, "side");
        assert_eq!(r.room, 50.0);
        assert_eq!(r.holding, Some(FiveHour));
        assert!(r.reason.contains("50%"), "{}", r.reason);
        assert_eq!(r.next, Some(("work".into(), 30.0)));
        assert_eq!(r.skipped.len(), 2);
        assert!(recommend(&all, Some(Provider::Codex), NOW).is_none());
    }

    #[test]
    fn readings_make_windows_with_peaks_and_hits() {
        let r = |t: f64, used: f64, resets: f64| Reading {
            t,
            kind: LimitKind::FiveHour,
            used_pct: used,
            resets_at: resets,
        };
        let readings = [
            r(100.0, 10.0, 18_000.0),
            r(200.0, 60.0, 18_030.0),
            r(300.0, 100.0, 18_000.0),
            r(20_000.0, 5.0, 38_000.0),
        ];
        let spans = windows(&readings, LimitKind::FiveHour);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].peak_pct, 100.0);
        assert!(spans[0].hit_limit);
        assert_eq!(spans[0].hit_at, Some(300.0));
        assert_eq!(spans[0].start, 0.0);
        assert!(!spans[1].hit_limit);
        assert_eq!(resets_between(&spans, 0.0, 20_000.0).len(), 1);
    }

    #[test]
    fn a_fast_pace_runs_out_before_the_reset() {
        // Half the 5-hour window gone, 80% used: 1.6 times an even pace.
        let l = Limit {
            kind: LimitKind::FiveHour,
            used_pct: 80.0,
            resets_at: NOW + 9000.0,
        };
        let f = forecast(&l, NOW).unwrap();
        assert!((f.elapsed_pct - 50.0).abs() < 1e-9);
        assert!((f.pace - 1.6).abs() < 1e-9);
        assert!(f.projected_pct > 100.0);
        let out = f.runs_out_at.unwrap();
        assert!(out > NOW && out < l.resets_at);
        let slow = Limit {
            used_pct: 20.0,
            ..l
        };
        assert_eq!(forecast(&slow, NOW).unwrap().runs_out_at, None);
        let fresh = Limit {
            resets_at: NOW + 17_990.0,
            ..l
        };
        assert!(forecast(&fresh, NOW).is_none(), "too early to judge");
    }

    #[test]
    fn the_heatmap_counts_rises_by_weekday_and_hour() {
        // 1970-01-05 was a Monday.
        let monday_9 = 4.0 * 86400.0 + 9.0 * 3600.0;
        let r = |t: f64, used: f64| Reading {
            t,
            kind: LimitKind::FiveHour,
            used_pct: used,
            resets_at: monday_9 + 18_000.0,
        };
        let map = heatmap(
            &[
                r(monday_9, 10.0),
                r(monday_9 + 600.0, 30.0),
                r(monday_9 + 3700.0, 100.0),
            ],
            LimitKind::FiveHour,
            &|_| 0,
        );
        assert_eq!(map.used[0][9], 30.0);
        assert_eq!(map.used[0][10], 70.0);
        assert_eq!(map.locked[0][10], 1);
        assert_eq!(map.busiest(), Some((0, 10, 70.0)));
    }

    #[test]
    fn plan_fit_reads_the_weekly_windows() {
        let w = |peak: f64| WindowSpan {
            kind: LimitKind::Weekly,
            start: 0.0,
            end: 1.0,
            peak_pct: peak,
            hit_limit: peak >= 100.0,
            hit_at: None,
        };
        assert_eq!(plan_fit(&[w(50.0)], None).verdict, FitVerdict::TooEarly);
        assert_eq!(
            plan_fit(&[w(100.0), w(100.0), w(40.0)], None).verdict,
            FitVerdict::TooSmall
        );
        assert_eq!(
            plan_fit(&[w(5.0), w(8.0)], None).verdict,
            FitVerdict::CouldCancel
        );
        let fit = plan_fit(&[w(20.0), w(30.0)], Some(100.0));
        assert_eq!(fit.verdict, FitVerdict::CouldDownsize);
        assert_eq!(fit.unused_per_month, Some(75.0));
        assert_eq!(
            plan_fit(&[w(60.0), w(80.0)], None).verdict,
            FitVerdict::Fits
        );
    }

    #[test]
    fn work_moves_to_an_account_of_the_same_agent_with_room() {
        use LimitKind::*;
        let all = vec![
            acct("work", Provider::ClaudeCode, &[(FiveHour, 95.0, 3600.0)]),
            acct("side", Provider::ClaudeCode, &[(FiveHour, 10.0, 3600.0)]),
            acct("codex", Provider::Codex, &[(FiveHour, 0.0, 3600.0)]),
        ];
        let moves = move_work(&all, NOW);
        assert_eq!(moves.len(), 1);
        assert_eq!(
            (moves[0].from.as_str(), moves[0].to.as_str()),
            ("work", "side")
        );
        let json = overview_json(&all, NOW);
        assert_eq!(json["recommended"]["account"], "codex");
        assert_eq!(json["accounts"][0]["status"], "almost-out");
    }
}
