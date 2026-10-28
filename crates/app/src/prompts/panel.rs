//! Sidebar list of saved prompts: search (`#tag` too), new, WhatsApp
//! import, and one-click copy.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::prompts::{fill, variables};

use super::store::PromptStore;

pub enum PromptsEvent {
    Open(String),
}

impl EventEmitter<PromptsEvent> for PromptsPanel {}

const ROW_HEIGHT: f32 = 44.;

pub struct PromptsPanel {
    store: Entity<PromptStore>,
    search: Entity<InputState>,
    scroll: UniformListScrollHandle,
    copied: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl PromptsPanel {
    pub fn new(store: Entity<PromptStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search prompts or #tag"));
        let subscriptions = vec![
            cx.observe(&store, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            store,
            search,
            scroll: UniformListScrollHandle::new(),
            copied: None,
            _subscriptions: subscriptions,
        }
    }

    fn results(&self, cx: &App) -> Vec<String> {
        let query = self.search.read(cx).value();
        self.store
            .read(cx)
            .library
            .search(&query)
            .into_iter()
            .map(|p| p.id.clone())
            .collect()
    }

    fn new_prompt(&mut self, cx: &mut Context<Self>) {
        let id = self.store.update(cx, |s, cx| s.create(cx));
        cx.emit(PromptsEvent::Open(id));
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a WhatsApp chat export (_chat.txt)".into()),
        });
        let store = self.store.clone();
        cx.spawn(async move |_, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            store.update(cx, |s, cx| s.import_whatsapp_file(path, cx));
        })
        .detach();
    }

    /// Copies a prompt as-is (variables stay as `{{name}}`; fill them in
    /// the prompt's tab).
    fn copy(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(body) = self.store.read(cx).library.get(id).map(|p| p.body.clone()) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(fill(&body, &Default::default())));
        self.store.update(cx, |s, cx| s.mark_used(id, cx));
        self.copied = Some(id.to_string());
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.copied.as_deref() == Some(id.as_str()) {
                    this.copied = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn render_row(&self, ix: usize, id: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(p) = self.store.read(cx).library.get(id).cloned() else {
            return div().into_any_element();
        };
        let muted = cx.theme().muted_foreground;
        let copied = self.copied.as_deref() == Some(id);
        let has_vars = !variables(&p.body).is_empty();
        let preview: String = p
            .body
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let (open_id, copy_id, star_id) = (id.to_string(), id.to_string(), id.to_string());
        h_flex()
            .id(("prompt-row", ix))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px_2()
            .gap_1p5()
            .cursor_pointer()
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(PromptsEvent::Open(open_id.clone()))))
            .child(
                Button::new(("prompt-star", ix))
                    .ghost()
                    .xsmall()
                    .icon(if p.favorite {
                        Icon::new(Lucide::Star).text_color(cx.theme().warning)
                    } else {
                        Icon::new(Lucide::Star)
                    })
                    .tooltip(if p.favorite {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.store
                            .update(cx, |s, cx| s.toggle_favorite(&star_id, cx));
                    })),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_sm().truncate().child(if p.title.is_empty() {
                        "(untitled)".to_string()
                    } else {
                        p.title.clone()
                    }))
                    .child(
                        h_flex()
                            .gap_1()
                            .text_xs()
                            .text_color(muted)
                            .children(p.tags.iter().take(3).map(|t| format!("#{t}")))
                            .child(div().min_w_0().truncate().child(preview)),
                    ),
            )
            .child(
                Button::new(("prompt-copy", ix))
                    .ghost()
                    .xsmall()
                    .icon(if copied { Lucide::Check } else { Lucide::Copy })
                    .tooltip(if has_vars {
                        "Copy as is (fill variables in the prompt tab)"
                    } else {
                        "Copy"
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.copy(&copy_id, cx);
                    })),
            )
            .into_any_element()
    }
}

impl Render for PromptsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let results = self.results(cx);
        let store = self.store.read(cx);
        let total = store.library.items.len();
        let message = store
            .load_error
            .clone()
            .map(|e| {
                (
                    format!("prompts.json can't be read, so it won't be overwritten: {e}"),
                    true,
                )
            })
            .or_else(|| store.notice.clone().map(|n| (n, false)));
        let count = results.len();
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(
                h_flex()
                    .h(px(32.))
                    .px_2()
                    .gap_0p5()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .font_semibold()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("PROMPT · {total}")),
                    )
                    .child(
                        Button::new("prompt-import")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Download)
                            .tooltip("Import from a WhatsApp chat export (_chat.txt)")
                            .on_click(cx.listener(|this, _, _, cx| this.import(cx))),
                    )
                    .child(
                        Button::new("prompt-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New prompt")
                            .on_click(cx.listener(|this, _, _, cx| this.new_prompt(cx))),
                    ),
            )
            .child(div().p_2().child(Input::new(&self.search).small().cleanable(true)))
            .when_some(message, |el, (text, error)| {
                el.child(
                    div()
                        .px_2()
                        .pb_1()
                        .text_xs()
                        .whitespace_normal()
                        .text_color(if error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(text),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .when(count > 0, |el| {
                        el.child(
                            uniform_list(
                                "prompt-rows",
                                count,
                                cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                                    let results = this.results(cx);
                                    range
                                        .filter_map(|ix| {
                                            let id = results.get(ix)?.clone();
                                            Some(this.render_row(ix, &id, cx))
                                        })
                                        .collect()
                                }),
                            )
                            .track_scroll(&self.scroll)
                            .size_full(),
                        )
                    })
                    .when(count == 0, |el| {
                        el.child(
                            div()
                                .p_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .whitespace_normal()
                                .child(if total == 0 {
                                    "No prompts yet. Click + to create one, or import from WhatsApp (Export Chat → _chat.txt)."
                                } else {
                                    "No matching prompts."
                                }),
                        )
                    }),
            )
    }
}
