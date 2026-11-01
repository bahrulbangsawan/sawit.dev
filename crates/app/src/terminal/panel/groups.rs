//! The tab strip's project groups: each tab sits with the tabs of the window's project it started
//! in, the groups in switcher order, and a group can be folded away.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, Entity, Hsla, WeakEntity};
use sawit_core::terminal::{Command, PaneId};

use super::TerminalPanel;
use super::open::shell_name;
use crate::tab_groups::group_color;
use crate::tabs::{Slot, insert_index};
use crate::terminal::tab::TerminalTab;

impl TerminalPanel {
    /// The projects open in the window and the active one, whose folder
    /// new terminals start in and whose group they join. When the first
    /// project opens, shells idle at their prompt follow it: untouched ones
    /// are reopened there (their prompt may not be drawn yet), used ones
    /// `cd`. Busy ones are left alone, and so are all shells on later
    /// switches. Tabs of a closed project stay open, out of any group.
    pub fn set_projects(
        &mut self,
        projects: Vec<PathBuf>,
        active: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let first = self.cwd.is_none() && active.is_some();
        self.projects = projects;
        self.cwd = active;
        self.collapsed.retain(|r| self.projects.contains(r));
        for tab in &self.tabs {
            tab.update(cx, |t, _| {
                if t.group.as_ref().is_some_and(|g| !self.projects.contains(g)) {
                    t.group = None;
                }
            });
        }
        if let Some(dir) = self.cwd.clone().filter(|_| first) {
            let idle: Vec<(usize, PaneId, bool)> = self
                .all_panes(cx)
                .filter(|(_, _, v)| v.read(cx).idle_shell())
                .map(|(ix, pane, v)| (ix, pane, v.read(cx).touched))
                .collect();
            for (ix, pane, touched) in idle {
                let tab = self.tabs[ix].clone();
                if touched {
                    if let Some(view) = tab.read(cx).view(pane) {
                        view.read(cx).cd(&dir);
                    }
                    tab.update(cx, |t, _| t.group = Some(dir.clone()));
                } else if let Some(fresh) = self.spawn_view(
                    Command {
                        cwd: Some(dir.clone()),
                        ..Command::default()
                    },
                    &shell_name(),
                    None,
                    cx,
                ) {
                    // Replacing a view focuses its pane; give the focus back.
                    tab.update(cx, |t, cx| {
                        let focused = t.state.focused;
                        t.replace_view(pane, fresh, cx);
                        t.state.focus(focused);
                        t.group = Some(dir.clone());
                    });
                }
            }
        }
        self.regroup(cx);
        cx.notify();
    }

    /// Where a tab of `group` goes: at the end of that group's tabs.
    fn insert_at(&self, group: Option<&Path>, cx: &App) -> usize {
        insert_index(&slots(&self.groups(cx)), false, group)
    }

    /// The group of each tab, in strip order.
    pub(super) fn groups(&self, cx: &App) -> Vec<Option<PathBuf>> {
        self.tabs.iter().map(|t| t.read(cx).group.clone()).collect()
    }

    /// The colour of the group of project `root`.
    pub(super) fn group_color(&self, root: &Path, cx: &App) -> Hsla {
        let at = self.projects.iter().position(|p| p == root).unwrap_or(0);
        group_color(at, cx)
    }

    /// Adds a tab at its group's end; returns its index.
    fn insert(&mut self, tab: Entity<TerminalTab>, cx: &App) -> usize {
        let group = tab.read(cx).group.clone();
        let at = self.insert_at(group.as_deref(), cx);
        self.tabs.insert(at, tab);
        at
    }

    /// Adds a tab at its group's end without changing which tab is active;
    /// returns its index.
    pub(super) fn add_tab(&mut self, tab: Entity<TerminalTab>, cx: &App) -> usize {
        let at = self.insert(tab, cx);
        if self.tabs.len() > 1 && at <= self.active {
            self.active += 1;
        }
        at
    }

    /// Puts every tab back in group order, keeping the active one.
    fn regroup(&mut self, cx: &App) {
        let active = self.tabs.get(self.active).cloned();
        for tab in std::mem::take(&mut self.tabs) {
            self.insert(tab, cx);
        }
        self.active = active
            .and_then(|a| self.tabs.iter().position(|t| *t == a))
            .unwrap_or(0);
    }

    /// Moves `tab` into `group` (or out of any).
    pub(super) fn move_to_group(
        &mut self,
        tab: &WeakEntity<TerminalTab>,
        group: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = tab.upgrade() {
            tab.update(cx, |t, _| t.group = group);
            self.regroup(cx);
            cx.notify();
        }
    }
}

/// Tabs in `groups` as grouping sees them: terminal tabs are never pinned.
pub(super) fn slots(groups: &[Option<PathBuf>]) -> Vec<Slot<'_>> {
    groups
        .iter()
        .map(|g| Slot {
            pinned: false,
            group: g.as_deref(),
        })
        .collect()
}
