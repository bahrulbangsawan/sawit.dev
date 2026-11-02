//! A Skills row: the pack's tile, name, publisher, skills and source, its status across the
//! agents, its actions, and under it its last line of output, its failure or its output.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::skills::{Pack, Status};

use super::{SkillsPage, State, doing, pack_logo, plural};
use crate::catalog::{self, Mark};

impl SkillsPage {
    pub(super) fn render_row(
        &self,
        ix: usize,
        pack: &'static Pack,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let muted = theme.muted_foreground;
        let mono = theme.mono_font_family.clone();
        let id = pack.id;
        let state = self.states.get(id).cloned();
        let total = self.report.targets.len();
        let having = self.having(pack);
        let full = having
            .iter()
            .filter(|&&t| self.status(pack, t).full())
            .count();
        // Where its copy of the skill Sawit ships isn't this Sawit's version.
        let differs = having
            .iter()
            .filter(|&&t| self.status(pack, t) == Status::Differs)
            .count();
        let busy = self.busy(id);

        let (mark, badge, color): (Mark, String, Hsla) = match &state {
            Some(State::Queued(_)) => (Mark::Icon(Lucide::Clock), "Queued".into(), muted),
            Some(State::Running(action)) => {
                (Mark::Spinner, format!("{}…", doing(*action)), theme.warning)
            }
            Some(State::Failed(..)) => (Mark::Icon(Lucide::CircleX), "Failed".into(), theme.danger),
            Some(State::Stopped) => (
                Mark::Icon(Lucide::CircleStop),
                "Stopped".into(),
                theme.warning,
            ),
            _ if self.loading => (Mark::Spinner, "Checking…".into(), muted),
            _ if having.is_empty() => (
                Mark::Icon(Lucide::CircleDashed),
                "Not installed".into(),
                muted,
            ),
            _ if full == 0 => (
                Mark::Icon(Lucide::CircleAlert),
                format!("Some skills · {} of {total}", having.len()),
                theme.warning,
            ),
            _ if differs > 0 => (
                Mark::Icon(Lucide::CircleAlert),
                format!(
                    "Installed · {} of {total} · Differs in {differs}",
                    having.len()
                ),
                theme.warning,
            ),
            _ => (
                Mark::Icon(Lucide::CircleCheck),
                format!("Installed · {} of {total}", having.len()),
                theme.success,
            ),
        };

        let log = self.logs.get(id);
        let running = matches!(state, Some(State::Running(_)));
        let last_line = log.filter(|_| running).and_then(|l| l.last().cloned());
        let expanded = self.expanded.contains(id) && log.is_some();
        let failure = match &state {
            Some(State::Failed(_, e)) if !expanded => Some(e.clone()),
            _ => None,
        };
        let ways: Vec<&str> = pack.methods.iter().map(|m| m.label()).collect();
        let home = pack.home;
        // Under the name: past the tile and its gap.
        let indent = px(catalog::TILE + 12.);

        let action = match &state {
            Some(State::Running(_)) => Button::new(("skill-stop", ix))
                .outline()
                .small()
                .icon(Lucide::CircleStop)
                .label("Stop")
                .tooltip("Stop the Script")
                .on_click(cx.listener(|this, _, _, cx| this.stop_running(cx))),
            Some(State::Queued(_)) => Button::new(("skill-dequeue", ix))
                .ghost()
                .small()
                .icon(Lucide::X)
                .label("Cancel")
                .tooltip("Take It Out of the Queue")
                .on_click(cx.listener(move |this, _, _, cx| this.dequeue(id, cx))),
            _ => Button::new(("skill-open", ix))
                .when(having.is_empty(), |b| {
                    b.primary().icon(Lucide::Download).label("Install")
                })
                .when(!having.is_empty(), |b| {
                    b.outline().icon(Lucide::Settings2).label("Manage")
                })
                .small()
                // It opens the sheet that installs, which the ellipsis says, as a Mac button that
                // opens a dialog does.
                .accessibility_label(if having.is_empty() {
                    format!("Install {}…", pack.name)
                } else {
                    format!("Manage {}…", pack.name)
                })
                .tooltip(if having.is_empty() {
                    "Choose the Agents and See the Commands"
                } else {
                    "Install, Update or Remove per Agent"
                })
                .disabled(self.loading || total == 0)
                .on_click(
                    cx.listener(move |this, _, window, cx| this.open_sheet(pack, window, cx)),
                ),
        };

        catalog::list_item(v_flex(), ("skill-row", ix), pack.name)
            .px_4()
            .py_3()
            .gap_1p5()
            .border_b_1()
            .border_color(theme.border)
            .hover(|el| el.bg(theme.list_hover))
            .when(running, |el| el.bg(theme.list_hover))
            .when(selected, |el| el.bg(theme.list_active))
            .child(catalog::columns(
                None,
                catalog::tile(pack_logo(pack, cx), cx),
                v_flex()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(div().text_sm().font_semibold().child(pack.name))
                            .child(catalog::tag(None, pack.category.name(), cx))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(format!("by {}", pack.by)),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(pack.about),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .whitespace_normal()
                            .child(format!(
                                "{}: {}",
                                plural(pack.skills.len(), "skill", "skills"),
                                pack.skills.join(" · ")
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(muted)
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(mono.clone())
                                    .child(pack.repo.trim_start_matches("https://")),
                            )
                            .child(format!("· {}", ways.join(", ")))
                            .child(
                                Button::new(("skill-docs", ix))
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::ExternalLink)
                                    .accessibility_label(format!("Open the Docs of {}", pack.name))
                                    .tooltip("Open Its Docs")
                                    .on_click(move |_, _, cx| cx.open_url(home)),
                            ),
                    )
                    .when_some(pack.note, |el, note| {
                        el.child(
                            h_flex()
                                .gap_1p5()
                                .items_start()
                                .text_xs()
                                .text_color(muted)
                                .child(div().pt(px(1.)).child(Icon::new(Lucide::Info).xsmall()))
                                .child(div().whitespace_normal().child(note)),
                        )
                    }),
                catalog::pill(mark, badge, color),
                h_flex()
                    .gap_1()
                    .when(log.is_some(), |el| {
                        el.child(
                            Button::new(("skill-log", ix))
                                .ghost()
                                .small()
                                .icon(Lucide::ScrollText)
                                .selected(expanded)
                                .accessibility_label("Output")
                                .toggled(expanded)
                                .tooltip(if expanded {
                                    "Hide Output"
                                } else {
                                    "Show Output"
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.expanded.remove(id) {
                                        this.expanded.insert(id);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!busy && !having.is_empty(), |el| {
                        let having = having.clone();
                        el.child(
                            Button::new(("skill-remove", ix))
                                .ghost()
                                .small()
                                .icon(Icon::new(Lucide::Trash).text_color(theme.danger))
                                .accessibility_label(format!(
                                    "Remove {} from Every Agent",
                                    pack.name
                                ))
                                .tooltip(format!("Remove {} from Every Agent", pack.name))
                                .disabled(self.loading)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.confirm_remove(pack, having.clone(), window, cx)
                                })),
                        )
                    })
                    .child(action),
            ))
            .when_some(last_line.filter(|_| !expanded), |el, line| {
                el.child(
                    div()
                        .pl(indent)
                        .text_xs()
                        .font_family(mono.clone())
                        .text_color(muted)
                        .truncate()
                        .child(line),
                )
            })
            .when_some(failure, |el, e| {
                el.child(
                    h_flex()
                        .pl(indent)
                        .gap_1p5()
                        .items_start()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(
                            div()
                                .pt(px(1.))
                                .child(Icon::new(Lucide::CircleAlert).xsmall()),
                        )
                        .child(div().whitespace_normal().child(e)),
                )
            })
            .when(expanded, |el| {
                let lines = log
                    .map(|l| l[l.len().saturating_sub(14)..].join("\n"))
                    .unwrap_or_default();
                el.child(
                    div()
                        .ml(indent)
                        .p_2()
                        .rounded(theme.radius)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.secondary)
                        .text_xs()
                        .font_family(mono)
                        .text_color(muted)
                        .whitespace_normal()
                        .child(lines),
                )
            })
            .into_any_element()
    }
}
