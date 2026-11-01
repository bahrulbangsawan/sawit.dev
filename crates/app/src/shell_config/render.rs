//! The Config Files sidebar drawn: its header and search box, the groups and their rows, a row's
//! context menu, and the actions the list's keys run.

use std::path::Path;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenu};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Group, OpenConfig, Row, ShellConfigEvent, ShellConfigPanel};
use crate::files::{
    CopyPath, Delete, Duplicate, OpenInDefaultApp, OpenInTerminal, RevealInFinder, Trash,
    TrashWithoutAsking,
};
use crate::states::{Banner, Placeholder};

impl ShellConfigPanel {
    fn render_section(&self, title: &'static str, cx: &mut Context<Self>) -> AnyElement {
        div()
            .w_full()
            .h(px(26.))
            .mt_2()
            .px_2()
            .flex()
            .items_center()
            .text_xs()
            .font_semibold()
            .text_color(cx.theme().muted_foreground)
            .child(title)
            .into_any_element()
    }

    fn render_group(
        &self,
        group: Group,
        count: usize,
        open: bool,
        filtering: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let title = group.title();
        let (id, logo): (SharedString, AnyElement) = match &group {
            Group::Shell => (
                "config-group-shell".into(),
                Icon::new(Lucide::SquareTerminal)
                    .small()
                    .text_color(muted)
                    .into_any_element(),
            ),
            Group::Agent(client, _) => (
                format!("config-group-{title}").into(),
                crate::mcp_page::client_logo(*client, 16., cx),
            ),
        };
        h_flex()
            .id(id)
            .w_full()
            .h(px(28.))
            .px_2()
            .gap_1p5()
            .when(!filtering, |el| {
                el.cursor_pointer()
                    .hover(|el| el.bg(cx.theme().list_hover))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(group.clone(), cx)))
            })
            .child(
                Icon::new(if open {
                    Lucide::ChevronDown
                } else {
                    Lucide::ChevronRight
                })
                .xsmall()
                .text_color(muted),
            )
            .child(logo)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_sm()
                    .font_medium()
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .text_xs()
                    .text_color(muted)
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_row(&self, ix: usize, row: &Row, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let path = row.path.clone();
        let selected = self.selected.as_ref() == Some(&row.path);
        let focus = self.focus.clone();
        let writable = row.writable;
        // Both lines truncate; the tooltip has them whole.
        let full: SharedString = format!("{}\n{}", row.path.display(), row.detail).into();
        h_flex()
            .id(("config-row", ix))
            .w_full()
            .h(px(40.))
            // In line with the group's title, past its chevron.
            .pl(px(26.))
            .pr_2()
            .gap_2()
            .cursor_pointer()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
            .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
            .on_click(cx.listener({
                let path = path.clone();
                move |this, _, window, cx| {
                    this.selected = Some(path.clone());
                    window.focus(&this.focus, cx);
                    cx.emit(ShellConfigEvent::OpenFile(path.clone()))
                }
            }))
            // The menu's commands act on the row clicked.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, _, _, cx| {
                    this.selected = Some(path.clone());
                    cx.notify();
                }),
            )
            .context_menu(move |m, _, _| config_menu(m, focus.clone(), writable))
            // One width for both kinds of icon, so names line up.
            .child(
                div()
                    .w(px(16.))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(match row.icon {
                        Some(icon) => Icon::new(icon).small().text_color(muted).into_any_element(),
                        None => crate::file_icons::file_icon(&row.path, cx),
                    }),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .truncate()
                            .when(row.dim, |el| el.text_color(muted))
                            .child(row.name.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .truncate()
                            .child(row.detail.clone()),
                    ),
            )
            .when(!row.writable, |el| {
                el.child(
                    div()
                        .id(("config-lock", ix))
                        .flex_none()
                        .child(Icon::new(Lucide::Lock).xsmall().text_color(muted))
                        .tooltip(|window, cx| {
                            Tooltip::new("Saving asks for your admin password").build(window, cx)
                        }),
                )
            })
            .into_any_element()
    }
}

fn config_menu(menu: PopupMenu, focus: FocusHandle, writable: bool) -> PopupMenu {
    menu.action_context(focus)
        .menu("Open", Box::new(OpenConfig))
        .separator()
        .menu("Reveal in Finder", Box::new(RevealInFinder))
        .menu("Open in Default App", Box::new(OpenInDefaultApp))
        .menu("Open in Terminal", Box::new(OpenInTerminal))
        .separator()
        .menu_with_disabled("Duplicate", Box::new(Duplicate), !writable)
        .menu("Copy Path", Box::new(CopyPath))
        .separator()
        .menu_with_disabled("Trash", Box::new(Trash), !writable)
        .menu_with_disabled("Delete", Box::new(Delete), !writable)
}

