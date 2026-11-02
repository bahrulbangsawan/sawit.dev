//! The Run panel as drawn: its header, the filter, the Local Explorer section, the saved commands,
//! then each group of scripts under a heading that folds.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Item, RunPanel};
use crate::states::{Banner, Placeholder};

impl RunPanel {
    pub(super) fn render_section(
        &self,
        id: impl Into<ElementId>,
        title: String,
        tag: Option<String>,
        tip: String,
        collapsed: Option<bool>,
        cx: &App,
    ) -> Stateful<Div> {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .id(id)
            .w_full()
            .h(px(24.))
            .px_2()
            .gap_1()
            .text_xs()
            .font_semibold()
            .text_color(muted)
            .bg(cx.theme().sidebar)
            .when_some(collapsed, |el, collapsed| {
                el.cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .xsmall(),
                    )
            })
            .child(div().flex_1().min_w_0().truncate().child(title))
            .when_some(tag, |el, tag| {
                el.child(
                    div()
                        .flex_none()
                        .px_1()
                        .rounded(px(3.))
                        .font_normal()
                        .text_color(cx.theme().primary)
                        .bg(cx.theme().primary.opacity(0.12))
                        .child(tag),
                )
            })
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
    }
}

impl Render for RunPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let header = h_flex()
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
                    .text_color(muted)
                    .child("RUN"),
            )
            .child(
                Button::new("run-add")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Plus)
                    .tooltip("Save a Command")
                    .disabled(self.root.is_none() || self.store.read(cx).load_error.is_some())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.open_editor(None, window, cx)),
                    ),
            )
            .child(
                Button::new("run-refresh")
                    .ghost()
                    .xsmall()
                    .icon(IconName::RotateCw)
                    .tooltip("Scan Scripts Again")
                    .disabled(self.root.is_none())
                    .loading(self.scanning)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.rescan(cx);
                        cx.notify();
                    })),
            );
        let Some(root) = self.root.clone() else {
            return v_flex()
                .size_full()
                .bg(cx.theme().sidebar)
                .child(header)
                .child(Placeholder::new(Lucide::Play, "No project open").detail(
                    "Open a project folder to run its scripts (dev, build, …) with one click.",
                ));
        };
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let mut rows: Vec<AnyElement> = Vec::new();
        let mut ix = 0;

        // Local Explorer first while nothing is filtered: what it needs is to be seen.
        if query.is_empty() {
            rows.extend(self.render_explorer(cx));
        }

        // Saved commands first: they are what the user picked.
        let saved: Vec<Item> = self
            .saved_items(&root, cx)
            .into_iter()
            .filter(|i| self.matches(i, None, &query))
            .collect();
        if !saved.is_empty() || query.is_empty() {
            rows.push(
                self.render_section(
                    "run-section-saved",
                    "SAVED COMMANDS".into(),
                    None,
                    "Your own commands for this project".into(),
                    None,
                    cx,
                )
                .into_any_element(),
            );
        }
        if saved.is_empty() && query.is_empty() && self.editor.is_none() {
            rows.push(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .text_xs()
                    .text_color(muted)
                    .child(div().flex_1().child(
                        "Save commands you run often, like docker compose up and pnpm dev together.",
                    ))
                    .child(
                        Button::new("run-add-empty")
                            .xsmall()
                            .icon(IconName::Plus)
                            .label("Add")
                            .disabled(self.store.read(cx).load_error.is_some())
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_editor(None, window, cx)),
                            ),
                    )
                    .into_any_element(),
            );
        }
        for item in saved {
            rows.push(self.render_item(ix, item, cx));
            ix += 1;
        }

        let filtering = !query.is_empty();
        let mut shown_scripts = 0;
        for (g_ix, group) in self.groups.clone().iter().enumerate() {
            let items: Vec<Item> = Self::group_items(group)
                .into_iter()
                .filter(|i| self.matches(i, Some(group), &query))
                .collect();
            if items.is_empty() {
                continue;
            }
            shown_scripts += items.len();
            let group_key = format!("{}\u{0}{}", group.dir.display(), group.source);
            let collapsed = !filtering && self.collapsed.contains(&group_key);
            let where_ = if group.rel.is_empty() {
                group.source.clone()
            } else {
                format!("{}/{}", group.rel, group.source)
            };
            rows.push(
                self.render_section(
                    ("run-section", g_ix),
                    group.label.to_uppercase(),
                    Some(group.tool.clone()),
                    format!("{} scripts from {where_}", items.len()),
                    Some(collapsed),
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed.remove(&group_key) {
                        this.collapsed.insert(group_key.clone());
                    }
                    cx.notify();
                }))
                .into_any_element(),
            );
            for item in items {
                if !collapsed {
                    rows.push(self.render_item(ix, item, cx));
                }
                ix += 1;
            }
        }

        let store = self.store.read(cx);
        let load_error = store.load_error.clone();
        let notice = store.notice.clone();
        let editor = self.editor.as_ref().map(|e| self.render_editor(e, cx));
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .child(header)
            .child(
                div()
                    .p_2()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(Input::new(&self.filter).small().cleanable(true)),
            )
            .when_some(load_error, |el, e| {
                el.child(Banner::error(
                    "run-load-error",
                    format!("Saved commands can't be read, so changes won't be saved. {e}"),
                ))
            })
            .when_some(notice, |el, n| {
                el.child(Banner::info("run-notice", n).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.store.update(cx, |s, cx| {
                            s.notice = None;
                            cx.notify();
                        });
                    },
                )))
            })
            .when_some(self.scan_error.clone(), |el, e| {
                el.child(Banner::error("run-scan-error", e))
            })
            .children(editor)
            .child(
                div()
                    .id("run-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows)
                    .when(!self.scanned, |el| {
                        el.child(Placeholder::loading("Looking for scripts…").compact())
                    })
                    .when(self.scanned && shown_scripts == 0, |el| {
                        el.child(if filtering {
                            Placeholder::new(IconName::Search, "No matching scripts")
                                .compact()
                                .action(
                                    Button::new("run-clear-filter")
                                        .small()
                                        .label("Clear Filter")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                )
                        } else {
                            Placeholder::new(Lucide::Package, "No scripts found")
                                .compact()
                                .detail("Nothing in package.json, deno.json, composer.json, a Makefile or Cargo.toml here. Save your own command with +.")
                        })
                    }),
            )
    }
}
