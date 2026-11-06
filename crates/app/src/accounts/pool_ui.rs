//! Taking over from Claude Pool in the app: the Accounts tab offers the import while Claude
//! Pool's data is there and Sawit hasn't imported it; the Settings tab offers to remove Claude
//! Pool once nothing needs it. Both show everything first, back up first, and ask.

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use sawit_core::agent_accounts::import::{self, PoolPaths};

use super::AccountsModel;
use super::page::AccountsPage;

/// Whether Claude Pool's menu bar app runs.
fn pool_running() -> bool {
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "ClaudePool"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Moves `path` to the Trash (where Finder can put it back).
fn trash(path: &std::path::Path) -> Result<(), String> {
    use objc2_foundation::{NSFileManager, NSString, NSURL};
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let fm = NSFileManager::defaultManager();
    fm.trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| e.localizedDescription().to_string())
}

impl AccountsModel {
    /// Claude Pool's data is here and hasn't been imported.
    pub fn pool_to_import(&self) -> bool {
        !self.accounts.settings.imported_from_pool && import::found(&self.home).is_some()
    }

    /// Anything of Claude Pool's is still on this Mac.
    pub fn pool_present(&self) -> bool {
        let p = PoolPaths::of(&self.home);
        p.home_dir.exists() || p.support.exists() || p.app.exists()
    }

    fn do_import(&mut self, cx: &mut Context<Self>) {
        let now = sawit_core::github::now();
        let done = import::plan(&self.home, &self.root, &Self::login_shell())
            .and_then(|p| import::apply(&p, &self.root, now));
        match done {
            Ok(lines) => {
                self.reload();
                self.accounts.settings.imported_from_pool = true;
                self.save();
                self.notice = Some(format!(
                    "Imported from Claude Pool: {}. Claude Pool itself is still there; quit it, and Settings offers to remove it.",
                    lines.join("; ")
                ));
                self.check_all(cx);
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    fn do_remove(&mut self, cx: &mut Context<Self>) {
        let list: Vec<_> = self.list().into_iter().cloned().collect();
        let refs: Vec<_> = list.iter().collect();
        let r = import::removal(
            &self.home,
            &PoolPaths::of(&self.home),
            &import::link_dirs(&self.home),
            &self.root,
            &refs,
            &Self::login_shell(),
            pool_running(),
            sawit_core::github::now(),
        );
        let result = import::remove(&r, &self.home).and_then(|()| match &r.app {
            Some(app) => trash(app).map_err(|e| sawit_core::CoreError::Config(format!("The backup is made and the folders are gone, but the app couldn't go to the Trash: {e}"))),
            None => Ok(()),
        });
        match result {
            Ok(()) => {
                self.notice = Some(format!(
                    "Removed Claude Pool. Its data is in {}{}.",
                    r.archive.display(),
                    if r.app.is_some() {
                        "; the app is in the Trash. If it still opens at login, turn it off in System Settings › General › Login Items"
                    } else {
                        ""
                    }
                ))
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}

impl AccountsPage {
    pub(super) fn confirm_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let plan = match import::plan(&m.home, &m.root, &AccountsModel::login_shell()) {
            Ok(p) => p,
            Err(e) => {
                self.model.update(cx, |m, cx| {
                    m.error = Some(e.to_string());
                    cx.notify()
                });
                return;
            }
        };
        let detail = format!(
            "{}\n\nClaude Pool's own files are copied, not moved; every file Sawit changes is backed up first. Quit Claude Pool afterwards: two apps syncing settings would undo each other.",
            plan.summary()
                .iter()
                .map(|l| format!("• {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        let answer = window.prompt(
            PromptLevel::Info,
            "Import from Claude Pool?",
            Some(&detail),
            &["Import", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |m, cx| m.do_import(cx));
            }
        })
        .detach();
    }

    pub(super) fn confirm_remove_pool(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let list: Vec<_> = m.list().into_iter().cloned().collect();
        let refs: Vec<_> = list.iter().collect();
        let r = import::removal(
            &m.home,
            &PoolPaths::of(&m.home),
            &import::link_dirs(&m.home),
            &m.root,
            &refs,
            &AccountsModel::login_shell(),
            pool_running(),
            sawit_core::github::now(),
        );
        if !r.blockers.is_empty() {
            let why = r.blockers.join(" ");
            self.model.update(cx, |m, cx| {
                m.error = Some(format!("Claude Pool can't be removed yet. {why}"));
                cx.notify()
            });
            return;
        }
        let n = m.list().len();
        let mut items = Vec::new();
        if let Some(app) = &r.app {
            items.push(format!("• {} (to the Trash)", app.display()));
        }
        if !r.links.is_empty() {
            items.push(format!(
                "• {} (links to that app)",
                r.links
                    .iter()
                    .map(|l| l.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        for f in &r.folders {
            items.push(format!("• {}", f.display()));
        }
        let detail = format!(
            "Sawit now does everything Claude Pool did, with your {n} accounts. This removes:\n{}\n\nThe folders are backed up first to {}. Claude Pool's launch at login stops with the app; check System Settings › Login Items if it lingers.\n\nYour Claude accounts, their folders and sign-ins are not touched.",
            items.join("\n"),
            r.archive.display()
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            "Remove Claude Pool?",
            Some(&detail),
            &["Remove Claude Pool", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |m, cx| m.do_remove(cx));
            }
        })
        .detach();
    }

    /// The import offer, at the top of the Accounts tab.
    pub(super) fn render_import_offer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.model.read(cx).pool_to_import() {
            return None;
        }
        Some(
            h_flex()
                .mx_4()
                .mt_3()
                .p_3()
                .gap_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary)
                .child(div().flex_1().text_sm().child("Claude Pool's accounts, usage history, status line and settings are on this Mac. Bring them into Sawit?"))
                .child(Button::new("accounts-import").small().primary().label("Import from Claude Pool…").on_click(cx.listener(|this, _, window, cx| this.confirm_import(window, cx))))
                .into_any_element(),
        )
    }

    /// The removal offer, on the Settings tab.
    pub(super) fn render_pool_card(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.model.read(cx).pool_present() {
            return None;
        }
        let muted = cx.theme().muted_foreground;
        Some(
            v_flex()
                .gap_2()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().font_semibold().child("Claude Pool"))
                .child(div().text_sm().text_color(muted).child("Claude Pool is still on this Mac. Once its accounts are imported and it has quit, Sawit can remove it, after packing its data into one backup."))
                .child(Button::new("accounts-remove-pool").small().outline().label("Remove Claude Pool…").on_click(cx.listener(|this, _, window, cx| this.confirm_remove_pool(window, cx))))
                .into_any_element(),
        )
    }
}
