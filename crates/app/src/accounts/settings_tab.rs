//! The Accounts page's Settings tab: the shell block (a plain `claude` starts as the active
//! account), the cp1–cp9 shortcuts, and Open at Login. Each change shows what it does first and
//! backs the file up.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::shell::{self, BlockState};

use super::AccountsModel;
use super::page::AccountsPage;
use super::ui::{self, Tone};

/// The login shell's startup file and where the block stands in it.
pub struct ShellStatus {
    pub file: Option<std::path::PathBuf>,
    pub state: BlockState,
    pub pool_block: bool,
}

impl AccountsModel {
    pub fn login_shell() -> String {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
    }

    pub fn shell_status(&self) -> ShellStatus {
        let file = shell::target(&self.home, &Self::login_shell());
        let text = file
            .as_ref()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default();
        ShellStatus {
            state: shell::state(&text, &self.root),
            pool_block: shell::has_pool_block(&text),
            file,
        }
    }

    /// Puts the block in (replacing Claude Pool's), after a backup.
    pub fn install_shell(&mut self, cx: &mut Context<Self>) {
        let Some(file) = shell::target(&self.home, &Self::login_shell()) else {
            return;
        };
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let done = shell::with_block(&text, &self.root)
            .and_then(|new| shell::write(&self.root, &file, &new, sawit_core::github::now()));
        match done {
            Ok(b) => {
                self.notice = Some(format!(
                    "{} now starts claude as the active account in new terminals.{}",
                    file.display(),
                    b.map(|b| format!(" The old file is backed up at {}.", b.display()))
                        .unwrap_or_default()
                ))
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    pub fn remove_shell(&mut self, cx: &mut Context<Self>) {
        let Some(file) = shell::target(&self.home, &Self::login_shell()) else {
            return;
        };
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let done = shell::without_block(&text)
            .and_then(|new| shell::write(&self.root, &file, &new, sawit_core::github::now()));
        match done {
            Ok(_) => self.notice = Some(format!("Took Sawit's block out of {}.", file.display())),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    /// Links cp1–cp9 to this app where `claude-pool`'s were (or `~/.local/bin`).
    pub fn install_shortcuts(&mut self, cx: &mut Context<Self>) {
        let path = std::env::var("PATH").unwrap_or_default();
        let (Some(dir), Ok(exe)) = (
            shell::shortcut_dir(&self.home, &path),
            std::env::current_exe(),
        ) else {
            self.error = Some(
                "No writable ~/.local/bin or /opt/homebrew/bin on PATH for the shortcuts.".into(),
            );
            cx.notify();
            return;
        };
        match shell::install_shortcuts(&self.root, &dir, &exe, sawit_core::github::now()) {
            Ok((linked, skipped)) => {
                self.notice = Some(format!(
                    "Linked {} in {}.{}",
                    linked.join(", "),
                    dir.display(),
                    if skipped.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " Left {} alone: they aren't Sawit's or Claude Pool's.",
                            skipped.join(", ")
                        )
                    }
                ))
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }

    pub fn remove_shortcuts(&mut self, cx: &mut Context<Self>) {
        let path = std::env::var("PATH").unwrap_or_default();
        let (Some(dir), Ok(exe)) = (
            shell::shortcut_dir(&self.home, &path),
            std::env::current_exe(),
        ) else {
            return;
        };
        match shell::remove_shortcuts(&dir, &exe) {
            Ok(n) => self.notice = Some(format!("Removed {n} shortcuts from {}.", dir.display())),
            Err(e) => self.error = Some(e.to_string()),
        }
        cx.notify();
    }
}

impl AccountsPage {
    /// Shows the block's before/after, then installs it.
    fn confirm_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let status = m.shell_status();
        let Some(file) = status.file else { return };
        let detail = format!(
            "Sawit adds this to {}{}, after backing the file up:\n\n{}",
            file.display(),
            if status.pool_block {
                " in place of Claude Pool's block"
            } else {
                ""
            },
            shell::snippet(&m.root)
        );
        let answer = window.prompt(
            PromptLevel::Info,
            "Start claude as the active account?",
            Some(&detail),
            &["Add to Startup File", "Cancel"],
            cx,
        );
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                model.update(cx, |m, cx| m.install_shell(cx));
            }
        })
        .detach();
    }

    /// Asks before taking the shell block out of the startup file.
    fn confirm_remove_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let file = self.model.read(cx).shell_status().file;
        let Some(file) = file else { return };
        let answer = crate::confirm::Confirm::new(
            format!("Remove Sawit's block from {}?", file.display()),
            "Remove Block",
        )
        .detail("A plain claude in a new terminal starts as the default account again. Open in Terminal keeps working. The file is backed up first.")
        .ask(window, cx);
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await {
                model.update(cx, |m, cx| m.remove_shell(cx));
            }
        })
        .detach();
    }

    /// Asks before removing the cp1–cp9 links.
    fn confirm_remove_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = crate::confirm::Confirm::new("Remove the cp1–cp9 shortcuts?", "Remove Shortcuts")
            .detail("Only the links that point at Sawit go. Open in Terminal and sawit account open keep working.")
            .ask(window, cx);
        let model = self.model.clone();
        cx.spawn(async move |_, cx| {
            if answer.await {
                model.update(cx, |m, cx| m.remove_shortcuts(cx));
            }
        })
        .detach();
    }

    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let pool = self.render_pool_card(cx);
        let m = self.model.read(cx);
        let status = m.shell_status();
        let home = m.home.to_string_lossy().to_string();
        let file = status
            .file
            .as_ref()
            .map(|f| f.display().to_string().replacen(&home, "~", 1));
        let shell_desc = match (&file, status.state) {
            (None, _) => {
                "This login shell isn't zsh or bash, so there is no startup file to change."
                    .to_string()
            }
            (Some(f), BlockState::Installed) => format!(
                "Installed in {f}: a plain claude in a new terminal starts as the active account."
            ),
            (Some(f), BlockState::Outdated) => {
                format!("Installed in {f}, but older than this version of Sawit.")
            }
            (Some(f), BlockState::Absent) if status.pool_block => format!(
                "{f} has Claude Pool's block, which reads Claude Pool's pointer, not Sawit's."
            ),
            (Some(f), BlockState::Absent) => format!(
                "Adds a block to {f} so a plain claude uses the active account. Open in Terminal works either way."
            ),
            (Some(f), BlockState::Unbalanced) => format!(
                "The block's markers in {f} were edited by hand: fix the file, then come back."
            ),
        };
        let installed = matches!(status.state, BlockState::Installed | BlockState::Outdated);
        let install_label = match status.state {
            _ if file.is_none() => None,
            BlockState::Outdated => Some("Update…"),
            BlockState::Absent if status.pool_block => Some("Replace Claude Pool's Block…"),
            BlockState::Absent => Some("Install…"),
            _ => None,
        };
        let shell_control =
            h_flex()
                .gap_2()
                .when(status.state == BlockState::Installed, |el| {
                    el.child(ui::pill(
                        "accounts-shell-state",
                        Tone::Good,
                        Some(Lucide::Check),
                        "Installed",
                        cx,
                    ))
                })
                .when_some(install_label, |el, label| {
                    el.child(
                        Button::new("accounts-shell-install")
                            .xsmall()
                            .outline()
                            .icon(Lucide::Download)
                            .label(label)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_shell(window, cx)),
                            ),
                    )
                })
                .when(installed, |el| {
                    el.child(
                        Button::new("accounts-shell-remove")
                            .xsmall()
                            .danger()
                            .outline()
                            .icon(Lucide::Trash)
                            .label("Remove…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_remove_shell(window, cx)
                            })),
                    )
                });
        let path = std::env::var("PATH").unwrap_or_default();
        let shortcut_dir = shell::shortcut_dir(&m.home, &path);
        let shortcuts_in = shortcut_dir.as_ref().filter(|d| d.join("cp1").exists());
        let shortcuts_desc = match shortcuts_in {
            Some(d) => format!("Installed in {}: cp2 starts claude as the second account, in the terminal you type it in.", d.display().to_string().replacen(&home, "~", 1)),
            None => "cp2 starts claude as the second account in the list, in the terminal you type it in (as sawit account open 2 does).".to_string(),
        };
        let shortcuts_control = h_flex()
            .gap_2()
            .when(shortcuts_in.is_some(), |el| {
                el.child(ui::pill(
                    "accounts-cp-state",
                    Tone::Good,
                    Some(Lucide::Check),
                    "Installed",
                    cx,
                ))
                .child(
                    Button::new("accounts-cp-remove")
                        .xsmall()
                        .danger()
                        .outline()
                        .icon(Lucide::Trash)
                        .label("Remove…")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.confirm_remove_shortcuts(window, cx)
                        })),
                )
            })
            .when(shortcuts_in.is_none(), |el| {
                el.child(
                    Button::new("accounts-cp-install")
                        .xsmall()
                        .outline()
                        .icon(Lucide::Plus)
                        .label("Install Shortcuts")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.model.update(cx, |m, cx| m.install_shortcuts(cx))
                        })),
                )
            });
        let (on, shared) = (m.accounts.settings.share_extensions, m.shared.clone());
        let last_run = match shared {
            Some((at, changes)) => {
                let when = sawit_core::github::ago(at, sawit_core::github::now());
                if changes.is_empty() {
                    format!("Last run {when}: everything was already the same.")
                } else {
                    format!("Last run {when}: {}", changes.join("; "))
                }
            }
            None if on => "Runs on each switch of the active account.".into(),
            None => "Turn on sharing to sync.".into(),
        };
        let st = &m.accounts.settings;
        let (alerts, suggest, threshold) =
            (st.limit_alerts, st.suggest_account, st.limit_threshold);
        let threshold_control = ButtonGroup::new("accounts-threshold")
            .outline()
            .compact()
            .xsmall()
            .disabled(!alerts)
            .children([75u8, 80, 85, 90, 95].into_iter().map(|t| {
                Button::new(SharedString::from(format!("accounts-threshold-{t}")))
                    .label(format!("{t}%"))
                    .accessibility_label(format!("Alert at {t}%"))
                    .selected(t == threshold)
                    // The group's own `disabled` leaves its buttons pressable.
                    .disabled(!alerts)
            }))
            .on_click(cx.listener(|this, picked: &Vec<usize>, _, cx| {
                let Some(&ix) = picked.first() else { return };
                let t = [75u8, 80, 85, 90, 95][ix.min(4)];
                this.model.update(cx, |m, cx| {
                    m.accounts.settings.limit_threshold = t;
                    m.save();
                    cx.notify()
                })
            }));
        let login = crate::login_item::status();
        let group = |icon: Lucide, title: &'static str, items: Vec<AnyElement>, cx: &App| {
            v_flex()
                .child(ui::section_header(icon, title, cx))
                .child(ui::group_box(cx).children(ui::rows(items, cx)))
        };
        let shell_rows = vec![
            ui::setting_row(
                Lucide::SquareTerminal,
                "Start claude as the active account",
                shell_desc,
                shell_control,
                false,
                cx,
            )
            .into_any_element(),
            ui::setting_row(
                Lucide::KeyRound,
                "Shortcuts cp1–cp9",
                shortcuts_desc,
                shortcuts_control,
                false,
                cx,
            )
            .into_any_element(),
        ];
        let shared_rows = vec![
            ui::setting_row(
                Lucide::Share2,
                "Share settings between accounts",
                "MCP servers, skills, agents, commands, CLAUDE.md, plugins, hooks and permissions, from ~/.claude. Sign-ins, the status line, theme and model stay per account. Every file changed is backed up first.",
                Switch::new("accounts-share").checked(on).accessibility_label("Share Between Accounts").on_click(cx.listener(|this, on: &bool, _, cx| this.model.update(cx, |m, cx| m.set_sharing(*on, cx)))),
                false,
                cx,
            )
            .into_any_element(),
            ui::setting_row(
                Lucide::RefreshCw,
                "Sync now",
                last_run,
                Button::new("accounts-share-now").xsmall().outline().icon(Lucide::RefreshCw).label("Sync Now").disabled(!on).on_click(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.run_shared(cx)))),
                !on,
                cx,
            )
            .into_any_element(),
        ];
        let alert_rows = vec![
            ui::setting_row(
                Lucide::Bell,
                "Notify near and at a limit",
                "Alerts only inform: nothing switches accounts by itself.",
                Switch::new("accounts-alerts")
                    .checked(alerts)
                    .accessibility_label("Notify Near and at a Limit")
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.accounts.settings.limit_alerts = *on;
                            m.save();
                            cx.notify()
                        })
                    })),
                false,
                cx,
            )
            .into_any_element(),
            ui::setting_row(
                Lucide::Gauge,
                "Alert threshold",
                if alerts {
                    "The share of a limit used that sends the first alert."
                } else {
                    "Turn on alerts to choose when they fire."
                },
                threshold_control,
                !alerts,
                cx,
            )
            .into_any_element(),
            ui::setting_row(
                Lucide::Users,
                "Suggest the account with the most room",
                "The account chip offers a switch when the active account nears its limit.",
                Switch::new("accounts-suggest")
                    .checked(suggest)
                    .accessibility_label("Suggest the Account with the Most Room")
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.model.update(cx, |m, cx| {
                            m.accounts.settings.suggest_account = *on;
                            m.save();
                            cx.notify()
                        })
                    })),
                false,
                cx,
            )
            .into_any_element(),
        ];
        let startup_rows = vec![match login {
            Some(on) => ui::setting_row(
                Lucide::Power,
                "Open Sawit at login",
                "Keeps shared settings in sync and the title bar's usage current. Usage tracking itself runs inside Claude Code.",
                Switch::new("accounts-open-at-login").checked(on).accessibility_label("Open Sawit at Login").on_click(cx.listener(|_, on: &bool, _, cx| {
                    crate::login_item::set(*on);
                    cx.notify();
                })),
                false,
                cx,
            )
            .into_any_element(),
            None => ui::setting_row(
                Lucide::Power,
                "Open Sawit at login",
                "Available when Sawit runs from its app bundle in Applications.",
                div(),
                true,
                cx,
            )
            .into_any_element(),
        }];
        let (scroll, content) = ui::tab_body("accounts-settings");
        scroll
            .child(
                content
                    .children(pool)
                    .child(group(Lucide::SquareTerminal, "Shell", shell_rows, cx))
                    .child(group(Lucide::Share2, "Shared settings", shared_rows, cx))
                    .child(group(Lucide::Bell, "Limit alerts", alert_rows, cx))
                    .child(group(Lucide::Power, "Startup", startup_rows, cx)),
            )
            .into_any_element()
    }
}
