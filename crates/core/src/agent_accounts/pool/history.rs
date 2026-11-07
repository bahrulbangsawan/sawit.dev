//! The pool over time: readings, the limit windows they show, a pace forecast, when work
//! happens (weekday by hour), and how well each plan fits.

use serde::{Deserialize, Serialize};

use super::{Limit, LimitKind};

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
