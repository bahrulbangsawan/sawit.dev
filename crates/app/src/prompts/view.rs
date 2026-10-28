//! One prompt in a center tab: title, tags and body edited in place (saved
//! automatically), `{{variable}}` fields filled before copying.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState, TabSize};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use pgdesk_core::prompts::{fill, normalize_tags, variables};

use super::store::PromptStore;

pub struct PromptView {
    pub id: String,
    store: Entity<PromptStore>,
    title: Entity<InputState>,
    tags: Entity<InputState>,
    body: Entity<EditorState>,
    /// One input per `{{variable}}`, in order of appearance.
    vars: Vec<(String, Entity<InputState>)>,
    copied: bool,
    _subscriptions: Vec<Subscription>,
}

impl PromptView {
    pub fn new(
        id: String,
        store: Entity<PromptStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prompt = store.read(cx).library.get(&id).cloned();
        let (title_text, tags_text, body_text) = prompt
            .map(|p| (p.title, p.tags.join(", "), p.body))
            .unwrap_or_default();
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Prompt title")
                .default_value(title_text)
        });
        let tags = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("tags, comma-separated (e.g. sql, review)")
                .default_value(tags_text)
        });
        let body = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .language("markdown")
                .line_number(false)
                .folding(false)
                .soft_wrap(true)
                .tab_size(TabSize {
                    tab_size: 2,
                    hard_tabs: false,
                })
                .placeholder(
                    "Write your prompt here. Use {{name}} for parts you fill in when copying.",
                )
                .default_value(body_text);
            state.set_scrollbar_renderer(Some(Rc::new(|_, _, _| div().into_any_element())));
            state
        });
        let subscriptions = vec![
            cx.subscribe(&title, |this, input, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    let v = input.read(cx).value().to_string();
                    this.edit(cx, |p| p.title = v);
                }
            }),
            cx.subscribe(&tags, |this, input, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    let raw = input.read(cx).value().to_string();
                    let tags = normalize_tags(&raw.split([',', ' ']).collect::<Vec<_>>());
                    this.edit(cx, |p| p.tags = tags);
                }
            }),
            cx.subscribe_in(&body, window, |this, input, e: &InputEvent, window, cx| {
                if matches!(e, InputEvent::Change) {
                    let v = input.read(cx).value().to_string();
                    this.sync_vars(&v, window, cx);
                    this.edit(cx, |p| p.body = v);
                }
            }),
        ];
        let mut this = Self {
            id,
            store,
            title,
            tags,
            body,
            vars: Vec::new(),
            copied: false,
            _subscriptions: subscriptions,
        };
        let text = this.body.read(cx).value().to_string();
        this.sync_vars(&text, window, cx);
        this
    }

    pub fn title(&self, cx: &App) -> String {
        let t = self.title.read(cx).value();
        if t.trim().is_empty() {
            "(untitled)".into()
        } else {
            t.to_string()
        }
    }

    fn edit(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut pgdesk_core::prompts::Prompt)) {
        let id = self.id.clone();
        self.store.update(cx, |s, cx| s.update(&id, cx, f));
        cx.notify();
    }

    /// Keeps one input per variable, reusing inputs (and what was typed)
    /// for names that stay.
    fn sync_vars(&mut self, body: &str, window: &mut Window, cx: &mut Context<Self>) {
        let names = variables(body);
        if names.iter().eq(self.vars.iter().map(|(n, _)| n)) {
            return;
        }
        let mut old: HashMap<String, Entity<InputState>> = self.vars.drain(..).collect();
        self.vars = names
            .into_iter()
            .map(|name| {
                let input = old.remove(&name).unwrap_or_else(|| {
                    let placeholder = name.clone();
                    cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
                });
                (name, input)
            })
            .collect();
        cx.notify();
    }

    fn copy(&mut self, cx: &mut Context<Self>) {
        let values: HashMap<String, String> = self
            .vars
            .iter()
            .map(|(n, i)| (n.clone(), i.read(cx).value().to_string()))
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let text = fill(&self.body.read(cx).value(), &values);
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let id = self.id.clone();
        self.store.update(cx, |s, cx| s.mark_used(&id, cx));
        self.copied = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.copied = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete prompt \"{}\"?", self.title(cx)),
            Some("This can't be undone."),
            &["Delete", "Cancel"],
            cx,
        );
        let (store, id) = (self.store.clone(), self.id.clone());
        cx.spawn(async move |_, cx| {
            if answer.await == Ok(0) {
                store.update(cx, |s, cx| s.remove(&id, cx));
            }
        })
        .detach();
    }
}

impl Render for PromptView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let prompt = self.store.read(cx).library.get(&self.id).cloned();
        let favorite = prompt.as_ref().is_some_and(|p| p.favorite);
        let used = prompt.as_ref().map_or(0, |p| p.used_count);
        let chars = self.body.read(cx).value().chars().count();
        let muted = cx.theme().muted_foreground;
        let has_vars = !self.vars.is_empty();
        let toolbar = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                Button::new("prompt-fav")
                    .ghost()
                    .small()
                    .icon(if favorite {
                        Icon::new(Lucide::Star).text_color(cx.theme().warning)
                    } else {
                        Icon::new(Lucide::Star)
                    })
                    .tooltip(if favorite {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        let id = this.id.clone();
                        this.store.update(cx, |s, cx| s.toggle_favorite(&id, cx));
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_lg()
                    .font_semibold()
                    .child(Input::new(&self.title).appearance(false)),
            )
            .child(
                Button::new("prompt-duplicate")
                    .ghost()
                    .small()
                    .icon(Lucide::CopyPlus)
                    .tooltip("Duplicate")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let id = this.id.clone();
                        this.store.update(cx, |s, cx| s.duplicate(&id, cx));
                    })),
            )
            .child(
                Button::new("prompt-delete")
                    .ghost()
                    .small()
                    .icon(Lucide::Trash)
                    .tooltip("Delete prompt")
                    .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
            )
            .child(
                Button::new("prompt-copy")
                    .primary()
                    .small()
                    .icon(if self.copied {
                        Lucide::Check
                    } else {
                        Lucide::Copy
                    })
                    .label(if self.copied {
                        "Copied"
                    } else if has_vars {
                        "Fill & copy"
                    } else {
                        "Copy"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.copy(cx))),
            );
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(toolbar)
            .child(
                h_flex()
                    .px_3()
                    .py_1p5()
                    .gap_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Icon::new(Lucide::Tag).xsmall().text_color(muted))
                    .child(
                        div()
                            .flex_1()
                            .child(Input::new(&self.tags).small().appearance(false)),
                    ),
            )
            .when(has_vars, |el| {
                el.child(
                    v_flex()
                        .px_3()
                        .py_2()
                        .gap_1p5()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().secondary)
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child("Variables — filled in when copying (empty keeps {{name}})"),
                        )
                        .children(self.vars.iter().map(|(name, input)| {
                            h_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(140.))
                                        .flex_none()
                                        .text_sm()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .truncate()
                                        .child(format!("{{{{{name}}}}}")),
                                )
                                .child(div().flex_1().child(Input::new(input).small()))
                        })),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_1()
                    .child(Editor::new(&self.body).size_full().bordered(false)),
            )
            .child(
                h_flex()
                    .px_3()
                    .h(px(24.))
                    .gap_3()
                    .text_xs()
                    .text_color(muted)
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(format!("{chars} characters"))
                    .child(format!("used {used}×"))
                    .child(div().flex_1())
                    .child("Saved automatically"),
            )
    }
}
