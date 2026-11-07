//! The page's sections: Claude Code's settings files with their rules, Sawit's tools that need
//! `confirm`, the macOS permissions, and the other agents' settings.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::dev_tools::extras::Privacy;
use sawit_core::permissions::{Bucket, Change, RulesFile};

use super::PermissionsPage;
use crate::catalog::tilde;

fn section(title: &'static str, note: &'static str, cx: &App) -> Div {
    v_flex().gap_2().child(
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
}

fn card(id: impl Into<ElementId>, name: String, cx: &App) -> Stateful<Div> {
    v_flex()
        .id(id)
        .role(Role::Group)
        .aria_label(name)
        .p_3()
        .gap_2()
        .rounded(cx.theme().radius_lg)
        .border_1()
        .border_color(cx.theme().border)
}

impl PermissionsPage {
    pub(super) fn render_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let files = self.files.clone();
        let cards: Vec<AnyElement> = files
            .iter()
            .enumerate()
            .map(|(ix, file)| self.render_file(ix, file, window, cx))
            .collect();
        section(
            "Claude Code",
            "permissions.allow, ask and deny in each settings file, highest priority first. A rule is a tool, optionally with what it applies to: Bash(npm run test:*), Read(~/notes/**), mcp__sawit__query. Project files show for the open projects; adding a rule to one that isn't there yet creates it.",
            cx,
        )
        .children(cards)
    }

    fn render_file(
        &mut self,
        ix: usize,
        file: &RulesFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let project = file
            .scope
            .project()
            .and_then(|p| p.file_name())
            .map(|n| format!(" · {}", n.to_string_lossy()))
            .unwrap_or_default();
        let name = format!("{}{project}", file.scope.label());
        let label = name.clone();
        let mut el = card(("perms-file", ix), name.clone(), cx).child(
            h_flex()
                .gap_2()
                .child(div().text_sm().font_semibold().child(name))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_xs()
                        .text_color(muted)
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(tilde(&file.path)),
                )
                .when(!file.exists, |el| {
                    el.child(crate::catalog::tag(None, "Not there yet", cx))
                })
                .when(!file.editable && file.problem.is_none(), |el| {
                    el.child(crate::catalog::tag(None, "Read only", cx))
                }),
        );
        if let Some(problem) = &file.problem {
            el = el.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().danger)
                    .whitespace_normal()
                    .child(format!(
                        "Its rules can't be read, so Sawit doesn't edit it: {problem}"
                    )),
            );
        }
        let lists = h_flex()
            .gap_3()
            .items_start()
            .children(Bucket::ALL.map(|bucket| {
                let rules = file.rules.get(bucket).to_vec();
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .font_semibold()
                            .text_color(muted)
                            .child(format!("{} ({})", bucket.title(), rules.len())),
                    )
                    .children(
                        rules
                            .into_iter()
                            .enumerate()
                            .map(|(r, rule)| self.render_rule(ix, bucket, r, rule, file, cx)),
                    )
            }));
        el = el.child(lists);
        if file.editable {
            let input = self.input(&file.path, &label, window, cx);
            let path = file.path.clone();
            el = el.child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(Input::new(&input).small()))
                    .children(Bucket::ALL.map(|bucket| {
                        let path = path.clone();
                        Button::new(SharedString::from(format!(
                            "perms-add-{ix}-{}",
                            bucket.key()
                        )))
                        .outline()
                        .small()
                        .label(format!("Add to {}…", bucket.title()))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.add_typed(path.clone(), bucket, window, cx)
                            },
                        ))
                    })),
            );
        }
        el.into_any_element()
    }

    fn render_rule(
        &self,
        ix: usize,
        bucket: Bucket,
        r: usize,
        rule: String,
        file: &RulesFile,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = SharedString::from(format!("perms-rule-{ix}-{}-{r}", bucket.key()));
        let chip = Button::new(id)
            .outline()
            .xsmall()
            .label(rule.clone())
            .tooltip(rule.clone());
        if !file.editable {
            return chip.disabled(true).into_any_element();
        }
        let (page, path) = (cx.weak_entity(), file.path.clone());
        chip.dropdown_caret(true)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for to in Bucket::ALL.into_iter().filter(|b| *b != bucket) {
                    let (page, path, rule) = (page.clone(), path.clone(), rule.clone());
                    menu = menu.item(
                        PopupMenuItem::new(format!("Move to {}…", to.title()))
                            .icon(crate::menu_icons::MOVE)
                            .on_click(move |_, _, cx| {
                                let _ = page.update(cx, |this, cx| {
                                    this.propose(
                                        path.clone(),
                                        Change::Move {
                                            rule: rule.clone(),
                                            to,
                                        },
                                        cx,
                                    )
                                });
                            }),
                    );
                }
                let (page, path, rule) = (page.clone(), path.clone(), rule.clone());
                menu.separator().item(
                    PopupMenuItem::new("Remove Rule…")
                        .icon(crate::menu_icons::DELETE)
                        .danger(true)
                        .on_click(move |_, _, cx| {
                            let _ = page.update(cx, |this, cx| {
                                this.propose(
                                    path.clone(),
                                    Change::Remove { rule: rule.clone() },
                                    cx,
                                )
                            });
                        }),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_tools(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let rows =
            self.tools.iter().enumerate().map(|(ix, t)| {
                let state = match &t.rule {
                    Some((Bucket::Deny, rule, file)) => format!("Denied by {rule} ({file})"),
                    Some((Bucket::Ask, rule, file)) => format!("Asks: {rule} ({file})"),
                    Some((Bucket::Allow, rule, file)) => {
                        format!("Allowed without asking: {rule} ({file})")
                    }
                    None => "Asks (Claude Code's default)".into(),
                };
                let tool = t.tool;
                h_flex()
                    .id(("perms-tool", ix))
                    .role(Role::Group)
                    .aria_label(tool)
                    .gap_3()
                    .py_1()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .w(px(220.))
                            .flex_none()
                            .child(
                                div()
                                    .text_sm()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .child(tool),
                            )
                            .child(div().text_xs().text_color(muted).child(t.what)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .whitespace_normal()
                            .child(state),
                    )
                    .child(
                        Button::new(("perms-tool-ask", ix))
                            .ghost()
                            .xsmall()
                            .label("Always Ask…")
                            .tooltip("Adds mcp__sawit__… to Ask in Your User Settings")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_tool(tool, Bucket::Ask, cx)
                            })),
                    )
                    .child(
                        Button::new(("perms-tool-deny", ix))
                            .ghost()
                            .xsmall()
                            .label("Deny…")
                            .tooltip("Adds mcp__sawit__… to Deny in Your User Settings")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_tool(tool, Bucket::Deny, cx)
                            })),
                    )
            });
        section(
            "Sawit's Tools That Need Confirmation",
            "These sawit MCP tools act only when called with confirm=true. Claude Code's rules decide whether it asks you first; an MCP tool no rule names asks.",
            cx,
        )
        .child(v_flex().children(rows))
    }

    pub(super) fn render_macos(&self, cx: &mut Context<Self>) -> Div {
        let muted = cx.theme().muted_foreground;
        let rows = Privacy::ALL.iter().enumerate().map(|(ix, p)| {
            let url = p.settings_url();
            let (state, color) = match p.state() {
                Some(true) => ("Allowed", cx.theme().success),
                Some(false) => ("Not Allowed", cx.theme().danger),
                None => ("See System Settings", muted),
            };
            h_flex()
                .id(("perms-macos", ix))
                .role(Role::Group)
                .aria_label(p.name())
                .gap_3()
                .py_1()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(div().text_sm().child(p.name()))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .whitespace_normal()
                                .child(p.why()),
                        ),
                )
                .child(
                    div()
                        .w(px(140.))
                        .flex_none()
                        .text_xs()
                        .text_color(color)
                        .child(state),
                )
                .child(
                    Button::new(("perms-macos-open", ix))
                        .outline()
                        .xsmall()
                        .icon(Lucide::Settings)
                        .label("Open Settings")
                        .tooltip(format!("Privacy & Security, {}", p.name()))
                        .on_click(move |_, _, cx| cx.open_url(url)),
                )
        });
        section(
            "macOS",
            "What Sawit, and the programs in its terminals (which macOS asks about as Sawit), may do. macOS shows Automation and Files & Folders only in System Settings.",
            cx,
        )
        .child(v_flex().children(rows))
    }

    pub(super) fn render_others(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.others.is_empty() {
            return None;
        }
        let cards = self.others.iter().enumerate().map(|(ix, o)| {
            card(("perms-other", ix), o.agent.to_string(), cx)
                .child(
                    h_flex()
                        .gap_2()
                        .child(div().text_sm().font_semibold().child(o.agent))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(tilde(&o.path)),
                        )
                        .child(crate::catalog::tag(None, "Read only", cx)),
                )
                .children(o.lines.iter().map(|l| {
                    div()
                        .text_xs()
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(l.clone())
                }))
        });
        Some(
            section(
                "Other Agents",
                "Their permission settings as their files hold them. Sawit shows them; change them in the agent or its file.",
                cx,
            )
            .children(cards),
        )
    }
}
