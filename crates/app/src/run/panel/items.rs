//! The Run panel's rows: the saved commands and the scripts, each with the terminals it starts,
//! their running state, and the rows by name for agents (`app_run_list`, `app_run_script`).

use std::path::Path;

use gpui_kit::*;
use sawit_core::scripts::{RunMode, ScriptGroup};

use super::{Item, Launch, RunPanel};
use crate::terminal::JobTag;

impl RunPanel {
    pub(super) fn saved_items(&self, root: &Path, cx: &App) -> Vec<Item> {
        self.store
            .read(cx)
            .saved
            .for_project(root)
            .iter()
            .map(|c| {
                let lines = c.lines();
                let many = lines.len() > 1;
                let launches = lines
                    .into_iter()
                    .enumerate()
                    .map(|(i, command)| Launch {
                        tag: JobTag {
                            key: format!("saved\u{0}{}\u{0}{}\u{0}{i}", root.display(), c.id),
                            name: if many {
                                format!("{} {}", c.name, i + 1)
                            } else {
                                c.name.clone()
                            },
                        },
                        dir: root.to_path_buf(),
                        command,
                    })
                    .collect();
                let joiner = match c.mode {
                    RunMode::Sequence => " → ",
                    RunMode::Parallel => " ‖ ",
                };
                Item {
                    name: c.name.clone(),
                    detail: c.commands.join(joiner),
                    launches,
                    saved: Some(c.id.clone()),
                }
            })
            .collect()
    }

    pub(super) fn group_items(group: &ScriptGroup) -> Vec<Item> {
        group
            .scripts
            .iter()
            .map(|s| Item {
                name: s.name.clone(),
                detail: if s.body.is_empty() {
                    s.command.clone()
                } else {
                    s.body.clone()
                },
                launches: vec![Launch {
                    tag: JobTag {
                        key: format!(
                            "script\u{0}{}\u{0}{}\u{0}{}",
                            group.dir.display(),
                            group.source,
                            s.name
                        ),
                        name: if group.rel.is_empty() {
                            s.name.clone()
                        } else {
                            format!("{} · {}", group.label, s.name)
                        },
                    },
                    dir: group.dir.clone(),
                    command: s.command.clone(),
                }],
                saved: None,
            })
            .collect()
    }

    /// (open, running) per row, in order.
    pub(super) fn status_snapshot(&self, cx: &App) -> Vec<(bool, bool)> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let terminal = self.terminal.read(cx);
        self.saved_items(root, cx)
            .iter()
            .chain(
                self.groups
                    .iter()
                    .flat_map(Self::group_items)
                    .collect::<Vec<_>>()
                    .iter(),
            )
            .map(|item| {
                let status: Vec<_> = item
                    .keys()
                    .filter_map(|k| terminal.job_status(k, cx))
                    .collect();
                (
                    !status.is_empty(),
                    status.iter().any(|s| s.running.is_some()),
                )
            })
            .collect()
    }

    /// Every row, saved commands first, as the panel lists them.
    fn items(&self, cx: &App) -> Vec<Item> {
        let Some(root) = &self.root else {
            return Vec::new();
        };
        let mut items = self.saved_items(root, cx);
        items.extend(self.groups.iter().flat_map(Self::group_items));
        items
    }

    /// The name `sawit-mcp` uses for a row: a script's tab name
    /// (`web · dev` in a workspace package), or a saved command's name.
    fn label(item: &Item) -> &str {
        match (&item.saved, item.launches.first()) {
            (None, Some(launch)) => &launch.tag.name,
            _ => &item.name,
        }
    }

    /// Every row with its command and whether it is running.
    pub fn describe(&self, cx: &App) -> Result<Vec<serde_json::Value>, String> {
        if self.root.is_none() {
            return Err("No project is open in this window.".into());
        }
        let terminal = self.terminal.read(cx);
        Ok(self
            .items(cx)
            .iter()
            .map(|item| {
                let running = item
                    .keys()
                    .filter_map(|k| terminal.job_status(k, cx))
                    .any(|s| s.running.is_some());
                serde_json::json!({
                    "name": Self::label(item),
                    "command": item.detail,
                    "saved": item.saved.is_some(),
                    "running": running,
                })
            })
            .collect())
    }

    /// The row called `name`: its label, else a unique row with that name.
    fn find(&self, name: &str, cx: &App) -> Result<Item, String> {
        if self.root.is_none() {
            return Err("No project is open in this window.".into());
        }
        if !self.scanned {
            return Err(
                "Sawit is still reading the project's scripts. Try again in a moment.".into(),
            );
        }
        let mut items = self.items(cx);
        if let Some(ix) = items.iter().position(|i| Self::label(i) == name) {
            return Ok(items.swap_remove(ix));
        }
        let mut same: Vec<Item> = items.into_iter().filter(|i| i.name == name).collect();
        match same.len() {
            1 => Ok(same.remove(0)),
            0 => Err(format!(
                "No script or saved command is called “{name}”. See app_run_list."
            )),
            _ => Err(format!(
                "Several scripts are called “{name}”: {}. Use the full name.",
                same.iter().map(Self::label).collect::<Vec<_>>().join(", ")
            )),
        }
    }

    /// The terminals the row called `name` starts, for an agent (the
    /// workspace starts them, to say where they run), and whether they all
    /// run already: they then aren't started twice, only shown.
    pub fn launches_named(&self, name: &str, cx: &App) -> Result<(Vec<Launch>, bool), String> {
        let item = self.find(name, cx)?;
        let terminal = self.terminal.read(cx);
        let running = item.keys().all(|k| {
            terminal
                .job_status(k, cx)
                .is_some_and(|s| s.running.is_some())
        });
        Ok((item.launches, running))
    }

    pub fn stop_named(&mut self, name: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let item = self.find(name, cx)?;
        self.stop(&item, cx);
        Ok(())
    }

    pub(super) fn matches(&self, item: &Item, group: Option<&ScriptGroup>, query: &str) -> bool {
        let has = |s: &str| s.to_lowercase().contains(query);
        query.is_empty()
            || has(&item.name)
            || has(&item.detail)
            || item.launches.iter().any(|l| has(&l.command))
            || group.is_some_and(|g| has(&g.label) || has(&g.tool))
    }
}
