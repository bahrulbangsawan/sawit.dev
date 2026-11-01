//! What detection finds, read again off the UI thread, and what it says of each tool: installed,
//! a newer release, installed outside what Sawit runs, busy, and the commands a run of it uses.

use gpui_kit::*;
use sawit_core::dev_tools::{
    Detection, Outside, TOOLS, Tool, detect, install_command, installed_with, is_newer,
    latest_versions, plan, uninstall_command, upgrade_command,
};
use sawit_core::package_manager::{self, Use};

use super::{State, ToolsPage};
use crate::catalog::tilde;
use crate::rt;

impl ToolsPage {
    /// Detects again and rereads the latest versions and the package managers.
    pub(super) fn refresh(&mut self, cx: &mut Context<Self>) {
        self.reload(cx);
        let rx = rt::spawn(cx, latest_versions());
        let pms = rt::spawn(cx, package_manager::detect());
        cx.spawn(async move |this, cx| {
            if let Ok(latest) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.latest = latest;
                    cx.notify();
                });
            }
            if let Ok(pms) = pms.await {
                let _ = this.update(cx, |this, cx| {
                    this.pm = package_manager::choose(
                        &pms,
                        this.pm,
                        crate::package_managers::remembered(),
                        Use::Global,
                    );
                    this.pms = pms;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Detects installed tools again (off the UI thread).
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.loading = true;
        let rx = rt::spawn(cx, detect());
        cx.spawn(async move |this, cx| {
            if let Ok(detection) = rx.await {
                let _ = this.update(cx, |this, cx| this.detected(detection, cx));
            }
        })
        .detach();
        cx.notify();
    }

    /// What detection found.
    pub(super) fn detected(&mut self, detection: Detection, cx: &mut Context<Self>) {
        self.found = detection.found;
        self.problem = detection.problem;
        self.loading = false;
        // Detection now agrees with these; the rest keep their
        // optimistic or failed state.
        for (id, s) in self.states.iter_mut() {
            if *s == State::Removed
                && let Some(f) = self.found.get(id)
            {
                *s = State::Failed(
                    format!(
                        "Still found at {}: that copy was installed another way (Homebrew, say).",
                        tilde(&f.path)
                    ),
                    None,
                );
            }
        }
        self.states.retain(|id, s| {
            let found = self.found.contains_key(id);
            // A copy "still found" that is gone now: detection agrees with the uninstall.
            !(found && matches!(s, State::Done | State::InTerminal | State::Skipped(_))
                || !found && matches!(s, State::Removed | State::Failed(_, None)))
        });
        self.selected.retain(|id| !self.found.contains_key(id));
        cx.notify();
    }

    /// Installed per detection, or the installer just succeeded; not when
    /// the uninstaller just did.
    pub(super) fn installed(&self, id: &str) -> bool {
        match self.states.get(id) {
            Some(State::Done) => true,
            Some(State::Removed) => false,
            _ => self.found.contains_key(id),
        }
    }

    /// The newer release an installed tool can upgrade to. None for an app
    /// installed outside what upgrades it ([`Self::outside`]).
    pub(super) fn newer(&self, id: &str) -> Option<&String> {
        let found = self.found.get(id)?;
        if found.outside.is_some() {
            return None;
        }
        let installed = found.version.as_ref()?;
        let latest = self.latest.get(id)?;
        (is_newer(latest, installed) && self.states.get(id) != Some(&State::Done)).then_some(latest)
    }

    /// What the app found was installed outside of, when it isn't the copy
    /// its own commands install, upgrade and remove (PureMac dragged in from
    /// its disk image): the row says so, with no Upgrade, Reinstall or
    /// Uninstall.
    pub(super) fn outside(&self, id: &str) -> Option<Outside> {
        self.found.get(id)?.outside
    }

    /// Whether the row offers Reinstall: installed, and not an app installed
    /// outside its installer, which Reinstall would fail on (`brew install
    /// --cask` over an app already there) or replace (Zed's script).
    pub(super) fn offers_reinstall(&self, id: &str) -> bool {
        self.installed(id) && self.outside(id).is_none()
    }

    /// Installed tools with a newer release, not already queued.
    pub(super) fn upgradable(&self) -> Vec<&'static str> {
        TOOLS
            .iter()
            .filter(|t| self.newer(t.id).is_some() && !self.busy(t.id))
            .map(|t| t.id)
            .collect()
    }

    /// The command a run of `tool` uses: its upgrade or its installer. A tool
    /// npm installs globally installs with the package manager picked, and
    /// upgrades (or installs again) with the one that installed it.
    pub(super) fn command(&self, tool: &Tool) -> String {
        if self.upgrades.contains(tool.id) {
            self.upgrade_line(tool)
        } else {
            let owner = self.found.get(tool.id).map(|f| installed_with(&f.path));
            install_command(tool, owner.or(self.pm))
        }
    }

    /// `tool`'s upgrade: with the package manager that installed the copy
    /// found, for one npm installs globally.
    pub(super) fn upgrade_line(&self, tool: &Tool) -> String {
        let owner = self.found.get(tool.id).map(|f| installed_with(&f.path));
        upgrade_command(tool, owner)
    }

    pub(super) fn busy(&self, id: &str) -> bool {
        matches!(
            self.states.get(id),
            Some(State::Queued { .. } | State::Installing | State::Uninstalling)
        )
    }

    /// Missing tools that "Install All" covers (not the ones that need the
    /// terminal).
    pub(super) fn missing(&self) -> Vec<&'static str> {
        TOOLS
            .iter()
            .filter(|t| !t.interactive && !self.installed(t.id) && !self.busy(t.id))
            .map(|t| t.id)
            .collect()
    }

    /// The tools `wanted` would run, prerequisites included.
    pub(super) fn plan_for(&self, wanted: &[&str]) -> Vec<&'static Tool> {
        plan(wanted, &|id| self.installed(id) || self.busy(id))
    }

    /// How to remove the copy detection found, if Sawit can.
    pub(super) fn uninstall_command(&self, tool: &Tool) -> Option<String> {
        uninstall_command(tool, &self.found.get(tool.id)?.path)
    }
}
