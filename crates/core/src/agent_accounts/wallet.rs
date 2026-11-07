//! What the pool costs: each account's price (per month, per year, included with another
//! subscription, or not set), credit top-ups with their expiry, upcoming renewals, paid top-ups
//! per month, accounts removed from the pool, and exchange rates to one currency. Kept in
//! `accounts/wallet.json` (0600); prices are what the user enters, with list prices offered
//! for known plans.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

/// How an account is paid for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case", tag = "billing")]
pub enum Billing {
    #[default]
    NotSet,
    Monthly {
        amount: f64,
    },
    Yearly {
        amount: f64,
    },
    /// Comes with another subscription (`with`), at no price of its own.
    Included {
        with: String,
    },
}

/// An account's price and renewal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Price {
    #[serde(flatten)]
    pub billing: Billing,
    /// The currency of the amount (`USD`); the wallet's own when empty.
    #[serde(default)]
    pub currency: String,
    /// The day of the month it renews (1–28), for monthly and yearly plans.
    #[serde(default)]
    pub renews_day: Option<u32>,
    /// The month a yearly plan renews in (1–12).
    #[serde(default)]
    pub renews_month: Option<u32>,
}

/// Credits bought for an account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopUp {
    pub id: String,
    pub account: String,
    pub amount: f64,
    #[serde(default)]
    pub currency: String,
    /// Credits it added, in the provider's unit.
    #[serde(default)]
    pub credits: Option<f64>,
    /// Unix seconds.
    pub at: i64,
    #[serde(default)]
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub note: String,
}

/// An account taken out of the pool, with what it cost, so the spending history stays whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Removed {
    pub name: String,
    pub provider: String,
    pub removed_at: i64,
    #[serde(default)]
    pub price: Option<Price>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wallet {
    /// The currency totals are shown in.
    pub currency: String,
    #[serde(default)]
    pub prices: BTreeMap<String, Price>,
    #[serde(default)]
    pub topups: Vec<TopUp>,
    #[serde(default)]
    pub removed: Vec<Removed>,
    /// Units of `currency` per unit of another currency (`EUR` → 1.08 with USD totals).
    #[serde(default)]
    pub rates: BTreeMap<String, f64>,
    /// Warn this many days before a top-up expires.
    #[serde(default = "default_alert_days")]
    pub expiry_alert_days: u32,
}

fn default_alert_days() -> u32 {
    7
}

impl Default for Wallet {
    fn default() -> Self {
        Wallet {
            currency: "USD".into(),
            prices: BTreeMap::new(),
            topups: Vec::new(),
            removed: Vec::new(),
            rates: BTreeMap::new(),
            expiry_alert_days: default_alert_days(),
        }
    }
}

pub fn path(root: &Path) -> PathBuf {
    super::store::dir(root).join("wallet.json")
}

