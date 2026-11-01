//! One line of the connection tree as the virtual list draws it: a connection with its status
//! and buttons, a section, a schema, a kind, a relation, a column, a group or an index or
//! constraint.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::menu::ContextMenuExt;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::db::DbKind;

use super::jobs::Target;
use super::menus::{profile_menu, relation_menu, schema_menu};
use super::tree::{Facet, Node, kind_icon, kind_label};
use super::{DatabasePanel, DbEvent};
use crate::db::logos;

/// Height of every tree line (the list is virtual, so lines are uniform).
const ROW_HEIGHT: f32 = 22.;

fn row(id: impl Into<ElementId>, depth: usize, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .h(px(ROW_HEIGHT))
        .pl(px(8. + depth as f32 * 12.))
        .pr_1()
        .gap_1p5()
        .text_sm()
        .cursor_pointer()
        .hover(|el| el.bg(cx.theme().list_hover))
}

impl DatabasePanel {
    pub(super) fn render_node(&self, ix: usize, node: &Node, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match node {
            Node::Profile {
                name,
                label,
                detail,
                url,
                kind,
                accessible,
                saved,
                route,
                connected,
                pending,
                status,
            } => {
                let color = if *connected {
                    cx.theme().success
                } else if *pending {
                    cx.theme().warning
                } else {
                    muted
                };
                let (n1, n2, n3, n4) = (name.clone(), name.clone(), name.clone(), name.clone());
                let n5 = name.clone();
                // The kind and name, where it points and where changes go: the logo is never
                // the only way to tell the kind.
                let mut lines = vec![accessible.clone()];
                if let Some(detail) = detail {
                    lines.push(format!("database {detail}"));
                }
                lines.push(url.clone());
                lines.extend(route.clone());
                let tip: SharedString = lines.join("\n").into();
                let depth = if *saved { 0 } else { 1 };
                row(("db-profile", ix), depth, cx)
                    .role(Role::TreeItem)
                    .aria_label(accessible.clone())
                    .child(div().size(px(7.)).flex_none().rounded_full().bg(color))
                    .child(logos::logo(*kind, 14.))
                    .child(
                        h_flex()
                            .id(("db-profile-name", ix))
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().truncate().child(label.clone()))
                            .when_some(detail.clone(), |el, d| {
                                el.child(div().text_xs().text_color(muted).truncate().child(d))
                            })
                            .when(*kind == DbKind::D1Remote, |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .px_1()
                                        .rounded(cx.theme().radius)
                                        .text_xs()
                                        .bg(cx.theme().warning)
                                        .text_color(cx.theme().warning_foreground)
                                        .child("remote"),
                                )
                            })
                            .tooltip(move |window, cx| {
                                gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                    .build(window, cx)
                            }),
                    )
                    .when_some(status.clone(), |el, status| {
                        el.child(div().text_xs().text_color(muted).child(status))
                    })
                    .child(if *connected || *pending {
                        Button::new(("db-disconnect", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Unplug)
                            .tooltip(if *pending {
                                "Stop Connecting"
                            } else {
                                "Disconnect"
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.disconnect(&n5, cx));
                            }))
                    } else {
                        Button::new(("db-connect", ix))
                            .ghost()
                            .xsmall()
                            .icon(Lucide::Plug)
                            .tooltip("Connect")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.store.update(cx, |s, cx| s.connect(&n5, cx));
                                this.expanded.insert(n5.clone());
                                cx.notify();
                            }))
                    })
                    .when(*connected, |el| {
                        el.child(
                            Button::new(("db-sql", ix))
                                .ghost()
                                .xsmall()
                                .label("SQL")
                                .tooltip("New SQL Editor")
                                .on_click(cx.listener(move |_, _, _, cx| {
                                    cx.stop_propagation();
                                    cx.emit(DbEvent::OpenSql {
                                        profile: n1.clone(),
                                        sql: None,
                                    });
                                })),
                        )
                        .child(
                            Button::new(("db-refresh", ix))
                                .ghost()
                                .xsmall()
                                .icon(IconName::RotateCw)
                                .tooltip("Reload Schema")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.store.update(cx, |s, cx| s.refresh(&n2, cx)).detach();
                                })),
                        )
                    })
                    .when(*saved, |el| {
                        el.child(
                            Button::new(("db-settings", ix))
                                .ghost()
                                .xsmall()
                                .icon(Lucide::Settings2)
                                .tooltip("Connection Settings")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    this.open_settings(&n3, window, cx);
                                })),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.store.read(cx).is_connected(&n4) {
                            this.toggle(n4.clone(), cx);
                        } else {
                            this.store.update(cx, |s, cx| s.connect(&n4, cx));
                            this.expanded.insert(n4.clone());
                            cx.notify();
                        }
                    }))
                    .context_menu({
                        let (weak, name, connected, kind, saved) = (
                            cx.entity().downgrade(),
                            name.clone(),
                            *connected,
                            *kind,
                            *saved,
                        );
                        move |menu, _, _| {
                            profile_menu(menu, weak.clone(), name.clone(), connected, kind, saved)
                        }
                    })
                    .into_any_element()
            }
            Node::Error { message } => {
                let full: SharedString = message.clone().into();
                // One line (the list is uniform); the whole text is in the tooltip.
                div()
                    .id(("db-error", ix))
                    .h(px(ROW_HEIGHT))
                    .pl(px(20.))
                    .pr_2()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .truncate()
                    .child(message.lines().next().unwrap_or_default().to_string())
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .into_any_element()
            }
            Node::Section {
                key,
                title,
                count,
                open,
                cloudflare,
            } => {
                let k = key.clone();
                row(
                    ElementId::Name(format!("db-section\u{0}{key}").into()),
                    0,
                    cx,
                )
                .mt_1()
                .text_color(muted)
                .role(Role::TreeItem)
                .aria_label(format!("{title}, {count}"))
                .child(
                    Icon::new(if *open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .xsmall(),
                )
                .when(*cloudflare, |el| el.child(logos::cloudflare(14.)))
                .child(div().text_xs().font_semibold().child(title.clone()))
                .child(div().text_xs().child(count.to_string()))
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                .into_any_element()
            }
            Node::Schema {
                key,
                profile,
                name,
                open,
                count,
                db: kind,
            } => {
                let k = key.clone();
                let node_profile = profile.clone();
                let (profile, schema) = (profile.clone(), name.clone());
                row(
                    ElementId::Name(format!("db-schema\u{0}{key}").into()),
                    1,
                    cx,
                )
                .child(
                    Icon::new(if *open {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    })
                    .xsmall(),
                )
                .child(div().truncate().child(name.clone()))
                .child(div().text_xs().text_color(muted).child(count.to_string()))
                .child(div().flex_1())
                .child(
                    Button::new(ElementId::Name(format!("db-diagram\u{0}{key}").into()))
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Waypoints)
                        .tooltip("Show Schema Diagram")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.stop_propagation();
                            cx.emit(DbEvent::OpenDiagram {
                                profile: profile.clone(),
                                schema: schema.clone(),
                            });
                        })),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                .context_menu({
                    let weak = cx.entity().downgrade();
                    let (profile, schema, kind) = (node_profile.clone(), name.clone(), *kind);
                    move |menu, _, _| {
                        schema_menu(menu, weak.clone(), profile.clone(), schema.clone(), kind)
                    }
                })
                .into_any_element()
            }
            Node::Kind {
                key,
                kind,
                count,
                open,
            } => {
                let k = key.clone();
                row(ElementId::Name(format!("db-kind\u{0}{key}").into()), 2, cx)
                    .text_color(muted)
                    .child(
                        Icon::new(if *open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall(),
                    )
                    .child(div().text_xs().font_semibold().child(kind_label(*kind)))
                    .child(div().text_xs().child(count.to_string()))
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                    .into_any_element()
            }
            Node::Relation {
                key,
                profile,
                schema,
                name,
                kind,
                open,
                readable,
                db: db_kind,
            } => {
                let id = |part: &str| ElementId::Name(format!("{part}\u{0}{key}").into());
                let icon = Icon::new(kind_icon(*kind));
                if !readable {
                    // Disabled: nothing to open, the tooltip says why.
                    return h_flex()
                        .id(id("db-rel"))
                        .w_full()
                        .h(px(ROW_HEIGHT))
                        .pl(px(8. + 3. * 12.))
                        .pr_1()
                        .gap_1p5()
                        .text_sm()
                        .text_color(muted)
                        .opacity(0.6)
                        .cursor_not_allowed()
                        .child(div().w(px(16.)))
                        .child(icon.xsmall())
                        .child(div().flex_1().truncate().child(name.clone()))
                        .child(Icon::new(Lucide::Lock).xsmall())
                        .tooltip(|window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(
                                "No SELECT permission for this role",
                            )
                            .build(window, cx)
                        })
                        .into_any_element();
                }
                let toggle_key = key.clone();
                let target = Target {
                    profile: profile.clone(),
                    schema: schema.clone(),
                    name: name.clone(),
                    kind: *kind,
                    db: *db_kind,
                };
                let (profile, schema, table) = (profile.clone(), schema.clone(), name.clone());
                row(id("db-rel"), 3, cx)
                    .child(
                        Button::new(id("db-rel-toggle"))
                            .ghost()
                            .xsmall()
                            .icon(if *open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.toggle(toggle_key.clone(), cx);
                            })),
                    )
                    .child(icon.xsmall().text_color(muted))
                    .child(div().truncate().child(name.clone()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(DbEvent::OpenTable {
                            profile: profile.clone(),
                            schema: schema.clone(),
                            table: table.clone(),
                        });
                    }))
                    .context_menu({
                        let weak = cx.entity().downgrade();
                        move |menu, _, _| relation_menu(menu, weak.clone(), target.clone())
                    })
                    .into_any_element()
            }
            Node::Column {
                key,
                name,
                detail,
                primary,
            } => row(ElementId::Name(format!("db-col\u{0}{key}").into()), 5, cx)
                .cursor_default()
                // The name keeps its room; the type truncates first.
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(relative(0.7))
                        .truncate()
                        .child(name.clone()),
                )
                .when(*primary, |el| {
                    el.child(Icon::new(Lucide::KeyRound).xsmall().text_color(muted))
                })
                .child(
                    div()
                        .min_w_0()
                        .text_xs()
                        .text_color(muted)
                        .truncate()
                        .child(detail.clone()),
                )
                .into_any_element(),
            Node::Group {
                key,
                facet,
                count,
                open,
            } => {
                let k = key.clone();
                let empty = *count == 0;
                row(ElementId::Name(format!("db-group\u{0}{key}").into()), 5, cx)
                    .text_color(muted)
                    .child(
                        Icon::new(if *open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .xsmall()
                        .when(empty, |i| i.invisible()),
                    )
                    .child(
                        Icon::new(match facet {
                            Facet::Indexes => Lucide::ListTree,
                            Facet::Constraints => Lucide::ShieldCheck,
                        })
                        .xsmall(),
                    )
                    .child(facet.label())
                    .child(div().text_xs().child(count.to_string()))
                    .when(empty, |el| el.cursor_default())
                    .when(!empty, |el| {
                        el.on_click(cx.listener(move |this, _, _, cx| this.toggle(k.clone(), cx)))
                    })
                    .into_any_element()
            }
            Node::Item {
                key,
                name,
                detail,
                definition,
                ..
            } => {
                let full: SharedString = definition.clone().into();
                row(ElementId::Name(format!("db-item\u{0}{key}").into()), 7, cx)
                    .cursor_default()
                    .child(div().truncate().child(name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .flex_shrink_0()
                            .child(detail.clone()),
                    )
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(full.clone()).build(window, cx)
                    })
                    .into_any_element()
            }
        }
    }
}
