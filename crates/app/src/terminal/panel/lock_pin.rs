//! Locking and pinning terminal tabs, and closing several at once (backlog item 2).
//!
//! A locked tab can't be closed (by its ×, a key, a Close Others or an agent's
//! `terminal_close`) until it is unlocked; typing in it stays allowed. A pinned tab sits at the
//! start of the strip, drawn compact, keeps its place when others move, and comes back when its
//! project opens again: each project's pins (name, folder, lock) are kept in Sawit's data folder
//! ([`sawit_core::terminal::pins`]), and a pinned tab comes back as a fresh shell there.
//! Close Others and Close All skip locked and pinned tabs and say how many they kept; they ask
//! first (the shared confirm) when a tab they close runs something.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, WeakEntity, Window};
use sawit_core::terminal::Command;
use sawit_core::terminal::pins::{PinnedTab, Pins};

use super::TerminalPanel;
use super::open::shell_name;
use crate::terminal::tab::TerminalTab;

/// Which tabs a bulk close takes, besides the one it is opened from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseScope {
    /// Every tab but the one.
    Others,
    /// The tabs before it in the strip.
    Left,
    /// The tabs after it.
    Right,
    /// Tabs with nothing running: Safe Close All.
    Idle,
    /// Every tab, running or not: Force Close All (it asks first when something runs).
    All,
}

impl CloseScope {
    /// From an agent's `terminal_close` scope: `others`, `left`, `right`, `idle` or `all`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "others" => Self::Others,
            "left" => Self::Left,
            "right" => Self::Right,
            "idle" => Self::Idle,
            "all" => Self::All,
            _ => return None,
        })
    }
}

/// What a bulk close would do: the tabs it closes, what runs in them, and the tabs it keeps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BulkClose {
    pub closing: Vec<usize>,
    pub running: Vec<String>,
    /// Locked or pinned tabs in scope that stay.
    pub kept: usize,
}

/// "Kept 2 tabs (locked, pinned or busy).", or nothing.
pub fn kept_note(kept: usize) -> Option<String> {
    match kept {
        0 => None,
        1 => Some("Kept 1 tab (locked, pinned or busy).".into()),
        n => Some(format!("Kept {n} tabs (locked, pinned or busy).")),
    }
}

impl TerminalPanel {
    /// Whether the tab at `ix` is locked.
    pub fn is_locked(&self, ix: usize, cx: &App) -> bool {
        self.tabs.get(ix).is_some_and(|t| t.read(cx).state.locked)
    }

