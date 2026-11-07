//! The SEO panel's Agents tab: agent readiness. The scan type (all, content site, API or app),
//! the total with its grade, the Essentials score, each layer's share with its weight, the
//! external score when asked for (or why there is none), and the checks of each layer, problems
//! first.

use gpui_kit::component::button::{Button, ButtonGroup};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::web::seo::{CheckResult, Layer, Report, ScanType, Status};

use super::super::panel;
use super::{SeoEvent, SeoPanel, rows};

const SCAN_TYPES: [(ScanType, &str); 3] = [
    (ScanType::All, "All"),
    (ScanType::Content, "Content site"),
    (ScanType::Api, "API or app"),
];

impl SeoPanel {
    pub(super) fn agents(&self, report: &Report, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, track) = {
            let t = cx.theme();
            (t.foreground, t.muted_foreground, t.muted)
        };
        let url = report.url.clone();
        let current = report.scan_type;
        let external_on = self.external;
        v_flex()
            .gap_4()
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        ButtonGroup::new("seo-scan-type")
                            .small()
                            .outline()
                            .w_full()
                            .children(SCAN_TYPES.iter().map(|(t, label)| {
                                Button::new(SharedString::from(format!("seo-type-{label}")))
                                    .label(*label)
                                    .flex_1()
                                    .selected(*t == current)
                            }))
                            .on_click(cx.listener(|_, clicks: &Vec<usize>, _, cx| {
                                if let Some(ix) = clicks.first()
                                    && let Some((t, _)) = SCAN_TYPES.get(*ix)
                                {
                                    cx.emit(SeoEvent::ScanType(*t));
                                }
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(match current {
                                ScanType::All => "Every check runs.",
                                ScanType::Content => "A site to read: API checks are left out.",
                                ScanType::Api => "An API or app: checks for content sites are left out.",
                            }),
                    ),
            )
            .child(rows::scores(
                vec![
                    (
                        "Agent readiness".into(),
                        report.agent_score,
                        format!("· {}", report.grade).into(),
                    ),
                    ("Essentials".into(), report.essentials, "/100".into()),
                ],
                cx,
            ))
            .child(panel::section(
                "Layers",
                None,
                v_flex().gap_2().children(report.layers.iter().map(|l| {
                    let shown = l.max > 0.;
                    let fill = l.percent as f32 / 100.;
                    v_flex()
                        .gap_1()
                        .child(
                            h_flex()
                                .justify_between()
                                .text_xs()
                                .child(div().text_color(fg).child(l.name.clone()))
                                .child(div().text_color(muted).child(if shown {
                                    format!("{}% · weight {:.0}", l.percent, l.weight)
                                } else {
                                    "n/a".into()
                                })),
                        )
                        .child(
                            div()
                                .h(px(6.))
                                .w_full()
                                .rounded_full()
                                .bg(track)
                                .overflow_hidden()
                                .when(shown, |d| {
                                    d.child(
                                        div()
                                            .h_full()
                                            .w(relative(fill))
                                            .rounded_full()
                                            .bg(fg),
                                    )
                                }),
                        )
                })),
                cx,
            ))
            .child(panel::section(
                "External score",
                Some(
                    Switch::new("seo-external")
                        .checked(external_on)
                        .small()
                        .label("Ask")
                        .on_click(cx.listener(|_, on: &bool, _, cx| cx.emit(SeoEvent::External(*on))))
                        .into_any_element(),
                ),
                div().text_xs().child(match &report.external {
                    Some(x) => match (x.score, &x.missing) {
                        (Some(s), _) => div()
                            .text_color(fg)
                            .child(format!(
                                "{s}/100{}: its results fill the checks that need outside data.",
                                x.grade.as_deref().map(|g| format!(" · {g}")).unwrap_or_default()
                            ))
                            .into_any_element(),
                        (None, Some(why)) => div()
                            .text_color(muted)
                            .child(why.clone())
                            .into_any_element(),
                        _ => div().into_any_element(),
                    },
                    None if report.local => div()
                        .text_color(muted)
                        .child("A local address isn't sent to an outside service.")
                        .into_any_element(),
                    None => div()
                        .text_color(muted)
                        .child("Off. Turn it on to compare with an outside scanner and fill the checks Sawit can't run (search ranking, directories).")
                        .into_any_element(),
                }),
                cx,
            ))
            .children(Layer::AGENT.iter().map(|layer| {
                let mut list: Vec<&CheckResult> = report
                    .checks
                    .iter()
                    .filter(|c| c.layer == *layer && c.status != Status::Skipped)
                    .collect();
                list.sort_by_key(|c| rows::status_rank(c.status));
                let to_fix = list
                    .iter()
                    .filter(|c| matches!(c.status, Status::Fail | Status::Warn))
                    .count();
                let skipped = report
                    .checks
                    .iter()
                    .filter(|c| c.layer == *layer && c.status == Status::Skipped)
                    .count();
                let rows: Vec<AnyElement> = list
                    .into_iter()
                    .map(|c| self.check_row(c, &url, cx).into_any_element())
                    .collect();
                panel::section(
                    layer.name(),
                    Some(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(if to_fix == 0 {
                                "Nothing to fix".to_string()
                            } else {
                                format!("{to_fix} to fix")
                            })
                            .into_any_element(),
                    ),
                    v_flex().gap_1().children(rows).when(skipped > 0, |el| {
                        el.child(
                            div()
                                .pt_1()
                                .text_xs()
                                .text_color(muted)
                                .child(format!(
                                    "{skipped} more need outside data: turn on the external score to fill them."
                                )),
                        )
                    }),
                    cx,
                )
                .into_any_element()
            }))
    }
}
