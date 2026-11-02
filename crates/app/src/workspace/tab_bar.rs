//! The tab bar: a header in front of each project's group, each tab with its icon, title and close or
//! pin button, and a tab's menu.

use super::*;

impl Workspace {
    /// The header in front of the run of `root`'s tabs that starts at tab `first`, `count` long:
    /// a click folds the group, and its menu closes the group's tabs.
    fn render_group_header(
        &self,
        first: usize,
        count: usize,
        root: PathBuf,
        cx: &Context<Self>,
    ) -> AnyElement {
        let weak = cx.entity().downgrade();
        let color = self.group_color(&root, cx);
        let collapsed = self.collapsed.contains(&root);
        let (toggle, close) = (root.clone(), root.clone());
        group_header(
            ("tab-group", first),
            project_name(&root),
            color,
            collapsed,
            count,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_group(toggle.clone(), cx)))
        .context_menu(move |menu, _, _| {
            let weak = weak.clone();
            let close = close.clone();
            menu.item(
                PopupMenuItem::new("Close Tabs in Group").on_click(move |_, _, cx| {
                    let _ = weak.update(cx, |ws, cx| ws.close_group(&close, cx));
                }),
            )
        })
        .into_any_element()
    }

    pub(super) fn render_tab_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let weak = cx.entity().downgrade();
        let projects = self.roots(cx);
        // A header in front of each group's run of tabs; a folded group shows its active tab only.
        let drawn = strip(&self.slots(), self.active, |g| self.collapsed.contains(g));
        let mut items: Vec<AnyElement> = Vec::with_capacity(drawn.len());
        // The active tab's place among the bar's children, which a scroll goes by.
        let mut active_child = None;
        for item in drawn {
            let ix = match item {
                Drawn::Tab(ix) => ix,
                Drawn::Header { first, count } => {
                    if let Some(root) = self.tabs[first].group.clone() {
                        items.push(self.render_group_header(first, count, root, cx));
                    }
                    continue;
                }
            };
            let tab = &self.tabs[ix];
            let group = tab.group.clone().filter(|_| !tab.pinned);
            let color = group.as_ref().map(|g| self.group_color(g, cx));
            let active = ix == self.active;
            if active {
                active_child = Some(items.len());
            }
            let projects = projects.clone();
            let tab_group = tab.group.clone();
            items.push({
                let pinned = tab.pinned;
                let glyph = |icon: Lucide| Icon::new(icon).xsmall().into_any_element();
                let (icon, title, dirty, read_only) = match &tab.item {
                    PaneItem::Server => {
                        (glyph(Lucide::Server), "Instance".to_string(), false, None)
                    }
                    PaneItem::File(e) => {
                        let e = e.read(cx);
                        (
                            crate::file_icons::file_icon(&e.path, cx),
                            e.title(),
                            e.dirty,
                            Some(e.read_only),
                        )
                    }
                    PaneItem::Other {
                        title, icon, file, ..
                    } => (
                        // The file's logo, else the tab's own icon (a canvas file has no logo).
                        match file {
                            Some(path) if crate::file_icons::logo_for(path).is_some() => {
                                crate::file_icons::file_icon(path, cx)
                            }
                            _ => glyph(*icon),
                        },
                        title.to_string(),
                        false,
                        None,
                    ),
                };
                // A canvas with agents at work on it: a dot in the colour of the latest.
                let agents = match &tab.item {
                    PaneItem::Other { view, .. } => (view.clone().downcast::<BoardView>().ok())
                        .and_then(|board| board.read(cx).agents_at_work(cx)),
                    _ => None,
                };
                let label: SharedString = match agents {
                    Some(_) => format!("{title}, agents at work").into(),
                    None => title.clone().into(),
                };
                let weak = weak.clone();
                h_flex()
                    .id(("pane-tab", ix))
                    .role(Role::Tab)
                    .aria_label(label)
                    .aria_selected(active)
                    .relative()
                    .flex_none()
                    .h_full()
                    .pl_3()
                    .pr_1()
                    .gap_1p5()
                    .text_sm()
                    .cursor_pointer()
                    .border_r_1()
                    .border_color(cx.theme().border)
                    .when(active, |el| {
                        el.bg(cx.theme().tab_active)
                            .text_color(cx.theme().tab_active_foreground)
                    })
                    .when(!active, |el| {
                        el.bg(cx.theme().tab).text_color(cx.theme().tab_foreground)
                    })
                    .child(icon)
                    .child(title)
                    .when(read_only == Some(true), |el| {
                        el.child(
                            Icon::new(Lucide::Lock)
                                .xsmall()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .when(dirty, |el| {
                        el.child(div().size(px(6.)).rounded_full().bg(cx.theme().foreground))
                    })
                    .when_some(agents, |el, colour| {
                        el.child(
                            div()
                                .debug_selector(|| "tab-agents-at-work".into())
                                .size(px(7.))
                                .rounded_full()
                                .border_1()
                                .border_color(gpui_kit::white())
                                .bg(colour),
                        )
                    })
                    .child(if pinned {
                        Button::new(("pin-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Pin)
                            .accessibility_label("Unpin")
                            .tooltip("Unpin")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle_pin(ix, cx);
                            }))
                    } else {
                        Button::new(("close-tab", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::X)
                            .accessibility_label("Close Tab")
                            .tooltip_with_action("Close", &CloseActiveTab, Some("Workspace"))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close(CloseOp::This, ix, window, cx);
                            }))
                    })
                    .when_some(color, |el, color| el.child(group_stripe(color, active)))
                    .on_click(cx.listener(move |this, _, _, cx| this.show_tab(ix, cx)))
                    .context_menu(move |menu, window, cx| {
                        Self::tab_menu(
                            weak.clone(),
                            ix,
                            read_only.is_some(),
                            read_only == Some(true),
                            pinned,
                            tab_group.clone(),
                            projects.clone(),
                            menu,
                            window,
                            cx,
                        )
                    })
                    .into_any_element()
            });
        }
        // Bring a newly opened or activated tab into view, by its place among the bar's children:
        // the group headers and the folded tabs make that differ from its index (SMOKE-1).
        let key = self.tab_view(self.active).map(|view| (view, self.active));
        if let (Some(key), Some(child)) = (key, active_child)
            && self.scrolled_to != Some(key)
        {
            self.tab_scroll.scroll_to_item(child);
            self.scrolled_to = Some(key);
        }
        h_flex()
            .id("tab-bar")
            .role(Role::TabList)
            .aria_label("Tabs")
            .h(px(32.))
            .flex_none()
            .bg(cx.theme().tab_bar)
            .border_b_1()
            .border_color(cx.theme().border)
            .overflow_x_scroll()
            .track_scroll(&self.tab_scroll)
            .children(items)
    }

    #[allow(clippy::too_many_arguments)]
    fn tab_menu(
        this: WeakEntity<Self>,
        ix: usize,
        is_file: bool,
        read_only: bool,
        pinned: bool,
        group: Option<PathBuf>,
        projects: Vec<PathBuf>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let muted = cx.theme().muted_foreground;
        let keys = |action: &dyn Action| crate::keymap::keys_for(action, "Workspace", window);
        let [
            close_keys,
            others_keys,
            left_keys,
            right_keys,
            clean_keys,
            all_keys,
            pin_keys,
        ] = [
            &CloseActiveTab as &dyn Action,
            &CloseOtherTabs,
            &CloseTabsLeft,
            &CloseTabsRight,
            &CloseCleanTabs,
            &CloseAllTabs,
            &TogglePinTab,
        ]
        .map(keys);
        let item = |label: &'static str, keys: SharedString| {
            PopupMenuItem::element(move |_, _| {
                h_flex()
                    .w_full()
                    .gap_6()
                    .justify_between()
                    .child(label)
                    .child(div().text_color(muted).child(keys.clone()))
            })
        };
        let close = |op: CloseOp| {
            let this = this.clone();
            move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |ws, cx| ws.close(op, ix, window, cx));
            }
        };
        let this_pin = this.clone();
        let this_ro = this.clone();
        let menu = Self::group_menu(this.clone(), ix, group, projects, menu, window, cx);
        menu.item(item("Close", close_keys).on_click(close(CloseOp::This)))
            .item(item("Close Others", others_keys).on_click(close(CloseOp::Others)))
            .separator()
            .item(item("Close Left", left_keys).on_click(close(CloseOp::Left)))
            .item(item("Close Right", right_keys).on_click(close(CloseOp::Right)))
            .separator()
            .item(item("Close Clean", clean_keys).on_click(close(CloseOp::Clean)))
            .item(item("Close All", all_keys).on_click(close(CloseOp::All)))
            .when(is_file, |menu| {
                menu.separator().item(
                    PopupMenuItem::new(if read_only {
                        "Make Tab Editable"
                    } else {
                        "Make Tab Read-Only"
                    })
                    .on_click(move |_, _, cx| {
                        let _ = this_ro.update(cx, |ws, cx| ws.toggle_read_only(ix, cx));
                    }),
                )
            })
            .separator()
            .item(
                item(if pinned { "Unpin Tab" } else { "Pin Tab" }, pin_keys).on_click(
                    move |_, _, cx| {
                        let _ = this_pin.update(cx, |ws, cx| ws.toggle_pin(ix, cx));
                    },
                ),
            )
    }

    /// "Move to Project" and "Remove from Group", for a tab's menu.
    fn group_menu(
        this: WeakEntity<Self>,
        ix: usize,
        group: Option<PathBuf>,
        projects: Vec<PathBuf>,
        menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
    ) -> PopupMenu {
        let others: Vec<PathBuf> = projects
            .into_iter()
            .filter(|p| Some(p) != group.as_ref())
            .collect();
        if others.is_empty() && group.is_none() {
            return menu;
        }
        let this_move = this.clone();
        menu.when(!others.is_empty(), |menu| {
            menu.submenu("Move to Project", window, cx, move |mut menu, _, _| {
                for root in &others {
                    let this = this_move.clone();
                    let to = root.clone();
                    menu = menu.item(PopupMenuItem::new(project_name(root)).on_click(
                        move |_, _, cx| {
                            let _ = this
                                .update(cx, |ws, cx| ws.move_to_group(ix, Some(to.clone()), cx));
                        },
                    ));
                }
                menu
            })
        })
        .when(group.is_some(), |menu| {
            menu.item(
                PopupMenuItem::new("Remove from Group").on_click(move |_, _, cx| {
                    let _ = this.update(cx, |ws, cx| ws.move_to_group(ix, None, cx));
                }),
            )
        })
        .separator()
    }
}