/// The saved wallet, or an empty one in USD.
pub fn load(root: &Path) -> Wallet {
    std::fs::read_to_string(path(root))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save(root: &Path, w: &Wallet) -> Result<()> {
    super::store::private_dir(&super::store::dir(root))?;
    let text =
        serde_json::to_string_pretty(w).map_err(|e| crate::CoreError::Parse(e.to_string()))?;
    super::store::write_private(&path(root), text.as_bytes())
}

/// List prices per month of plans the agents report, in USD: an offer when a price isn't set.
/// Prices change; the user's own entry always wins.
pub fn list_price(plan: &str) -> Option<f64> {
    let p = plan.to_lowercase();
    let price = if p.contains("max") && p.contains("20") {
        200.0
    } else if p.contains("max") {
        100.0
    } else if (p.contains("pro") && (p.contains("claude") || p == "pro"))
        || p == "plus"
        || p.contains("chatgpt plus")
    {
        20.0
    } else if p.contains("chatgpt pro") {
        200.0
    } else if p == "team" {
        30.0
    } else {
        return None;
    };
    Some(price)
}

impl Wallet {
    /// `amount` in `currency` converted to the wallet's currency, when the rate is known.
    pub fn convert(&self, amount: f64, currency: &str) -> Option<f64> {
        if currency.is_empty() || currency == self.currency {
            return Some(amount);
        }
        self.rates.get(currency).map(|r| amount * r)
    }

    /// What an account costs per month in the wallet's currency: a yearly price over twelve,
    /// nothing when included, `None` when not set or not convertible.
    pub fn monthly(&self, account: &str) -> Option<f64> {
        let p = self.prices.get(account)?;
        let raw = match &p.billing {
            Billing::NotSet => return None,
            Billing::Monthly { amount } => *amount,
            Billing::Yearly { amount } => amount / 12.0,
            Billing::Included { .. } => 0.0,
        };
        self.convert(raw, &p.currency)
    }

    /// Every account's monthly cost summed (unset prices left out), and per year.
    pub fn totals(&self) -> (f64, f64) {
        let m: f64 = self.prices.keys().filter_map(|a| self.monthly(a)).sum();
        (m, m * 12.0)
    }

    /// Each account's next renewal, when it falls in the next `days` from `now` (local calendar dates through `offset`):
    /// (account, Unix time of the renewal day, amount in the wallet's currency).
    pub fn renewals(&self, now: i64, days: i64, offset: i64) -> Vec<(String, i64, f64)> {
        let today = crate::date::in_zone(now, offset);
        let mut out = Vec::new();
        for (account, p) in &self.prices {
            let (amount, yearly) = match &p.billing {
                Billing::Monthly { amount } => (*amount, false),
                Billing::Yearly { amount } => (*amount, true),
                _ => continue,
            };
            let Some(day) = p.renews_day else { continue };
            let day = day.clamp(1, 28);
            // The next renewal on or after today, month by month.
            for ahead in 0..=13 {
                let m0 = today.month as i64 - 1 + ahead;
                let (y, m) = (
                    today.year + m0.div_euclid(12),
                    (m0.rem_euclid(12) + 1) as u32,
                );
                if yearly && Some(m) != p.renews_month {
                    continue;
                }
                let day_number = days_from_civil(y, m, day);
                if day_number < today.day_number {
                    continue;
                }
                let date = day_number * 86400 - offset;
                if date - now <= days * 86400
                    && let Some(v) = self.convert(amount, &p.currency)
                {
                    out.push((account.clone(), date, v));
                }
                break;
            }
        }
        out.sort_by_key(|r| r.1);
        out
    }

    /// Paid top-ups per calendar month for the last `months` (this one last): (year, month,
    /// total in the wallet's currency).
    pub fn topups_by_month(&self, now: i64, months: u32, offset: i64) -> Vec<(i64, u32, f64)> {
        let today = crate::date::in_zone(now, offset);
        (0..months as i64)
            .rev()
            .map(|back| {
                let m0 = today.month as i64 - 1 - back;
                let (y, m) = (
                    today.year + m0.div_euclid(12),
                    (m0.rem_euclid(12) + 1) as u32,
                );
                let total = self
                    .topups
                    .iter()
                    .filter(|t| {
                        let l = crate::date::in_zone(t.at, offset);
                        l.year == y && l.month == m
                    })
                    .filter_map(|t| self.convert(t.amount, &t.currency))
                    .sum();
                (y, m, total)
            })
            .collect()
    }

    /// Top-ups that expire within the alert window from `now` and haven't yet.
    pub fn expiring(&self, now: i64) -> Vec<&TopUp> {
        let window = self.expiry_alert_days as i64 * 86400;
        self.topups
            .iter()
            .filter(|t| t.expires_at.is_some_and(|e| e > now && e - now <= window))
            .collect()
    }

    /// Adds a top-up with a fresh id; returns the id.
    pub fn add_topup(&mut self, mut t: TopUp) -> String {
        t.id = format!("t{}-{}", t.at, self.topups.len() + 1);
        let id = t.id.clone();
        self.topups.push(t);
        id
    }

    /// Takes an account out, keeping a record of it.
    pub fn remove_account(&mut self, account: &str, name: &str, provider: &str, now: i64) {
        let price = self.prices.remove(account);
        self.removed.push(Removed {
            name: name.into(),
            provider: provider.into(),
            removed_at: now,
            price,
        });
    }
}

/// Days since 1970-01-01 of a calendar date.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2025-10-29 12:00 UTC.
    const NOW: i64 = 20390 * 86400 + 12 * 3600;

    fn wallet() -> Wallet {
        let mut w = Wallet::default();
        w.prices.insert(
            "work".into(),
            Price {
                billing: Billing::Monthly { amount: 100.0 },
                renews_day: Some(3),
                ..Default::default()
            },
        );
        w.prices.insert(
            "side".into(),
            Price {
                billing: Billing::Yearly { amount: 240.0 },
                currency: "EUR".into(),
                renews_day: Some(15),
                renews_month: Some(1),
                ..Default::default()
            },
        );
        w.prices.insert(
            "team".into(),
            Price {
                billing: Billing::Included {
                    with: "work".into(),
                },
                ..Default::default()
            },
        );
        w.rates.insert("EUR".into(), 1.1);
        w
    }

    #[test]
    fn monthly_costs_convert_and_total() {
        let w = wallet();
        assert_eq!(w.monthly("work"), Some(100.0));
        assert!((w.monthly("side").unwrap() - 22.0).abs() < 1e-9);
        assert_eq!(w.monthly("team"), Some(0.0));
        assert_eq!(w.monthly("nobody"), None);
        let (m, y) = w.totals();
        assert!((m - 122.0).abs() < 1e-9 && (y - 1464.0).abs() < 1e-6);
    }

    #[test]
    fn renewals_come_due_in_order() {
        let w = wallet();
        let soon = w.renewals(NOW, 30, 0);
        assert_eq!(soon.len(), 1);
        assert_eq!(soon[0].0, "work");
        assert_eq!(crate::date::in_zone(soon[0].1, 0).day, 3);
        assert_eq!(crate::date::in_zone(soon[0].1, 0).month, 11);
        // Each account's next renewal: a yearly one in January shows within 90 days.
        let later = w.renewals(NOW, 90, 0);
        assert_eq!(
            later.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
            ["work", "side"]
        );
        assert_eq!(crate::date::in_zone(later[1].1, 0).month, 1);
    }

    #[test]
    fn topups_sum_by_month_and_warn_before_expiry() {
        let mut w = wallet();
        w.add_topup(TopUp {
            id: String::new(),
            account: "work".into(),
            amount: 10.0,
            currency: String::new(),
            credits: Some(1000.0),
            at: NOW,
            expires_at: Some(NOW + 3 * 86400),
            note: String::new(),
        });
        w.add_topup(TopUp {
            id: String::new(),
            account: "work".into(),
            amount: 5.0,
            currency: "EUR".into(),
            credits: None,
            at: NOW - 40 * 86400,
            expires_at: Some(NOW + 60 * 86400),
            note: String::new(),
        });
        let months = w.topups_by_month(NOW, 6, 0);
        assert_eq!(months.len(), 6);
        assert_eq!(months[5], (2025, 10, 10.0));
        assert!((months[4].2 - 5.5).abs() < 1e-9);
        assert_eq!(w.expiring(NOW).len(), 1);
        w.remove_account("work", "work", "claude-code", NOW);
        assert_eq!(w.removed.len(), 1);
        assert!(w.monthly("work").is_none());
    }

    #[test]
    fn list_prices_for_known_plans() {
        assert_eq!(list_price("Max 20x"), Some(200.0));
        assert_eq!(list_price("Claude Max"), Some(100.0));
        assert_eq!(list_price("plus"), Some(20.0));
        assert_eq!(list_price("enterprise"), None);
    }
}
