//! Saved servers and `~/.ssh/config` hosts, shared by the sidebar and the
//! server tabs: reachability, saved passwords, and how to connect.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter};
use sawit_core::instances::AppPaths;
use sawit_core::ssh::{
    self, Auth, ConfigHost, HostList, Probe, SshHost, launch, launch_alias, now,
};

use crate::rt;
use crate::terminal::Remote;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

pub enum SshStoreEvent {
    /// A server was deleted (its tab should close).
    Removed(String),
}

impl EventEmitter<SshStoreEvent> for SshStore {}

#[derive(Clone)]
pub enum ProbeState {
    Checking,
    Done(Probe),
}

/// Key of a saved server: what its sessions and status are filed under.
pub fn saved_key(id: &str) -> String {
    format!("ssh\u{0}{id}")
}

/// Key of a `~/.ssh/config` host.
pub fn alias_key(alias: &str) -> String {
    format!("ssh-config\u{0}{alias}")
}

pub struct SshStore {
    file: PathBuf,
    pub list: HostList,
    pub config: Vec<ConfigHost>,
    /// Set when the file exists but can't be read: nothing is written then.
    pub load_error: Option<String>,
    pub notice: Option<String>,
    save_scheduled: bool,
    pub probes: HashMap<String, ProbeState>,
    /// Ids of servers with a password in the Keychain.
    pub passwords: HashSet<String>,
}

