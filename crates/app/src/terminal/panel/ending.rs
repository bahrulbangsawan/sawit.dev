//! Closing a project closes its terminals (backlog item 12): every tab grouped with the project,
//! the Run panel's scripts among them, goes. What a terminal started stops with it, not only its
//! shell: a terminal that goes reads its process tree and ends it, SIGHUP, then SIGTERM, then
//! SIGKILL after a grace period ([`TerminalView`]'s drop, [`sawit_core::terminal::reap`]), so no
//! dev server keeps its port. Terminals of no project are not touched. Quitting stops them too,
//! with short grace periods the app waits for ([`stop_all_now`]).

use std::path::Path;

use gpui_kit::{AnyWindowHandle, App, Context, Entity, Window};
use sawit_core::terminal::reap;

use super::TerminalPanel;
#[cfg(doc)]
use crate::terminal::view::TerminalView;

/// What closing a project would end in its terminals.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectTerminals {
    /// How many tabs are the project's.
    pub tabs: usize,
    /// What runs in them, one name per busy pane ([`TerminalPanel::running`]).
    pub running: Vec<String>,
}

/// The grace periods when the app quits.
const QUIT_GRACE: reap::Grace = reap::Grace {
    hangup: std::time::Duration::from_millis(150),
    terminate: std::time::Duration::from_millis(350),
};

impl TerminalPanel {
    /// The indices of the tabs grouped with `root`.
    fn tabs_of(&self, root: &Path, cx: &App) -> Vec<usize> {
        (0..self.tabs.len())
            .filter(|&ix| self.tabs[ix].read(cx).group.as_deref() == Some(root))
            .collect()
    }

    /// What closing project `root` would end in its terminals.
    pub fn project_terminals(&self, root: &Path, cx: &App) -> ProjectTerminals {
        let tabs = self.tabs_of(root, cx);
        ProjectTerminals {
            tabs: tabs.len(),
            running: tabs
                .iter()
                .flat_map(|&ix| self.running_in_tab(ix, cx))
                .collect(),
        }
    }

    /// Closes every tab of project `root`; each terminal stops what it started as it goes. The
    /// keyboard, if it was in one of them, goes to the tab shown next.
    pub fn close_project(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let closing = self.tabs_of(root, cx);
        if closing.is_empty() {
            return;
        }
        // Its pinned tabs, as they are now (names, folders, locks), come back next time.
        self.save_pins(root, cx);
        let had_keyboard = closing
            .iter()
            .any(|&ix| self.tabs[ix].read(cx).has_focus(window, cx));
        for ix in closing.into_iter().rev() {
            self.close(ix, cx);
        }
        if had_keyboard {
            self.focus_active(window, cx);
        }
        cx.notify();
    }

    /// Everything the panel's terminals started, now.
    fn all_trees(&self, cx: &App) -> Vec<u32> {
        let shells: Vec<u32> = self
            .all_panes(cx)
            .filter_map(|(_, _, v)| v.read(cx).shell_pid())
            .collect();
        reap::process_tree(&shells)
    }
}

/// Stops everything the terminals of `windows` started, before the app quits: it won't be there
/// for the grace periods, so they are short and waited for here.
pub fn stop_all_now(windows: &[(AnyWindowHandle, Entity<TerminalPanel>)], cx: &App) {
    for (_, panel) in windows {
        let panel = panel.read(cx);
        // Pinned tabs come back as they are now: their names and folders.
        for project in panel.projects.iter() {
            panel.save_pins(project, cx);
        }
    }
    let pids: Vec<u32> = windows
        .iter()
        .flat_map(|(_, panel)| panel.read(cx).all_trees(cx))
        .collect();
    if !pids.is_empty() {
        reap::stop(&pids, QUIT_GRACE);
    }
}

/// Ends `pids` on a thread of its own: SIGHUP, SIGTERM, SIGKILL ([`reap::stop`]).
pub(in crate::terminal) fn stop_trees(pids: Vec<u32>) {
    if pids.is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("sawit-reap".into())
        .spawn(move || reap::stop(&pids, reap::Grace::default()))
        .ok();
}
