//! The model's actions on one account and on all: usage tracking, Check Usage Now, shared
//! settings.

use gpui_kit::*;
use sawit_core::agent_accounts::{Account, run, tracking};

use super::AccountsModel;
use crate::rt;

impl AccountsModel {
    /// Turns usage tracking on or off (after the page's confirmation).
    pub fn set_tracking(&mut self, id: &str, on: bool, cx: &mut Context<Self>) {
        let Some(mut a) = self.accounts.get(id).cloned() else {
            return;
        };
        let now = sawit_core::github::now();
        let done = if on {
            tracking::enable(&self.root, &mut a, now)
        } else {
            tracking::disable(&self.root, &mut a, now)
        };
        match done {
            Ok(backup) => {
                if let Some(slot) = self.accounts.accounts.iter_mut().find(|x| x.id == id) {
                    *slot = a;
                }
                self.save();
                self.notice = Some(match backup {
                    Some(b) => format!(
                        "Usage tracking is {}. settings.json was backed up to {}.",
                        if on { "on" } else { "off" },
                        b.display()
                    ),
                    None => format!("Usage tracking is {}.", if on { "on" } else { "off" }),
                });
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Shared settings: one run, when they are on.
    pub fn sync_shared(&mut self, cx: &mut Context<Self>) {
        if self.accounts.settings.share_extensions {
            self.run_shared(cx);
        }
    }

    /// One shared-settings run now (Sync Now).
    pub fn run_shared(&mut self, cx: &mut Context<Self>) {
        let (root, home) = (self.root.clone(), self.home.clone());
        let accounts: Vec<Account> = self.list().into_iter().cloned().collect();
        let rx = rt::spawn_blocking(cx, move || {
            let refs: Vec<&Account> = accounts.iter().collect();
            sawit_core::agent_accounts::shared::sync(&root, &home, &refs, sawit_core::github::now())
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |m, cx| {
                match result {
                    Ok(r) => m.shared = Some((sawit_core::github::now(), r.changes)),
                    Err(e) => m.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn set_sharing(&mut self, on: bool, cx: &mut Context<Self>) {
        self.accounts.settings.share_extensions = on;
        self.save();
        self.sync_shared(cx);
        cx.notify();
    }

    /// "Check Usage Now" for `id`.
    pub fn check_usage(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(account) = self.accounts.get(id).cloned() else {
            return;
        };
        let (home, root) = (self.home.clone(), self.root.clone());
        let Some(bin) = self.claude.clone() else {
            return;
        };
        self.checking_usage = Some(id.into());
        cx.notify();
        let rx = rt::spawn_blocking(cx, move || run::check_usage(&bin, &account, &home, &root));
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |m, cx| {
                m.checking_usage = None;
                match result {
                    Ok(s) => {
                        m.usage.insert(id, s);
                    }
                    Err(e) => m.error = Some(e.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}
