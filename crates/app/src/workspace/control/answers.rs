//! What an agent's window commands answer: `app_state`'s view of the window, the terminals a
//! script runs in, and whether a tab closed or the user was asked first.

use super::*;

impl Workspace {
    /// The window as `sawit-mcp`'s `app_state` shows it.
    pub fn describe(&self, cx: &App) -> Value {
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .enumerate()
            .map(|(ix, tab)| {
                let (kind, title, path, dirty) = match &tab.item {
                    PaneItem::Server => {
                        ("instance".to_string(), "Instance".to_string(), None, false)
                    }
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        ("file".to_string(), e.title(), Some(e.path.clone()), e.dirty)
                    }
                    PaneItem::Other {
                        key, title, file, ..
                    } => (
                        key.split('\u{0}').next().unwrap_or_default().to_string(),
                        title.to_string(),
                        file.clone(),
                        false,
                    ),
                };
                json!({
                    "index": ix,
                    "title": title,
                    "kind": kind,
                    "path": path,
                    "active": ix == self.active,
                    "pinned": tab.pinned,
                    "unsaved": dirty,
                })
            })
            .collect();
        json!({
            "project": self.root(cx),
            "projects": self.roots(cx),
            "welcome_screen": self.welcome.is_some(),
            "panel": panel_name(self.sidebar),
            "layout": {
                "sidebar": self.layout.sidebar,
                "center": self.layout.center,
                "terminal": self.layout.terminal,
            },
            "tabs": tabs,
            "terminals": self.terminal.read(cx).describe(cx),
            "canvas_open": self.boards.iter().any(|b| b.upgrade().is_some()),
            "browser": self
                .browser_tab(self.root(cx).as_deref())
                .map(|(_, b)| b.read(cx).describe()),
            "agents": self.describe_agents(cx),
        })
    }

    /// The agents at work in this window, as `app_state.agents` lists them: on each canvas and
    /// browser where one acted in the last 30 s, its id, label, colour, the surface (`canvas` with
    /// its file, `browser` with its project), what it did last and how long ago.
    fn describe_agents(&self, cx: &App) -> Vec<Value> {
        let boards = self.boards.iter().filter_map(|b| b.upgrade());
        let mut agents: Vec<Value> = boards
            .flat_map(|b| b.read(cx).describe_agents(cx))
            .collect();
        for tab in &self.tabs {
            if let PaneItem::Other { view, .. } = &tab.item
                && let Ok(browser) = view.clone().downcast::<BrowserView>()
            {
                agents.extend(browser.read(cx).describe_agents(cx));
            }
        }
        agents
    }
}

pub(super) fn panel_name(tab: SidebarTab) -> &'static str {
    match tab {
        SidebarTab::Servers => "instances",
        SidebarTab::Files => "files",
        SidebarTab::Git => "git",
        SidebarTab::Database => "database",
        SidebarTab::Run => "run",
        SidebarTab::Ports => "ports",
        SidebarTab::Network => "network",
        SidebarTab::Prompts => "prompts",
        SidebarTab::Ssh => "ssh",
        SidebarTab::ShellConfig => "shell_config",
    }
}

/// `app_run_script`'s answer: whether the script started or ran already, and the terminal (tab
/// and pane) it runs in, or each terminal of a saved command that runs several, by name. The
/// agent's `terminal_read` and `terminal_input` need them: the script doesn't come in front of a
/// terminal the user types in.
pub(super) fn script_answer(name: &str, running: bool, at: &[(String, usize, usize)]) -> Value {
    let mut answer = match running {
        false => json!({ "started": name }),
        true => json!({
            "running": name,
            "note": "It was running already, so it wasn't started again. app_stop_script stops it.",
        }),
    };
    match at {
        [(_, tab, pane)] => {
            answer["terminal"] = json!(tab);
            answer["pane"] = json!(pane);
        }
        _ => {
            answer["terminals"] = at
                .iter()
                .map(|(name, tab, pane)| json!({ "name": name, "terminal": tab, "pane": pane }))
                .collect();
        }
    }
    answer
}

/// `app_close_tab`'s answer for the tab at `ix`, with `tabs` open now: closed, or, when the close
/// asked the user first (`asking`: what about), that the question is up. The tab then closes only
/// if they agree, as when they close it themselves.
pub(super) fn close_answer(ix: usize, tabs: usize, asking: Option<&str>) -> Value {
    match asking {
        None => json!({ "closed": ix, "tabs": tabs }),
        Some(why) => json!({
            "asking": ix,
            "tabs": tabs,
            "note": format!(
                "Not closed yet: {why}, so Sawit asked the user whether to close it. It closes if \
                 they agree; app_state shows whether it did."
            ),
        }),
    }
}
