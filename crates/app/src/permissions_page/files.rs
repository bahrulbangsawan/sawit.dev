//! The Claude Code section: a row per settings file, with its rules by list (each a chip whose
//! menu moves or removes it), and the new-rule field under the file it adds to.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::permissions::{Bucket, Change, RulesFile};

use super::PermissionsPage;
use super::sections::{file_matches, icon_tile, list, mono_line, section};
use crate::catalog::{self, Mark, tilde};

impl PermissionsPage {
    pub(super) fn render_files(
        &mut self,
        query: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let files = self.files.clone();
        let rows: Vec<Stateful<Div>> = files
            .iter()
            .enumerate()
            .filter(|(_, f)| file_matches(f, query))
            .map(|(ix, file)| self.render_file(ix, file, window, cx))
            .collect();
        if rows.is_empty() {
            return None;
        }
        Some(section(
            "Claude Code",
            "permissions.allow, ask and deny in each settings file, highest priority first. A rule is a tool, optionally with what it applies to: Bash(npm run test:*), Read(~/notes/**), mcp__sawit__query. Project files show for the open projects; adding a rule to one that isn't there yet creates it.",
            list("Settings File", rows, cx),
            cx,
        ))
    }

    fn render_file(
        &mut self,
        ix: usize,
        file: &RulesFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let project = file
            .scope
            .project()
            .and_then(|p| p.file_name())
            .map(|n| format!(" · {}", n.to_string_lossy()))
            .unwrap_or_default();
        let name = format!("{}{project}", file.scope.label());
        let counts = Bucket::ALL.map(|b| file.rules.get(b).len());
        let total: usize = counts.iter().sum();
        let adding = self.adding.as_ref() == Some(&file.path);

        let buckets = Bucket::ALL
            .into_iter()
            .filter(|b| !file.rules.get(*b).is_empty())
            .map(|bucket| {
                let rules = file.rules.get(bucket).to_vec();
                h_flex()
                    .gap_2()
                    .items_start()
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .pt(px(3.))
                            .text_xs()
                            .font_medium()
                            .text_color(muted)
                            .child(bucket.title()),
                    )
                    .child(
                        h_flex().flex_1().min_w_0().flex_wrap().gap_1().children(
                            rules
                                .into_iter()
                                .enumerate()
                                .map(|(r, rule)| self.render_rule(ix, bucket, r, rule, file, cx)),
                        ),
                    )
            })
            .collect::<Vec<_>>();

        let details = v_flex()
            .gap_0p5()
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(div().text_sm().font_semibold().child(name.clone()))
                    .when(!file.editable && file.problem.is_none(), |el| {
                        el.child(catalog::tag(Some(Lucide::Lock), "Read only", cx))
                    }),
            )
            .child(mono_line(tilde(&file.path), cx))
            .when_some(file.problem.clone(), |el, problem| {
                el.child(
                    h_flex()
                        .gap_1p5()
                        .items_start()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(div().pt(px(1.)).child(Icon::new(Lucide::CircleX).xsmall()))
                        .child(div().flex_1().min_w_0().whitespace_normal().child(format!(
                            "Its rules can't be read, so Sawit doesn't edit it: {problem}"
                        ))),
                )
            })
            .when(!buckets.is_empty(), |el| {
                el.child(v_flex().pt_1p5().gap_1p5().children(buckets))
            });

        let (mark, status, color) = if file.problem.is_some() {
            (Lucide::CircleX, "Can't read".to_string(), theme.danger)
        } else if !file.exists {
            (Lucide::CircleDashed, "Not there yet".to_string(), muted)
        } else if total == 0 {
            (Lucide::CircleDashed, "No rules".to_string(), muted)
        } else {
            (
                Lucide::ListChecks,
                format!(
                    "{} allow · {} ask · {} deny",
                    counts[0], counts[1], counts[2]
                ),
                theme.foreground,
            )
        };

        let path = file.path.clone();
        let actions = h_flex().gap_1().when(file.editable, |el| {
            el.child(
                Button::new(("perms-add", ix))
                    .outline()
                    .small()
                    .icon(Lucide::Plus)
                    .label("Add Rule")
                    .selected(adding)
                    .accessibility_label(format!("Add a Rule to {name}"))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.toggle_adding(path.clone(), window, cx)
                    })),
            )
        });

        let row = catalog::columns(
            None,
            icon_tile(Lucide::FileCog, cx),
            details,
            catalog::pill(Mark::Icon(mark), status, color),
            actions,
        );
        let mut el = v_flex().gap_3().child(row);
        if adding && file.editable {
            el = el.child(self.render_add(ix, file, &name, window, cx));
        }
        catalog::list_item(el, ("perms-file", ix), name)
    }

    /// The new rule's field and where it goes, under its file's row.
    fn render_add(
        &mut self,
        ix: usize,
        file: &RulesFile,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let input = self.input(&file.path, name, window, cx);
        let path = file.path.clone();
        h_flex()
            // Under the name: past the tile and its gap.
            .pl(px(catalog::TILE + 12.))
            .gap_2()
            .flex_wrap()
            .child(
                div()
                    .flex_1()
                    .min_w(px(220.))
                    .child(Input::new(&input).small()),
            )
            .child(
                h_flex()
                    .gap_1()
                    .children(Bucket::ALL.map(|bucket| {
                        let path = path.clone();
                        Button::new(SharedString::from(format!(
                            "perms-add-{ix}-{}",
                            bucket.key()
                        )))
                        .when(bucket == Bucket::Allow, |b| b.primary())
                        .when(bucket != Bucket::Allow, |b| b.outline())
                        .small()
                        .label(format!("Add to {}…", bucket.title()))
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.add_typed(path.clone(), bucket, window, cx)
                            },
                        ))
                    }))
                    .child(
                        Button::new(("perms-add-cancel", ix))
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.adding = None;
                                cx.notify();
                            })),
                    ),
            )
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
}
