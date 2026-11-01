//! The new instance form: its name, port and PostgreSQL binaries (installed, or downloaded), and
//! adding an existing data directory instead.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{ServerDetail, error_box, field};
use crate::servers::store::BinaryChoice;

/// Majors offered for download when creating an instance.
const DOWNLOADABLE: [u32; 3] = [18, 17, 16];

impl ServerDetail {
    fn choices(&self, cx: &App) -> Vec<(String, BinaryChoice)> {
        let store = self.store.read(cx);
        let mut choices: Vec<(String, BinaryChoice)> = store
            .binaries
            .iter()
            .map(|b| {
                (
                    format!("PostgreSQL {} · {}", b.version, b.source),
                    BinaryChoice::Installed(b.clone()),
                )
            })
            .collect();
        for major in DOWNLOADABLE {
            let installed = store.binaries.iter().any(|b| {
                b.version.major == major && b.source == sawit_core::binaries::BinarySource::Managed
            });
            if !installed {
                choices.push((
                    format!("Download PostgreSQL {major} (no Homebrew/Docker)"),
                    BinaryChoice::Download { major },
                ));
            }
        }
        choices
    }

    pub(super) fn reset_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form_error = None;
        self.form_busy = false;
        self.choice = 0;
        self.name_input
            .update(cx, |s, cx| s.set_value("", window, cx));
        let rx = self.store.update(cx, |s, cx| s.suggest_port(cx));
        cx.spawn_in(window, async move |this, window| {
            if let Ok(port) = rx.await {
                let _ = this.update_in(window, |this, window, cx| {
                    this.port_input
                        .update(cx, |s, cx| s.set_value(port.to_string(), window, cx));
                });
            }
        })
        .detach();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.name_input.read(cx).value().trim().to_string();
        let Ok(port) = self.port_input.read(cx).value().trim().parse::<u16>() else {
            self.form_error = Some("Enter a port from 1 to 65535.".into());
            cx.notify();
            return;
        };
        let Some((_, choice)) = self.choices(cx).into_iter().nth(self.choice) else {
            self.form_error = Some("Choose a PostgreSQL binary.".into());
            cx.notify();
            return;
        };
        self.form_busy = true;
        self.form_error = None;
        cx.notify();
        let task = self
            .store
            .update(cx, |s, cx| s.create(name, port, choice, cx));
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.form_busy = false;
                this.form_error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    fn adopt(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Data Directory".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            let Ok(task) = this.update(cx, |this, cx| {
                this.store.update(cx, |s, cx| s.adopt(dir, cx))
            }) else {
                return;
            };
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.form_error = result.err();
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let choices = self.choices(cx);
        let options: Vec<_> = choices
            .iter()
            .enumerate()
            .map(|(ix, (label, choice))| {
                let selected = ix == self.choice;
                let icon = match choice {
                    BinaryChoice::Installed(_) => Icon::new(IconName::HardDrive),
                    BinaryChoice::Download { .. } => Icon::new(Lucide::Download),
                };
                h_flex()
                    .id(("bin-choice", ix))
                    .px_2()
                    .py_1()
                    .gap_2()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(if selected {
                        cx.theme().ring
                    } else {
                        cx.theme().border
                    })
                    .when(selected, |el| el.bg(cx.theme().list_active))
                    .cursor_pointer()
                    .child(icon.small())
                    .child(div().text_sm().child(label.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choice = ix;
                        cx.notify();
                    }))
            })
            .collect();
        let downloading = self.form_busy
            && matches!(
                choices.get(self.choice),
                Some((_, BinaryChoice::Download { .. }))
            );

        let data_root = {
            let root = self.store.read(cx).paths.root.display().to_string();
            match std::env::var("HOME") {
                Ok(home) if root.starts_with(&home) => root.replacen(&home, "~", 1),
                _ => root,
            }
        };
        v_flex()
            .p_6()
            .gap_4()
            .child(div().text_lg().font_semibold().child("New PostgreSQL Instance"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Stored in {}. Listens on localhost only, with user `postgres` and no password.",
                        data_root
                    )),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(div().flex_1().child(field("Name", Input::new(&self.name_input))))
                    .child(div().w(px(80.)).child(field("Port", Input::new(&self.port_input)))),
            )
            .child(field("Binary", v_flex().gap_1().children(options)))
            .when_some(self.form_error.clone(), |el, e| el.child(error_box(e, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("create")
                            .primary()
                            .small()
                            .loading(self.form_busy)
                            .disabled(self.form_busy)
                            .label(if downloading { "Downloading & creating…" } else { "Create Instance" })
                            .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                    )
                    .child(
                        Button::new("adopt")
                            .ghost()
                            .small()
                            .icon(IconName::FolderOpen)
                            .label("Add Existing Data Directory…")
                            .on_click(cx.listener(|this, _, _, cx| this.adopt(cx))),
                    ),
            )
    }
}
