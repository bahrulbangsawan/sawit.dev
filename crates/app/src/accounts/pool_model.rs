//! The account pool in the app: read from disk off the main thread (`pool_data::collect`) when a
//! pool tab opens and once a minute after, with what the app knows that files don't (sign-ins,
//! plans). The views read [`PoolState`]; the wallet is changed here and saved at once.

use gpui_kit::*;
use sawit_core::agent_accounts::pool_data::{self, Known, PoolData};
use sawit_core::agent_accounts::{readings, wallet};

use super::{AccountsModel, SignIn};
use crate::rt;

/// What the pool tabs show, and whether it is being read.
#[derive(Default)]
pub struct PoolState {
    pub data: Option<PoolData>,
    pub loading: bool,
    pub error: Option<String>,
    /// The history's period in days (Timeline, Analytics: 30 or 90).
    pub days: u32,
}

impl AccountsModel {
    /// Reads the pool again (and stores new readings), unless a read is under way.
    pub fn refresh_pool(&mut self, cx: &mut Context<Self>) {
        if self.pool.loading {
            return;
        }
        self.pool.loading = true;
        if self.pool.days == 0 {
            self.pool.days = 30;
        }
        let (root, home, days) = (self.root.clone(), self.home.clone(), self.pool.days);
        let signed: Vec<(String, bool)> = self
            .sign_in
            .iter()
            .filter_map(|(id, s)| match s {
                SignIn::Known(s) => Some((id.clone(), s.signed_in)),
                _ => None,
            })
            .collect();
        let plans: Vec<(String, String)> = self
            .plan
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let rx = rt::spawn_blocking(cx, move || {
            let signed_in = |id: &str| signed.iter().find(|(x, _)| x == id).map(|(_, s)| *s);
            let plan = |id: &str| plans.iter().find(|(x, _)| x == id).map(|(_, p)| p.clone());
            let known = Known {
                signed_in: &signed_in,
                plan: &plan,
            };
            let now = sawit_core::github::now() as f64;
            pool_data::collect(&root, &home, &known, now, days as f64, true)
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |m, cx| {
                m.pool.loading = false;
                match result {
                    Ok(d) => {
                        m.pool.data = Some(d);
                        m.pool.error = None;
                    }
                    Err(e) => m.pool.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows `days` of history (30 or 90) in Timeline and Analytics.
    pub fn set_pool_days(&mut self, days: u32, cx: &mut Context<Self>) {
        if self.pool.days != days {
            self.pool.days = days;
            self.refresh_pool(cx);
        }
    }

    /// Marks every notification read.
    pub fn mark_notices_read(&mut self, cx: &mut Context<Self>) {
        match readings::open(&self.root).and_then(|db| readings::mark_all_read(&db)) {
            Ok(_) => {
                if let Some(d) = &mut self.pool.data {
                    d.notices.iter_mut().for_each(|n| n.read = true);
                }
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Changes the wallet and saves it.
    pub fn edit_wallet(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut wallet::Wallet)) {
        let Some(d) = &mut self.pool.data else { return };
        f(&mut d.wallet);
        if let Err(e) = wallet::save(&self.root, &d.wallet) {
            self.error = Some(e.to_string());
        }
        cx.notify();
    }
}
