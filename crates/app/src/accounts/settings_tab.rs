//! The Accounts page's Settings tab: the shell block (a plain `claude` starts as the active
//! account), the cp1–cp9 shortcuts, and Open at Login. Each change shows what it does first and
//! backs the file up.

use gpui_kit::component::button::*;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::agent_accounts::shell::{self, BlockState};

use super::AccountsModel;
use super::page::AccountsPage;

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

    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let m = self.model.read(cx);
        let status = m.shell_status();
        let muted = cx.theme().muted_foreground;
        let file = status
            .file
            .as_ref()
            .map(|f| f.display().to_string())
            .unwrap_or_else(|| "(this login shell isn't zsh or bash)".into());
        let (state_text, install_label) = match status.state {
            BlockState::Installed => (
                "Installed: a plain claude in a new terminal starts as the active account.",
                None,
            ),
            BlockState::Outdated => (
                "Installed, but older than this version of Sawit.",
                Some("Update…"),
            ),
            BlockState::Absent if status.pool_block => (
                "Claude Pool's block is there: it reads Claude Pool's pointer, not Sawit's.",
                Some("Replace Claude Pool's Block…"),
            ),
            BlockState::Absent => (
                "Not installed: a plain claude starts as the default account. Open in Terminal works either way.",
                Some("Install…"),
            ),
            BlockState::Unbalanced => (
                "The block's markers were edited by hand: fix the file, then come back.",
                None,
            ),
        };
        let card = |title: &'static str| {
            v_flex()
                .gap_2()
                .p_3()
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .child(div().text_sm().font_semibold().child(title))
        };
        let login = crate::login_item::status();
        v_flex()
            .id("accounts-settings")
            .flex_1()
            .overflow_y_scroll()
            .p_6()
            .gap_4()
            .child(
                card("Shell Integration")
                    .child(div().text_xs().text_color(muted).child(file))
                    .child(div().text_sm().child(state_text))
                    .child(
                        h_flex()
                            .gap_1()
                            .when_some(install_label, |el, label| {
                                el.child(Button::new("accounts-shell-install").small().outline().label(label).on_click(cx.listener(|this, _, window, cx| this.confirm_shell(window, cx))))
                            })
                            .when(matches!(status.state, BlockState::Installed | BlockState::Outdated), |el| {
                                el.child(Button::new("accounts-shell-remove").small().ghost().label("Remove").on_click(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.remove_shell(cx)))))
                            }),
                    ),
            )
            .child(
                card("Shortcuts cp1–cp9")
                    .child(div().text_sm().text_color(muted).child("cp2 starts claude as account 2 in the list, right in the terminal you type it in (the same as sawit account open 2)."))
                    .child(
                        h_flex()
                            .gap_1()
                            .child(Button::new("accounts-cp-install").small().outline().label("Install Shortcuts").on_click(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.install_shortcuts(cx)))))
                            .child(Button::new("accounts-cp-remove").small().ghost().label("Remove").on_click(cx.listener(|this, _, _, cx| this.model.update(cx, |m, cx| m.remove_shortcuts(cx))))),
                    ),
            )
            .child(
                card("Open at Login")
                    .child(div().text_sm().text_color(muted).child("Sawit opens when you log in, so shared settings stay in sync and the title bar shows usage as Claude Pool did. Usage tracking itself runs inside Claude Code and needs no app."))
                    .child(match login {
                        Some(on) => Switch::new("accounts-open-at-login")
                            .checked(on)
                            .label("Open Sawit at Login")
                            .on_click(cx.listener(|_, on: &bool, _, cx| {
                                crate::login_item::set(*on);
                                cx.notify();
                            }))
                            .into_any_element(),
                        None => div().text_xs().text_color(muted).child("Available when Sawit runs from its app bundle in Applications.").into_any_element(),
                    }),
            )
            .into_any_element()
    }
}
