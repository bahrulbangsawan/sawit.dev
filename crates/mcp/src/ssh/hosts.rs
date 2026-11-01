//! The SSH servers saved in Sawit (`ssh_hosts.json`, passwords in the Keychain) and the hosts of
//! `~/.ssh/config`: listing them, and adding, changing and removing saved ones.

use rmcp::model::CallToolResult;
use sawit_core::app_control::Store;
use sawit_core::ssh::{self, Auth, ConfigHost, SshHost};

use super::{AddHostArgs, UpdateHostArgs, find_saved};
use crate::{Sawit, blocking, fail, ok, tri};

fn auth_name(auth: Auth) -> &'static str {
    match auth {
        Auth::Default => "default keys",
        Auth::Key => "key",
        Auth::Password => "password",
    }
}

pub(crate) fn parse_auth(text: &str) -> Result<Auth, String> {
    match text.trim().to_lowercase().as_str() {
        "default" | "agent" => Ok(Auth::Default),
        "key" => Ok(Auth::Key),
        "password" => Ok(Auth::Password),
        other => Err(format!("auth “{other}” must be default, key or password.")),
    }
}

/// Whether `confirm` names `host` again: its id or its name, exactly.
pub(crate) fn names(host: &SshHost, confirm: &str) -> bool {
    !confirm.is_empty() && (confirm == host.id || confirm == host.name)
}

fn saved_line(h: &SshHost, has_password: bool) -> String {
    let mut parts = vec![
        format!("- {} · {}", h.label(), h.address()),
        format!("id {}", h.id),
        format!(
            "auth {}{}",
            auth_name(h.auth),
            if h.auth == Auth::Password && !has_password {
                " (none saved)"
            } else {
                ""
            }
        ),
    ];
    if h.auth == Auth::Key {
        parts.push(format!("key {}", h.identity_file));
    }
    if !h.jump.trim().is_empty() {
        parts.push(format!("via {}", h.jump.trim()));
    }
    if !h.forwards.is_empty() {
        parts.push(format!("forwards {}", h.forwards.join(", ")));
    }
    if !h.group.trim().is_empty() {
        parts.push(format!("group {}", h.group.trim()));
    }
    parts.join(" · ")
}

fn config_line(c: &ConfigHost) -> String {
    let mut line = format!("- {} · {}", c.alias, c.address());
    if let Some(j) = &c.jump {
        line.push_str(&format!(" · via {j}"));
    }
    line
}

impl Sawit {
    pub(super) async fn list_ssh_hosts_tool(&self) -> CallToolResult {
        let list = tri!(self.ssh_hosts());
        let hosts = list.search("");
        let saved: Vec<SshHost> = hosts.iter().map(|h| (*h).clone()).collect();
        let (lines, config) = tri!(
            blocking(move || {
                let lines: Vec<String> = saved
                    .iter()
                    .map(|h| {
                        let has = h.auth == Auth::Password && ssh::has_password(h);
                        saved_line(h, has)
                    })
                    .collect();
                (lines, ssh::load_config_hosts())
            })
            .await
        );
        let mut out = Vec::new();
        out.push(if lines.is_empty() {
            "Saved in Sawit: none".to_string()
        } else {
            format!("Saved in Sawit ({}):", lines.len())
        });
        out.extend(lines);
        out.push(String::new());
        out.push(if config.is_empty() {
            "~/.ssh/config: no hosts".to_string()
        } else {
            format!("~/.ssh/config ({}; use the alias as `host`):", config.len())
        });
        out.extend(config.iter().map(config_line));
        ok(out.join("\n"))
    }

    pub(super) async fn add_ssh_host_tool(&self, a: AddHostArgs) -> CallToolResult {
        let mut host = SshHost::new(&a.name, &a.host);
        host.user = a.user.unwrap_or_default().trim().to_string();
        host.port = a.port.filter(|p| *p != 0 && *p != 22);
        if let Some(key) = a.identity_file.filter(|k| !k.trim().is_empty()) {
            host.auth = Auth::Key;
            host.identity_file = key.trim().to_string();
        }
        let password = a.password.filter(|p| !p.is_empty());
        if password.is_some() {
            host.auth = Auth::Password;
        }
        host.forwards = a.forwards.unwrap_or_default();
        host.jump = a.jump.unwrap_or_default();
        host.group = a.group.unwrap_or_default();
        host.remote_dir = a.remote_dir.unwrap_or_default();
        host.startup = a.startup.unwrap_or_default();
        if let Some(problem) = host.problem() {
            return fail(problem);
        }
        let mut list = tri!(self.ssh_hosts());
        let id = list.add(host.clone(), ssh::now());
        if let Some(pw) = password {
            let h = host.clone();
            tri!(
                blocking(move || ssh::store_password(&h, &pw))
                    .await
                    .and_then(|r| r.map_err(|e| format!("Unable to save the password. {e}")))
            );
        }
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        self.notify(Store::Ssh).await;
        ok(format!(
            "Saved {} ({}) with id {id}.",
            host.label(),
            host.address()
        ))
    }

