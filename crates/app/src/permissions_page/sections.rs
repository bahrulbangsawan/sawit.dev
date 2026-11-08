//! The page's sections, each a list in the catalog pages' style (MCP, Skills, Dev Tools): a card
//! with column titles, then a row per item with its tile, name and details, a status pill and
//! its buttons. Claude Code's settings files (`files.rs`), Sawit's tools that need `confirm`, the
//! macOS permissions, and the other agents' settings.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::extras::Privacy;
use sawit_core::permissions::{Bucket, OtherAgent, RulesFile, ToolState};

use super::{PermissionsPage, Section};
use crate::catalog::{self, Mark, tilde};

/// A section: its title and what it is over the list.
pub(super) fn section(
    title: &'static str,
    note: &'static str,
    list: AnyElement,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_2()
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_base().font_semibold().child(title))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .whitespace_normal()
                        .child(note),
                ),
        )
        .child(list)
        .into_any_element()
}

/// The list card: column titles, then the rows, the last without a rule under it.
pub(super) fn list(column: &'static str, rows: Vec<Stateful<Div>>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let last = rows.len().saturating_sub(1);
    v_flex()
        .id(column)
        .role(Role::List)
        .rounded(theme.radius_lg)
        .border_1()
        .border_color(theme.border)
        .overflow_hidden()
        .child(catalog::header(None, column, cx))
        .children(rows.into_iter().enumerate().map(|(ix, row)| {
            row.px_4()
                .py_3()
                .when(ix < last, |el| el.border_b_1().border_color(theme.border))
                .hover(|el| el.bg(theme.list_hover))
        }))
        .into_any_element()
}

/// A logo tile with an icon, as the catalog rows lead with.
pub(super) fn icon_tile(icon: Lucide, cx: &App) -> Div {
    catalog::tile(
        Icon::new(icon)
            .size(px(18.))
            .text_color(cx.theme().muted_foreground)
            .into_any_element(),
        cx,
    )
}

pub(super) fn mono_line(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .font_family(cx.theme().mono_font_family.clone())
        .whitespace_normal()
        .child(text.into())
}

/// Whether `query` (lower case) is in any of `texts`.
fn has(query: &str, texts: &[&str]) -> bool {
    query.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(query))
}

pub(super) fn file_matches(file: &RulesFile, query: &str) -> bool {
    let path = tilde(&file.path);
    has(query, &[&file.scope.label(), &path])
        || Bucket::ALL
            .iter()
            .any(|b| file.rules.get(*b).iter().any(|r| has(query, &[r])))
}

pub(super) fn tool_matches(tool: &ToolState, query: &str) -> bool {
    has(query, &[tool.tool, tool.what])
}

pub(super) fn macos_matches(p: &Privacy, query: &str) -> bool {
    has(query, &[p.name(), p.why()])
}

pub(super) fn other_matches(o: &OtherAgent, query: &str) -> bool {
    has(query, &[o.agent, &tilde(&o.path)]) || o.lines.iter().any(|l| has(query, &[l]))
}

