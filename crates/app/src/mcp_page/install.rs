//! Installing and removing a server, client by client (and stopping that), from a row or from
//! the install sheet, which this opens.

use futures::future::abortable;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use sawit_core::mcp_catalog::{Auth, DEFAULT_AWS_REGION, SERVERS, valid_region};
use sawit_core::mcp_clients::{Status, install, read_entries, status_in, uninstall};

use super::{InstallSheet, McpPage, Pair, Sheet, plural, target_label};
use crate::rt;

impl McpPage {
    pub(super) fn act(&mut self, pair: Pair, remove: bool, cx: &mut Context<Self>) {
        let (s, t) = pair;
        let Some(target) = self.targets.get(t).cloned() else {
            return;
        };
        // GitHub's server is handed the sign-in: none, nothing to install. Removing still works.
        if !remove && self.github_gate(s, cx).is_some() {
            return;
        }
        if !self.busy.insert(pair) {
            return;
        }
        self.errors.remove(&pair);
        let spec = self.spec(s);
        let cli = self.cli_for(target.client);
        let (run, stop) = abortable({
            let (target, spec) = (target.clone(), spec.clone());
            async move {
                if remove {
                    uninstall(&target, &spec.name, cli.as_deref()).await
                } else {
                    install(&target, &spec, cli.as_deref()).await
                }
            }
        });
        self.stops.insert(pair, stop);
        // The client, not its place in the list: a detection can move it.
        let id = (target.client, target.config.clone());
        let rx = rt::spawn(cx, async move {
            // `None` when it was stopped; the config is read again either way.
            let result = run.await.ok();
            (result, read_entries(&target))
        });
        cx.spawn(async move |this, cx| {
            if let Ok((result, read)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.busy.remove(&pair);
                    this.stops.remove(&pair);
                    let Some(t) = this
                        .targets
                        .iter()
                        .position(|x| x.client == id.0 && x.config == id.1)
                    else {
                        cx.notify();
                        return;
                    };
                    let pair = (s, t);
                    match result {
                        Some(Err(e)) => {
                            this.errors.insert(pair, e.to_string());
                        }
                        None => {
                            this.errors.insert(pair, "Stopped.".into());
                        }
                        Some(Ok(())) => {}
                    }
                    // Every server's status in that config: an install can also drop
                    // the old pre-rename entry.
                    for s in 0..SERVERS.len() {
                        let spec = this.spec(s);
                        this.status.insert((s, t), status_in(&read, &spec));
                    }
                    if let Some(entries) = this.entries.get_mut(t) {
                        *entries = read;
                    }
                    if !remove && SERVERS[s].auth == Auth::Browser {
                        if let Some(states) = this.sign_ins.get_mut(&t) {
                            states.remove(SERVERS[s].id);
                        }
                        this.check_sign_ins(vec![t], cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    // Install sheet

    pub(super) fn open_install(&mut self, s: usize, window: &mut Window, cx: &mut Context<Self>) {
        // What's installed, to update or remove; not an install in another
        // Region, which Install would move.
        let selected = (0..self.targets.len())
            .filter(|t| self.is_installed((s, *t)) && self.other_region((s, *t)).is_none())
            .collect();
        let (region, subscription) = if SERVERS[s].has_region() {
            let region = self.region.clone();
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).placeholder(DEFAULT_AWS_REGION);
                state.set_value(region, window, cx);
                state
            });
            let sub = cx.subscribe_in(&input, window, |this, input, e: &InputEvent, _, cx| {
                if matches!(e, InputEvent::Change) {
                    let value = input.read(cx).value().trim().to_string();
                    let region = if value.is_empty() {
                        DEFAULT_AWS_REGION.to_string()
                    } else {
                        value
                    };
                    // It goes into the server's host name.
                    let valid = valid_region(&region);
                    if let Some(Sheet::Install(sheet)) = &mut this.sheet {
                        sheet.region_invalid = !valid;
                    }
                    if valid {
                        this.region_chosen = true;
                        this.set_region(region, cx);
                    }
                    cx.notify();
                }
            });
            (Some(input), Some(sub))
        } else {
            (None, None)
        };
        self.sheet = Some(Sheet::Install(InstallSheet {
            server: s,
            selected,
            region,
            region_invalid: false,
            _subscription: subscription,
        }));
        self.focus.focus(window, cx);
        cx.notify();
    }

    /// Stops every install or removal running for server `s`.
    pub(super) fn stop(&mut self, s: usize, cx: &mut Context<Self>) {
        for (_, stop) in self.stops.iter().filter(|((x, _), _)| *x == s) {
            stop.abort();
        }
        cx.notify();
    }

    /// The targets server `s` can be removed from here (not the ones edited
    /// by hand).
    pub(super) fn removable(&self, s: usize) -> Vec<usize> {
        (0..self.targets.len())
            .filter(|&t| {
                self.targets[t].client.editable()
                    && matches!(
                        self.status.get(&(s, t)),
                        Some(Status::Installed | Status::Outdated(_))
                    )
            })
            .collect()
    }

    /// Asks, then removes server `s` from every client it's installed in.
    pub(super) fn confirm_uninstall(
        &mut self,
        s: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let targets = self.removable(s);
        if targets.is_empty() {
            return;
        }
        let names: Vec<String> = targets
            .iter()
            .map(|&t| target_label(&self.targets[t]))
            .collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Uninstall {} from {}?",
                SERVERS[s].name,
                plural(targets.len(), "client", "clients")
            ),
            Some(&format!(
                "Its entry is removed from: {}. Restart those clients to drop it.",
                names.join(", ")
            )),
            &["Uninstall", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    for t in targets {
                        this.act((s, t), true, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// Asks, then removes the sheet's server from the ticked clients, as the
    /// row's trash does: the sheet opens with every installed client ticked.
    pub(super) fn confirm_remove_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return;
        };
        let s = sheet.server;
        let targets = self.selected_for(true);
        if targets.is_empty() {
            return;
        }
        let names: Vec<String> = targets
            .iter()
            .map(|&t| target_label(&self.targets[t]))
            .collect();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!(
                "Remove {} from {}?",
                SERVERS[s].name,
                plural(targets.len(), "client", "clients")
            ),
            Some(&format!(
                "Its entry is removed from: {}. Restart those clients to drop it.",
                names.join(", ")
            )),
            &["Remove", "Cancel"],
            cx,
        );
        cx.spawn(async move |this, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = this.update(cx, |this, cx| {
                    for t in targets {
                        this.act((s, t), true, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// The ticked clients the sheet's Install (or Remove) acts on.
    pub(super) fn selected_for(&self, remove: bool) -> Vec<usize> {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return Vec::new();
        };
        let s = sheet.server;
        let mut chosen: Vec<usize> = sheet
            .selected
            .iter()
            .copied()
            .filter(|&t| {
                let editable = self.targets.get(t).is_some_and(|x| x.client.editable());
                // Without npx, there's nothing to install there.
                let installable = editable && !self.bridge_blocked((s, t));
                match self.status.get(&(s, t)) {
                    Some(Status::NotInstalled) => !remove && installable,
                    Some(Status::Outdated(_)) => {
                        if remove {
                            editable
                        } else {
                            installable
                        }
                    }
                    Some(Status::Installed) => remove && editable,
                    _ => false,
                }
            })
            .collect();
        chosen.sort();
        chosen
    }

    pub(super) fn install_selected(&mut self, cx: &mut Context<Self>) {
        let Some(Sheet::Install(sheet)) = &self.sheet else {
            return;
        };
        let s = sheet.server;
        for t in self.selected_for(false) {
            self.act((s, t), false, cx);
        }
    }
}