impl SshStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let file = AppPaths::default_macos().root.join("ssh_hosts.json");
        let this = Self {
            file: file.clone(),
            list: HostList::default(),
            config: Vec::new(),
            load_error: None,
            notice: None,
            save_scheduled: false,
            probes: HashMap::new(),
            passwords: HashSet::new(),
        };
        let rx = rt::spawn(cx, async move {
            let list = HostList::load(&file);
            // Which password servers have one saved (never the password itself).
            let saved: HashSet<String> = match &list {
                Ok(l) => l
                    .hosts
                    .iter()
                    .filter(|h| h.auth == Auth::Password)
                    .filter(|h| ssh::has_password(h))
                    .map(|h| h.id.clone())
                    .collect(),
                Err(_) => HashSet::new(),
            };
            (list, ssh::load_config_hosts(), saved)
        });
        cx.spawn(async move |this, cx| {
            let Ok((list, config, saved)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                match list {
                    Ok(list) => this.list = list,
                    Err(e) => this.load_error = Some(e.to_string()),
                }
                this.config = config;
                this.passwords = saved;
                this.probe_all(cx);
                cx.notify();
            });
        })
        .detach();
        this
    }

    /// Reads the saved servers again after another process (`sawit-mcp`)
    /// changed them, then `~/.ssh/config`; tabs of deleted servers close.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let file = self.file.clone();
        let rx = rt::spawn_blocking(cx, move || {
            let list = HostList::load(&file)?;
            let saved: HashSet<String> = list
                .hosts
                .iter()
                .filter(|h| h.auth == Auth::Password)
                .filter(|h| ssh::has_password(h))
                .map(|h| h.id.clone())
                .collect();
            Ok::<_, sawit_core::CoreError>((list, saved))
        });
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok((list, saved)) => {
                        let gone: Vec<String> = this
                            .list
                            .hosts
                            .iter()
                            .filter(|h| list.get(&h.id).is_none())
                            .map(|h| h.id.clone())
                            .collect();
                        this.list = list;
                        this.passwords = saved;
                        this.load_error = None;
                        for id in gone {
                            cx.emit(SshStoreEvent::Removed(id));
                        }
                    }
                    Err(e) => this.notice = Some(format!("Unable to read saved servers. {e}")),
                }
                this.refresh(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Reads `~/.ssh/config` again and re-checks every server.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let rx = rt::spawn(cx, async { ssh::load_config_hosts() });
        cx.spawn(async move |this, cx| {
            let Ok(config) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.config = config;
                this.probe_all(cx);
            });
        })
        .detach();
    }

    pub fn probe_all(&mut self, cx: &mut Context<Self>) {
        let keys: Vec<String> = self
            .list
            .hosts
            .iter()
            .map(|h| saved_key(&h.id))
            .chain(self.config.iter().map(|h| alias_key(&h.alias)))
            .collect();
        for key in keys {
            self.probe(&key, cx);
        }
    }

    /// Checks whether the server with `key` answers.
    pub fn probe(&mut self, key: &str, cx: &mut Context<Self>) {
        let rx = if let Some(id) = key.strip_prefix("ssh\u{0}") {
            let Some(host) = self.list.get(id).cloned() else {
                return;
            };
            rt::spawn(cx, ssh::probe_host(host))
        } else if let Some(alias) = key.strip_prefix("ssh-config\u{0}") {
            rt::spawn(cx, ssh::probe_alias(alias.to_string()))
        } else {
            return;
        };
        self.probes.insert(key.to_string(), ProbeState::Checking);
        cx.notify();
        let key = key.to_string();
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                this.probes.insert(key, ProbeState::Done(result));
                cx.notify();
            });
        })
        .detach();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        if self.load_error.is_some() || self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let Ok((list, file)) = this.update(cx, |this, _| {
                this.save_scheduled = false;
                (this.list.clone(), this.file.clone())
            }) else {
                return;
            };
            let rx = cx.update(|cx| rt::spawn(cx, async move { list.save(&file) }));
            if let Ok(Err(e)) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.notice = Some(format!("Unable to save servers. {e}"));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// A new, empty server; returns its id.
    pub fn create(&mut self, cx: &mut Context<Self>) -> String {
        let id = self.list.add(SshHost::new("", ""), now());
        self.changed(cx);
        id
    }

    /// A saved copy of a `~/.ssh/config` host: connecting by its alias keeps
    /// every setting the config gives it.
    pub fn save_alias(&mut self, alias: &str, cx: &mut Context<Self>) -> String {
        let id = self.list.add(SshHost::new(alias, alias), now());
        self.changed(cx);
        self.probe(&saved_key(&id), cx);
        id
    }

    pub fn duplicate(&mut self, id: &str, cx: &mut Context<Self>) -> Option<String> {
        let source = self.list.get(id)?.clone();
        let mut copy = SshHost::new(&format!("{} (copy)", source.label()), &source.host);
        let new_id = copy.id.clone();
        copy = SshHost {
            id: new_id,
            name: copy.name,
            last_connected_at: 0,
            ..source
        };
        let id = self.list.add(copy, now());
        self.changed(cx);
        self.probe(&saved_key(&id), cx);
        Some(id)
    }

    pub fn update(&mut self, id: &str, cx: &mut Context<Self>, edit: impl FnOnce(&mut SshHost)) {
        if self.list.update(id, edit) {
            self.changed(cx);
        }
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(host) = self.list.get(id).cloned()
            && self.list.remove(id)
        {
            if self.passwords.remove(id) {
                drop(rt::spawn(cx, async move { ssh::delete_password(&host) }));
            }
            self.probes.remove(&saved_key(id));
            cx.emit(SshStoreEvent::Removed(id.to_string()));
            self.changed(cx);
        }
    }

    /// Saves (or, when empty, removes) the server's password in the Keychain.
    pub fn set_password(&mut self, id: &str, password: String, cx: &mut Context<Self>) {
        let Some(host) = self.list.get(id).cloned() else {
            return;
        };
        let clear = password.is_empty();
        let rx = rt::spawn(cx, async move {
            if clear {
                ssh::delete_password(&host)
            } else {
                ssh::store_password(&host, &password)
            }
        });
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            let Ok(result) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) if clear => {
                        this.passwords.remove(&id);
                    }
                    Ok(()) => {
                        this.passwords.insert(id);
                    }
                    Err(e) => this.notice = Some(format!("Unable to save the password. {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// How to open a session to the server with `key`, or why it can't be.
    pub fn remote(&mut self, key: &str, cx: &mut Context<Self>) -> Result<Remote, String> {
        if let Some(id) = key.strip_prefix("ssh\u{0}") {
            let host = self.list.get(id).cloned().ok_or("This server was deleted.")?;
            if let Some(problem) = host.problem() {
                return Err(format!("{}: {problem}", host.label()));
            }
            // The app answers ssh's password prompt itself (see main.rs).
            let helper = self
                .passwords
                .contains(id)
                .then(std::env::current_exe)
                .and_then(Result::ok);
            self.list.update(id, |h| h.last_connected_at = now());
            self.changed(cx);
            Ok(Remote {
                key: key.to_string(),
                name: host.label(),
                launch: launch(&host, helper.as_deref()),
            })
        } else if let Some(alias) = key.strip_prefix("ssh-config\u{0}") {
            Ok(Remote {
                key: key.to_string(),
                name: alias.to_string(),
                launch: launch_alias(alias),
            })
        } else {
            Err("Unknown server.".into())
        }
    }

    /// The `ssh` command for `key`, to paste into another terminal.
    pub fn command_line(&self, key: &str) -> Option<String> {
        if let Some(id) = key.strip_prefix("ssh\u{0}") {
            self.list.get(id).map(ssh::plain_command)
        } else {
            key.strip_prefix("ssh-config\u{0}")
                .map(|alias| format!("ssh {alias}"))
        }
    }
}