    pub(super) async fn update_ssh_host_tool(&self, a: UpdateHostArgs) -> CallToolResult {
        let mut list = tri!(self.ssh_hosts());
        let Some(current) = tri!(find_saved(&list, &a.id)).cloned() else {
            return fail(format!("No saved host “{}”. See list_ssh_hosts.", a.id));
        };
        let mut host = current.clone();
        if let Some(v) = a.name {
            host.name = v.trim().to_string();
        }
        if let Some(v) = a.host {
            host.host = v.trim().to_string();
        }
        if let Some(v) = a.user {
            host.user = v.trim().to_string();
        }
        if let Some(p) = a.port {
            host.port = Some(p).filter(|p| *p != 0 && *p != 22);
        }
        if let Some(auth) = a.auth {
            host.auth = tri!(parse_auth(&auth));
        }
        if let Some(key) = a.identity_file {
            host.identity_file = key.trim().to_string();
            if !host.identity_file.is_empty() {
                host.auth = Auth::Key;
            }
        }
        let password = a.password.filter(|p| !p.is_empty());
        if password.is_some() {
            host.auth = Auth::Password;
        }
        if let Some(v) = a.forwards {
            host.forwards = v;
        }
        if let Some(v) = a.jump {
            host.jump = v;
        }
        if let Some(v) = a.group {
            host.group = v;
        }
        if let Some(v) = a.remote_dir {
            host.remote_dir = v;
        }
        if let Some(v) = a.startup {
            host.startup = v;
        }
        if let Some(problem) = host.problem() {
            return fail(problem);
        }
        // Off password sign-in, the saved password goes: that needs a yes.
        let forgets = current.auth == Auth::Password && host.auth != Auth::Password;
        if forgets && !a.confirm.as_deref().is_some_and(|c| names(&current, c)) {
            return fail(format!(
                "Refused: this moves {} from password to {} sign-in, which deletes its saved Keychain password. Nothing was changed. To go ahead, pass confirm=\"{}\" (its id or name).",
                current.label(),
                auth_name(host.auth),
                a.id
            ));
        }
        let edited = host.clone();
        list.update(&current.id, |h| *h = edited);
        if let Some(pw) = password {
            let h = host.clone();
            tri!(
                blocking(move || ssh::store_password(&h, &pw))
                    .await
                    .and_then(|r| r.map_err(|e| format!("Unable to save the password. {e}")))
            );
        } else if forgets {
            let h = host.clone();
            let _ = blocking(move || ssh::delete_password(&h)).await;
        }
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        self.notify(Store::Ssh).await;
        ok(format!(
            "Updated {}.\n{}",
            host.label(),
            saved_line(&host, true)
        ))
    }

    pub(super) async fn remove_ssh_host_tool(&self, key: &str, confirm: &str) -> CallToolResult {
        let mut list = tri!(self.ssh_hosts());
        let Some(host) = tri!(find_saved(&list, key)).cloned() else {
            return fail(format!("No saved host “{key}”. See list_ssh_hosts."));
        };
        if !names(&host, confirm) {
            return fail(format!(
                "Refused: removing {} ({}) deletes it and its Keychain password for good. Pass confirm=\"{key}\" (its id or name).",
                host.label(),
                host.address()
            ));
        }
        list.remove(&host.id);
        tri!(list.save(&self.ssh_file()).map_err(|e| e.to_string()));
        let h = host.clone();
        let _ = blocking(move || ssh::delete_password(&h)).await;
        self.notify(Store::Ssh).await;
        ok(format!("Removed {} ({}).", host.label(), host.address()))
    }
}