    /// Locks or unlocks the tab at `ix` (`None` flips it); returns whether it is locked.
    pub fn set_locked(&mut self, ix: usize, locked: Option<bool>, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return false;
        };
        let locked = tab.update(cx, |t, _| {
            t.state.locked = locked.unwrap_or(!t.state.locked);
            t.state.locked
        });
        self.save_pins_of(&tab, cx);
        cx.notify();
        locked
    }

    /// Pins or unpins the tab at `ix` (`None` flips it): pinned, it moves to the end of the
    /// pinned run at the strip's start. Returns whether it is pinned and where it is now.
    pub fn set_pinned(
        &mut self,
        ix: usize,
        pinned: Option<bool>,
        cx: &mut Context<Self>,
    ) -> (bool, usize) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return (false, ix);
        };
        let pinned = tab.update(cx, |t, _| {
            t.state.pinned = pinned.unwrap_or(!t.state.pinned);
            t.state.pinned
        });
        self.regroup(cx);
        self.save_pins_of(&tab, cx);
        cx.notify();
        let at = self.tabs.iter().position(|t| *t == tab).unwrap_or(ix);
        (pinned, at)
    }

    /// What closing per `scope` from the tab at `target` would do. Locked tabs always stay,
    /// pinned ones except for [`CloseScope::All`], and busy ones for [`CloseScope::Idle`]; what
    /// the others close may run something (the caller asks first).
    pub fn bulk_close(&self, scope: CloseScope, target: usize, cx: &App) -> BulkClose {
        let mut plan = BulkClose::default();
        for ix in 0..self.tabs.len() {
            let in_scope = match scope {
                CloseScope::Others => ix != target,
                CloseScope::Left => ix < target,
                CloseScope::Right => ix > target,
                CloseScope::Idle | CloseScope::All => true,
            };
            if !in_scope {
                continue;
            }
            let state = &self.tabs[ix].read(cx).state;
            // Force Close All closes pinned tabs too (they come back with their project); a
            // lock always holds.
            if state.locked || (state.pinned && scope != CloseScope::All) {
                plan.kept += 1;
                continue;
            }
            let running = self.running_in_tab(ix, cx);
            if scope == CloseScope::Idle && !running.is_empty() {
                plan.kept += 1;
                continue;
            }
            plan.closing.push(ix);
            plan.running.extend(running);
        }
        plan
    }

    /// Closes per `scope` for the user: asks first (the shared confirm) when a tab it closes
    /// runs something, naming it, and says how many locked or pinned tabs it kept.
    pub fn close_tabs_for_user(
        &mut self,
        scope: CloseScope,
        target: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let plan = self.bulk_close(scope, target, cx);
        if plan.running.is_empty() {
            self.close_planned(&plan, window, cx);
            return;
        }
        let n = plan.closing.len();
        let tabs = if n == 1 {
            "1 terminal".to_string()
        } else {
            format!("{n} terminals")
        };
        let stop = match plan.running.as_slice() {
            [one] => format!("`{one}`"),
            many => format!("{} running processes", many.len()),
        };
        let mut detail = "Closing them ends what runs in them.".to_string();
        if let Some(kept) = kept_note(plan.kept) {
            detail = format!("{detail} {kept}");
        }
        let answer = crate::confirm::Confirm::new(
            format!("Close {tabs} and stop {stop}?"),
            "Close and Stop",
        )
        .detail(detail)
        .items(plan.running.clone())
        .ask(window, cx);
        let tabs: Vec<WeakEntity<TerminalTab>> = plan
            .closing
            .iter()
            .map(|&ix| self.tabs[ix].downgrade())
            .collect();
        let kept = plan.kept;
        cx.spawn_in(window, async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                let closing: Vec<usize> = tabs.iter().filter_map(|t| this.index_of(t)).collect();
                let plan = BulkClose {
                    closing,
                    running: Vec::new(),
                    kept,
                };
                this.close_planned(&plan, window, cx);
            });
        })
        .detach();
    }

    /// An agent's `terminal_close` with a scope: closes what [`Self::bulk_close`] plans, refused
    /// without `confirm` while a tab it would close runs something. Never asks the user (that
    /// would take their keyboard).
    pub fn close_scope_for_agent(
        &mut self,
        scope: CloseScope,
        target: usize,
        confirm: bool,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, String> {
        if target >= self.tabs.len() {
            return Err(format!("There is no terminal {target}."));
        }
        let plan = self.bulk_close(scope, target, cx);
        if !plan.running.is_empty() && !confirm {
            return Err(format!(
                "Nothing was closed: closing them ends what runs there ({}). Pass confirm: true \
                 to close them anyway.",
                plan.running.join(", ")
            ));
        }
        let closed = plan.closing.len();
        // The user's keyboard stays where it is: an agent's close never moves it.
        let mut closing = plan.closing.clone();
        closing.sort_unstable();
        for ix in closing.into_iter().rev() {
            self.close(ix, cx);
        }
        cx.notify();
        Ok(serde_json::json!({ "closed": closed, "kept": plan.kept }))
    }

    /// Closes the tabs `plan` names; the keyboard, if it was in one, goes to the tab shown
    /// next. Says how many it kept.
    pub(super) fn close_planned(
        &mut self,
        plan: &BulkClose,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_keyboard = plan
            .closing
            .iter()
            .any(|&ix| self.tabs[ix].read(cx).has_focus(window, cx));
        let mut closing = plan.closing.clone();
        closing.sort_unstable();
        for ix in closing.into_iter().rev() {
            self.close(ix, cx);
        }
        self.notice = kept_note(plan.kept);
        if had_keyboard {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Where pins are kept: set by the workspace (tests leave it unset, so nothing is written).
    pub fn set_pins_file(&mut self, file: PathBuf) {
        self.pins_file = Some(file);
    }

    /// Saves the pins of `tab`'s project, as the strip has them now.
    pub(super) fn save_pins_of(&self, tab: &gpui_kit::Entity<TerminalTab>, cx: &App) {
        if let Some(project) = tab.read(cx).group.clone() {
            self.save_pins(&project, cx);
        }
    }

    /// Saves `project`'s pinned tabs: name, folder (the shell's, else the project's) and lock.
    pub(super) fn save_pins(&self, project: &Path, cx: &App) {
        let Some(file) = &self.pins_file else {
            return;
        };
        let tabs: Vec<PinnedTab> = self
            .tabs
            .iter()
            .map(|t| t.read(cx))
            .filter(|t| t.state.pinned && t.group.as_deref() == Some(project))
            .map(|t| PinnedTab {
                name: t.state.name.clone(),
                cwd: t
                    .focused_view()
                    .read(cx)
                    .cwd()
                    .unwrap_or_else(|| project.to_path_buf()),
                locked: t.state.locked,
            })
            .collect();
        let mut pins = Pins::load(file);
        pins.set(project, tabs);
        if let Err(e) = pins.save(file) {
            eprintln!("terminal pins: {e}");
        }
    }

    /// Opens `project`'s pinned tabs as they were kept, pinned (and locked), each a fresh shell
    /// in its folder: when the project opens.
    pub(super) fn restore_pins(&mut self, project: &Path, cx: &mut Context<Self>) {
        let Some(file) = &self.pins_file else {
            return;
        };
        let pins = Pins::load(file);
        for pin in pins.of(project).to_vec() {
            let cwd = Some(pin.cwd.clone())
                .filter(|d| d.is_dir())
                .unwrap_or_else(|| project.to_path_buf());
            let cmd = Command {
                cwd: Some(cwd),
                ..Command::default()
            };
            let Some(tab) = self.spawn(cmd, &shell_name(), None, cx) else {
                continue;
            };
            tab.update(cx, |t, _| {
                t.group = Some(project.to_path_buf());
                t.state.name = pin.name.clone();
                t.state.pinned = true;
                t.state.locked = pin.locked;
            });
            self.add_tab(tab, cx);
        }
    }
}
