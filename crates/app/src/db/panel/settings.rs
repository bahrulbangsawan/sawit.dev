//! Connection Settings: the form that renames a connection, changes its connection string or
//! password, or removes it.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::DatabasePanel;

/// The Connection Settings form for one profile.
pub(super) struct SettingsForm {
    profile: String,
    label: Entity<InputState>,
    url: Entity<InputState>,
    password: Entity<InputState>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl DatabasePanel {
    pub(super) fn open_settings(
        &mut self,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(profile) = self.store.read(cx).profile(name).cloned() else {
            return;
        };
        let label = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(profile.name.clone())
                .default_value(profile.label.clone().unwrap_or_default())
        });
        let url = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("postgres:// or mysql://user@localhost/db")
                .default_value(profile.connection_string())
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Leave empty to keep the saved one")
                .masked(true)
        });
        let subscriptions = [&label, &url, &password]
            .into_iter()
            .map(|input| {
                cx.subscribe(input, |this, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.save_settings(cx);
                    }
                })
            })
            .collect();
        label.update(cx, |s, cx| s.focus(window, cx));
        self.show_url = false;
        self.settings = Some(SettingsForm {
            profile: profile.name,
            label,
            url,
            password,
            error: None,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    fn save_settings(&mut self, cx: &mut Context<Self>) {
        let Some(form) = &self.settings else {
            return;
        };
        let (name, label, url, password) = (
            form.profile.clone(),
            form.label.read(cx).value().to_string(),
            form.url.read(cx).value().to_string(),
            form.password.read(cx).value().to_string(),
        );
        let result = self.store.update(cx, |s, cx| {
            s.update_profile(&name, &label, &url, Some(password), cx)
        });
        match result {
            Ok(()) => self.settings = None,
            Err(e) => {
                if let Some(form) = &mut self.settings {
                    form.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    /// Asks first (the shared confirm), then removes the connection and its password.
    fn remove_from_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(form) = &self.settings else {
            return;
        };
        let name = form.profile.clone();
        let shown = self
            .store
            .read(cx)
            .profile(&name)
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| name.clone());
        let answer = crate::confirm::Confirm::new(
            format!("Remove the connection {shown}?"),
            "Remove Connection",
        )
        .thing(shown)
        .detail(
            "Sawit forgets it and deletes its password from the Keychain. The database itself \
             isn't touched.",
        )
        .ask(window, cx);
        cx.spawn(async move |this, cx| {
            if !answer.await {
                return;
            }
            let _ = this.update(cx, |this, cx| {
                if this.settings.as_ref().is_some_and(|f| f.profile == name) {
                    this.settings = None;
                }
                this.expanded.remove(&name);
                this.store.update(cx, |s, cx| s.remove(&name, cx));
                cx.notify();
            });
        })
        .detach();
    }
}

impl DatabasePanel {
    pub(super) fn render_settings(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let form = self.settings.as_ref()?;
        let muted = cx.theme().muted_foreground;
        let title = self
            .store
            .read(cx)
            .profile(&form.profile)
            .map(|p| p.display_name().to_string())
            .unwrap_or_default();
        let field = |label: &'static str, input: &Entity<InputState>, hint: &'static str| {
            v_flex()
                .gap_1()
                .child(div().text_xs().font_medium().child(label))
                .child(Input::new(input).small())
                .child(div().text_xs().text_color(muted).child(hint))
        };
        // A server (PostgreSQL, MySQL) has a connection string and a password; a file doesn't.
        let postgres = self
            .store
            .read(cx)
            .profile(&form.profile)
            .is_none_or(|p| p.kind.is_server());
        Some(
            v_flex()
                .p_2()
                .gap_3()
                .border_b_1()
                .border_color(cx.theme().border)
                .child(
                    v_flex()
                        .child(div().text_sm().font_semibold().child("Connection Settings"))
                        .child(div().text_xs().text_color(muted).truncate().child(title)),
                )
                .child(field(
                    "Name",
                    &form.label,
                    "Shown in the sidebar. Leave empty to use the connection details.",
                ))
                .when(postgres, |el| {
                    el.child(field(
                        "Connection string",
                        &form.url,
                        "Change the host, database or SSL settings here. Saving reconnects.",
                    ))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().font_medium().child("Password"))
                            .child(Input::new(&form.password).small().mask_toggle())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Stored in Keychain, never shown."),
                            ),
                    )
                })
                .when_some(form.error.clone(), |el, e| {
                    el.child(
                        div()
                            .id("db-settings-error")
                            .max_h(px(96.))
                            .overflow_y_scroll()
                            .whitespace_normal()
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(e),
                    )
                })
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("db-settings-save")
                                .small()
                                .primary()
                                .label("Save")
                                .on_click(cx.listener(|this, _, _, cx| this.save_settings(cx))),
                        )
                        .child(
                            Button::new("db-settings-cancel")
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.settings = None;
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("db-settings-remove")
                                .small()
                                .danger()
                                .ghost()
                                .icon(Lucide::Trash)
                                .label("Remove…")
                                .tooltip("Removes the connection and its Keychain password")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.remove_from_settings(window, cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