impl Render for ShellConfigPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.filter.read(cx).value().trim().to_lowercase();
        let filtering = !query.is_empty();
        let groups = self.groups();
        let total: usize = groups.iter().map(|(_, rows)| rows.len()).sum();
        let mut items = Vec::new();
        let mut ix = 0;
        let mut agents_titled = false;
        for (group, rows) in &groups {
            let title = group.title();
            // Searching a group's name lists all of its files.
            let rows: Vec<&Row> = if title.to_lowercase().contains(&query) {
                rows.iter().collect()
            } else {
                rows.iter().filter(|r| r.matches(&query)).collect()
            };
            if rows.is_empty() {
                continue;
            }
            if matches!(group, Group::Agent(..)) && !agents_titled {
                agents_titled = true;
                items.push(self.render_section("AGENTS · HIGHEST PRIORITY FIRST", cx));
            }
            let open = filtering || !self.collapsed.contains(group);
            items.push(self.render_group(group.clone(), rows.len(), open, filtering, cx));
            if open {
                for row in rows {
                    items.push(self.render_row(ix, row, cx));
                    ix += 1;
                }
            }
        }
        let empty = items.is_empty();
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .key_context("ConfigFilesPanel")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &OpenConfig, _, cx| {
                if let Some(path) = this.selected.clone() {
                    cx.emit(ShellConfigEvent::OpenFile(path));
                }
            }))
            .on_action(cx.listener(|this, _: &RevealInFinder, _, cx| {
                if let Some(path) = &this.selected {
                    cx.reveal_path(path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInDefaultApp, _, cx| {
                if let Some(path) = &this.selected {
                    cx.open_with_system(path);
                }
            }))
            .on_action(cx.listener(|this, _: &OpenInTerminal, _, cx| {
                if let Some(dir) = this.selected.as_deref().and_then(Path::parent) {
                    cx.emit(ShellConfigEvent::OpenTerminal(dir.to_path_buf()));
                }
            }))
            .on_action(cx.listener(|this, _: &Duplicate, _, cx| this.duplicate(cx)))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| {
                if let Some(path) = &this.selected {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        path.to_string_lossy().into_owned(),
                    ));
                }
            }))
            .on_action(cx.listener(|this, _: &Trash, w, cx| this.trash(true, w, cx)))
            .on_action(cx.listener(|this, _: &TrashWithoutAsking, w, cx| {
                this.trash(false, w, cx)
            }))
            .on_action(cx.listener(|this, _: &Delete, w, cx| this.delete(w, cx)))
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
                            .child(format!("Config Files · {total}")),
                    )
                    .child(
                        Button::new("shell-config-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Look Again")
                            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx))),
                    ),
            )
            .child(
                div().p_2().border_b_1().border_color(cx.theme().border).child(
                    Input::new(&self.filter)
                        .small()
                        .prefix(
                            Icon::new(Lucide::Search)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                        .cleanable(true),
                ),
            )
            .when_some(self.error.clone(), |el, e| {
                el.child(Banner::error("shell-config-error", e).on_dismiss(cx.listener(
                    |this, _, _, cx| {
                        this.error = None;
                        cx.notify();
                    },
                )))
            })
            .child(
                div()
                    .id("shell-config-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pb_2()
                    .when(!empty, |el| el.children(items))
                    .when(empty && filtering, |el| {
                        el.child(
                            Placeholder::new(Lucide::Search, "No matching files")
                                .compact()
                                .detail("Nothing matches this name, path or description.")
                                .action(
                                    Button::new("clear-config-filter")
                                        .small()
                                        .label("Clear Search")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.filter
                                                .update(cx, |s, cx| s.set_value("", window, cx));
                                        })),
                                ),
                        )
                    })
                    .when(empty && !filtering, |el| {
                        el.child(
                            Placeholder::new(Lucide::FileTerminal, "No config files")
                                .compact()
                                .detail("Files like ~/.zshrc, ~/.claude/settings.json and ~/.codex/config.toml show up here once they exist."),
                        )
                    }),
            )
            .child(self.render_footer(cx))
    }
}