impl PermissionsPage {
    /// The sections the tab and the search leave, or the empty state.
    pub(super) fn render_sections(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let current = self.section;
        let show = |s: Section| current == Section::All || current == s;
        let mut out = Vec::new();
        if show(Section::Claude) {
            out.extend(self.render_files(query, window, cx));
        }
        if show(Section::Tools) {
            out.extend(self.render_tools(query, cx));
        }
        if show(Section::MacOs) {
            out.extend(self.render_macos(query, cx));
        }
        if show(Section::Others) {
            out.extend(self.render_others(query, cx));
        }
        if out.is_empty() {
            let title = if query.is_empty() {
                "Nothing here".to_string()
            } else {
                format!("Nothing matches “{query}”")
            };
            out.push(
                v_flex()
                    .rounded(cx.theme().radius_lg)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(catalog::empty(
                        title,
                        Button::new("perms-clear-filters")
                            .outline()
                            .small()
                            .icon(Lucide::FunnelX)
                            .label("Clear Filters")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.clear_filters(window, cx)),
                            ),
                        cx,
                    ))
                    .into_any_element(),
            );
        }
        out
    }

    fn render_tools(&self, query: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let rows: Vec<Stateful<Div>> = self
            .tools
            .iter()
            .enumerate()
            .filter(|(_, t)| tool_matches(t, query))
            .map(|(ix, t)| {
                let bucket = t.rule.as_ref().map(|(b, _, _)| *b);
                let (mark, status, color) = match bucket {
                    Some(Bucket::Deny) => (Lucide::Ban, "Denied", theme.danger),
                    Some(Bucket::Ask) => (Lucide::CircleCheck, "Always asks", theme.success),
                    Some(Bucket::Allow) => {
                        (Lucide::CircleAlert, "Runs without asking", theme.warning)
                    }
                    None => (Lucide::Info, "Asks by default", muted),
                };
                let tool = t.tool;
                let details = v_flex()
                    .gap_0p5()
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .font_family(theme.mono_font_family.clone())
                            .child(tool),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(t.what),
                    )
                    .when_some(t.rule.as_ref(), |el, (_, rule, file)| {
                        el.child(mono_line(format!("{rule} · {file}"), cx))
                    });
                let actions = h_flex()
                    .gap_1()
                    .when(bucket != Some(Bucket::Ask), |el| {
                        el.child(
                            Button::new(("perms-tool-ask", ix))
                                .outline()
                                .small()
                                .label("Always Ask…")
                                .tooltip("Adds mcp__sawit__… to Ask in Your User Settings")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_tool(tool, Bucket::Ask, cx)
                                })),
                        )
                    })
                    .when(bucket != Some(Bucket::Deny), |el| {
                        el.child(
                            Button::new(("perms-tool-deny", ix))
                                .outline()
                                .small()
                                .label("Deny…")
                                .tooltip("Adds mcp__sawit__… to Deny in Your User Settings")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_tool(tool, Bucket::Deny, cx)
                                })),
                        )
                    });
                let row = catalog::columns(
                    None,
                    icon_tile(Lucide::Wrench, cx),
                    details,
                    catalog::pill(Mark::Icon(mark), status, color),
                    actions,
                );
                catalog::list_item(row, ("perms-tool", ix), tool)
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(section(
            "Sawit's Tools That Need Confirmation",
            "These sawit MCP tools act only when called with confirm=true. Claude Code's rules decide whether it asks you first; an MCP tool no rule names asks.",
            list("Tool", rows, cx),
            cx,
        ))
    }

    fn render_macos(&self, query: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let rows: Vec<Stateful<Div>> = Privacy::ALL
            .iter()
            .enumerate()
            .filter(|(_, p)| macos_matches(p, query))
            .map(|(ix, p)| {
                let url = p.settings_url();
                let (mark, state, color) = match p.state() {
                    Some(true) => (Lucide::CircleCheck, "Allowed", theme.success),
                    Some(false) => (Lucide::CircleX, "Not allowed", theme.danger),
                    None => (Lucide::Info, "See System Settings", muted),
                };
                let row = catalog::columns(
                    None,
                    icon_tile(Lucide::Lock, cx),
                    v_flex()
                        .gap_0p5()
                        .child(div().text_sm().font_semibold().child(p.name()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child(p.why()),
                        ),
                    catalog::pill(Mark::Icon(mark), state, color),
                    Button::new(("perms-macos-open", ix))
                        .outline()
                        .small()
                        .icon(Lucide::Settings)
                        .label("Open Settings")
                        .tooltip(format!("Privacy & Security, {}", p.name()))
                        .on_click(move |_, _, cx| cx.open_url(url)),
                );
                catalog::list_item(row, ("perms-macos", ix), p.name())
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(section(
            "macOS",
            "What Sawit, and the programs in its terminals (which macOS asks about as Sawit), may do. macOS shows Automation and Files & Folders only in System Settings.",
            list("Permission", rows, cx),
            cx,
        ))
    }

    fn render_others(&self, query: &str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let muted = cx.theme().muted_foreground;
        let rows: Vec<Stateful<Div>> = self
            .others
            .iter()
            .enumerate()
            .filter(|(_, o)| other_matches(o, query))
            .map(|(ix, o)| {
                let n = o.lines.len();
                let row = catalog::columns(
                    None,
                    icon_tile(Lucide::Bot, cx),
                    v_flex()
                        .gap_0p5()
                        .child(div().text_sm().font_semibold().child(o.agent))
                        .child(mono_line(tilde(&o.path), cx))
                        .when(n > 0, |el| {
                            el.child(
                                v_flex()
                                    .mt_1p5()
                                    .p_2()
                                    .gap_0p5()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().secondary)
                                    .children(o.lines.iter().map(|l| {
                                        div()
                                            .text_xs()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .whitespace_normal()
                                            .child(l.clone())
                                    })),
                            )
                        }),
                    catalog::pill(
                        Mark::Icon(Lucide::Lock),
                        if n == 1 {
                            "Read only · 1 setting".to_string()
                        } else {
                            format!("Read only · {n} settings")
                        },
                        muted,
                    ),
                    div(),
                );
                catalog::list_item(row, ("perms-other", ix), o.agent)
            })
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(section(
            "Other Agents",
            "Their permission settings as their files hold them. Sawit shows them; change them in the agent or its file.",
            list("Agent", rows, cx),
            cx,
        ))
    }
}
