//! Drawing the dock: the tab strip, with a header for each project's group and each tab's label,
//! agents' logos, close button and menu, then the tab in front.

use std::path::PathBuf;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::{ContextMenuExt, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::terminal::{AgentKind, SplitDir};

use super::groups::slots;
use super::{RunningAgent, TerminalPanel, ToggleMaximize};
use crate::states::{Banner, Placeholder};
use crate::tab_groups::{group_header, group_stripe, project_name};
use crate::tabs::{Drawn, strip};
use crate::terminal::agents;
use crate::terminal::tab::TerminalTab;
use crate::terminal::view::TerminalView;
use crate::terminal::{RenameTab, SplitDown, SplitRight};

impl TerminalPanel {
    /// The header in front of the run of `root`'s tabs that starts at tab `first`, `count` long:
    /// a click folds the group.
    fn render_group_header(
        &self,
        first: usize,
        count: usize,
        root: PathBuf,
        cx: &Context<Self>,
    ) -> AnyElement {
        let color = self.group_color(&root, cx);
        let collapsed = self.collapsed.contains(&root);
        group_header(
            ("term-group", first),
            project_name(&root),
            color,
            collapsed,
            count,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            if !this.collapsed.remove(&root) {
                this.collapsed.insert(root.clone());
            }
            cx.notify();
        }))
        .into_any_element()
    }

    /// Every tab closed, or the first one failed to start.
    fn render_empty(&self, cx: &mut Context<Self>) -> Placeholder {
        let shell = Button::new("empty-new-terminal")
            .small()
            .icon(IconName::Plus)
            .label("New Terminal")
            .on_click(cx.listener(|this, _, window, cx| this.open_shell(Some(window), cx)));
        match &self.error {
            Some(e) => Placeholder::error("Unable to open a terminal")
                .detail(e.clone())
                .action(shell.label("Try Again")),
            None => Placeholder::new(Lucide::SquareTerminal, "No terminals open")
                .detail("Start a shell in the project folder.")
                .action(shell.primary()),
        }
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let weak = cx.entity().downgrade();
        // A header in front of each group's run of tabs; a folded group shows its active tab only.
        let groups = self.groups(cx);
        let drawn = strip(&slots(&groups), self.active, |g| self.collapsed.contains(g));
        let mut tabs: Vec<AnyElement> = Vec::with_capacity(drawn.len());
        // The active tab's place among the strip's children, which a scroll goes by.
        let mut active_child = None;
        // A pane on screen, where the tab menus look up the terminal's keys to show them.
        let keys_in = self
            .tabs
            .get(self.active)
            .map(|t| t.read(cx).focused_view().focus_handle(cx));
        let running = self.running_agents(cx);
        for item in drawn {
            let ix = match item {
                Drawn::Tab(ix) => ix,
                Drawn::Header { first, count } => {
                    if let Some(root) = groups[first].clone() {
                        tabs.push(self.render_group_header(first, count, root, cx));
                    }
                    continue;
                }
            };
            let tab = &self.tabs[ix];
            let t = tab.read(cx);
            let v = t.focused_view().read(cx);
            let group = t.group.clone();
            let color = group.as_ref().map(|g| self.group_color(g, cx));
            let active = ix == self.active;
            if active {
                active_child = Some(tabs.len());
            }
            let others: Vec<PathBuf> = self
                .projects
                .iter()
                .filter(|p| Some(*p) != group.as_ref())
                .cloned()
                .collect();
            let weak = weak.clone();
            let keys_in = keys_in.clone();
            let this_tab = tab.downgrade();
            let grouped = group.is_some();
            // The tab is renamed in its label; so is a one-pane tab's pane
            // that was being renamed when the tab's other panes closed.
            let rename = t.tab_rename_field(tab.downgrade()).or_else(|| {
                (!t.state.is_split())
                    .then(|| t.rename_field(tab.downgrade(), t.state.focused))
                    .flatten()
            });
            let named = t.state.name.as_deref().or(t.pane_name());
            tabs.push({
                // SSH tabs keep the server's name (the remote shell's title
                // says little across servers), numbered when it is open twice.
                // A name the user gave the tab or its pane goes over either.
                let (icon, label) = match &v.remote {
                    Some(r) => {
                        let same = |w: &Entity<TerminalTab>| {
                            focused(w, cx)
                                .remote
                                .as_ref()
                                .is_some_and(|o| o.key == r.key)
                        };
                        let nth = self.tabs[..ix].iter().filter(|w| same(w)).count();
                        let name = if let Some(named) = named {
                            named.to_string()
                        } else if nth > 0 || self.tabs[ix + 1..].iter().any(same) {
                            format!("{} {}", r.name, nth + 1)
                        } else {
                            r.name.clone()
                        };
                        let state = if v.exited { " (disconnected)" } else { "" };
                        (Lucide::Server, format!("{name}{state}"))
                    }
                    None if v.job.is_some() => {
                        let name = t.label(cx);
                        let state = if v.exited {
                            " (exited)"
                        } else if v.foreground_job().is_none() {
                            " (stopped)"
                        } else {
                            ""
                        };
                        (Lucide::Play, format!("{name}{state}"))
                    }
                    None if v.exited => (Lucide::Terminal, format!("{} (exited)", t.title(cx))),
                    None => (Lucide::Terminal, t.title(cx).to_string()),
                };
                // A live SSH session or a running script: the glyph turns the success colour,
                // and over agents' logos (which keep their own colours) a dot in it says so.
                let live = (v.remote.is_some() && !v.exited)
                    || (v.job.is_some() && v.foreground_job().is_some());
                let on_tab: Vec<&RunningAgent> = running
                    .iter()
                    .filter(|r| r.tab.entity_id() == tab.entity_id())
                    .collect();
                let kinds: Vec<AgentKind> = on_tab.iter().map(|r| r.kind).collect();
                let running_phrase = (!kinds.is_empty()).then(|| agents::running_phrase(&kinds));
                let icon = match &running_phrase {
                    None => Icon::new(icon)
                        .xsmall()
                        .when(live, |i| i.text_color(cx.theme().success))
                        .into_any_element(),
                    Some(phrase) => {
                        // The tooltip names each pane's agent, by its place in reading order.
                        let mut lines = vec![phrase.clone()];
                        if t.state.is_split() {
                            let order = t.state.reading_order(t.area());
                            lines.extend(on_tab.iter().map(|r| {
                                let n = order.iter().position(|p| *p == r.pane).unwrap_or(0) + 1;
                                format!("Pane {n}: {}", r.kind.name())
                            }));
                        }
                        let badge = agents::badge(kinds.iter().copied());
                        let ring = if active {
                            cx.theme().tab_active
                        } else {
                            cx.theme().tab_bar
                        };
                        agents::logos(&badge, &format!("term-agent-{ix}"), window, cx)
                            .id(("term-agents", ix))
                            .debug_selector(move || format!("term-agents-{ix}"))
                            .relative()
                            .role(Role::Image)
                            .aria_label(phrase.clone())
                            .tooltip(move |window, cx| agents::tooltip(lines.clone(), window, cx))
                            .when(live, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .right(px(-2.))
                                        .bottom(px(-2.))
                                        .size(px(6.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(ring)
                                        .bg(cx.theme().success),
                                )
                            })
                            .into_any_element()
                    }
                };
                h_flex()
                    .id(("term-tab", ix))
                    .role(Role::Tab)
                    .aria_label(label.clone())
                    .when_some(running_phrase, |el, phrase| el.aria_description(phrase))
                    .aria_selected(active)
                    .debug_selector(move || format!("term-tab-{ix}"))
                    .relative()
                    .flex_none()
                    .h_full()
                    .px_2()
                    .gap_1()
                    .text_xs()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| el.text_color(cx.theme().tab_foreground))
                    .child(icon)
                    .map(|el| match rename {
                        Some(field) => el.child(field),
                        None => el.child(label),
                    })
                    .child(
                        Button::new(("close-term", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .accessibility_label("Close Terminal")
                            .debug_selector(move || format!("close-term-{ix}"))
                            .on_click(cx.listener({
                                let tab = this_tab.clone();
                                move |this, _, window, cx| {
                                    // The tab under the × would activate itself next.
                                    cx.stop_propagation();
                                    let had_keyboard = tab
                                        .upgrade()
                                        .is_some_and(|t| t.read(cx).has_focus(window, cx));
                                    this.close_for_user(&tab, had_keyboard, window, cx)
                                }
                            })),
                    )
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener({
                        let tab = this_tab.clone();
                        move |this, e: &ClickEvent, window, cx| {
                            if let Some(ix) = this.index_of(&tab) {
                                this.activate(ix, window, cx);
                                // A double-click renames the tab.
                                if e.click_count() >= 2 {
                                    this.rename_tab(ix, window, cx);
                                }
                            }
                        }
                    }))
                    .context_menu(move |menu, window, cx| {
                        // The menu has the keyboard from here on: whether the tab had it
                        // before decides where it goes when the tab closes.
                        let had_keyboard = this_tab
                            .upgrade()
                            .is_some_and(|t| t.read(cx).has_focus(window, cx));
                        let rename_weak = weak.clone();
                        let rename_tab = this_tab.clone();
                        let move_weak = weak.clone();
                        let move_tab = this_tab.clone();
                        let others = others.clone();
                        // Its keys are the terminal's: they show as a pane on screen has them.
                        let menu = match &keys_in {
                            Some(pane) => menu.action_context(pane.clone()),
                            None => menu,
                        }
                        .item(
                            PopupMenuItem::new("Rename Tab")
                                .action(Box::new(RenameTab))
                                .on_click(move |_, window, cx| {
                                    let _ = rename_weak.update(cx, |p, cx| {
                                        if let Some(ix) = p.index_of(&rename_tab) {
                                            p.rename_tab(ix, window, cx)
                                        }
                                    });
                                }),
                        );
                        let menu = menu.when(!others.is_empty(), |menu| {
                            menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
                                for root in &others {
                                    let weak = move_weak.clone();
                                    let tab = move_tab.clone();
                                    let to = root.clone();
                                    menu =
                                        menu.item(PopupMenuItem::new(project_name(root)).on_click(
                                            move |_, _, cx| {
                                                let _ = weak.update(cx, |t, cx| {
                                                    t.move_to_group(&tab, Some(to.clone()), cx)
                                                });
                                            },
                                        ));
                                }
                                menu
                            })
                        });
                        let ungroup = weak.clone();
                        let ungroup_tab = this_tab.clone();
                        let close = weak.clone();
                        let close_tab = this_tab.clone();
                        menu.when(grouped, |menu| {
                            menu.item(PopupMenuItem::new("Remove from Group").on_click(
                                move |_, _, cx| {
                                    let _ = ungroup.update(cx, |t, cx| {
                                        t.move_to_group(&ungroup_tab, None, cx)
                                    });
                                },
                            ))
                        })
                        .separator()
                        .item(PopupMenuItem::new("Close").on_click(move |_, window, cx| {
                            let _ = close.update(cx, |t, cx| {
                                t.close_for_user(&close_tab, had_keyboard, window, cx)
                            });
                        }))
                    })
                    .into_any_element()
            });
        }
        // Bring a newly opened or activated tab into view, by its place among the strip's children:
        // the group headers and the folded tabs make that differ from its index (SMOKE-2).
        let key = self
            .tabs
            .get(self.active)
            .map(|t| (t.entity_id(), self.active));
        if let (Some(key), Some(child)) = (key, active_child)
            && self.scrolled_to != Some(key)
        {
            self.tab_scroll.scroll_to_item(child);
            self.scrolled_to = Some(key);
        }
        let (max_icon, max_tip) = if self.maximized {
            (Lucide::Minimize, "Restore Layout")
        } else {
            (Lucide::Maximize, "Maximize Terminal")
        };

        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                this.split_active(SplitDir::Right, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitDown, window, cx| {
                this.split_active(SplitDir::Down, window, cx)
            }))
            .bg(cx.theme().background)
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(28.))
                    .bg(cx.theme().tab_bar)
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .id("term-tabs")
                            .role(Role::TabList)
                            .aria_label("Terminals")
                            .debug_selector(|| "term-tabs".into())
                            .h_full()
                            .flex_1()
                            .overflow_x_scroll()
                            .track_scroll(&self.tab_scroll)
                            .children(tabs),
                    )
                    .child(
                        h_flex()
                            .px_1()
                            .gap_1()
                            .child(
                                Button::new("new-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .accessibility_label("New Terminal")
                                    .tooltip("New Terminal")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_shell(Some(window), cx)
                                    })),
                            )
                            .child(
                                Button::new("maximize-terminal")
                                    .ghost()
                                    .xsmall()
                                    .icon(max_icon)
                                    .accessibility_label(max_tip)
                                    .tooltip(max_tip)
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ToggleMaximize))),
                            ),
                    ),
            )
            .when(!self.tabs.is_empty(), |el| {
                el.when_some(self.error.clone(), |el, e| {
                    el.child(Banner::error("term-error", e).on_dismiss(cx.listener(
                        |this, _, _, cx| {
                            this.error = None;
                            cx.notify();
                        },
                    )))
                })
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .map(|el| match self.tabs.get(self.active).cloned() {
                        Some(tab) => el.child(tab),
                        None => el.child(self.render_empty(cx)),
                    }),
            )
    }
}

/// The terminal of `tab`'s focused pane.
fn focused<'a>(tab: &Entity<TerminalTab>, cx: &'a App) -> &'a TerminalView {
    tab.read(cx).focused_view().read(cx)
}
