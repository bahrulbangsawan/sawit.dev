//! The SEO panel's Overview tab: the scores, what to fix first and what the page says about
//! itself.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::web::seo::{CheckResult, Report, Status};

use super::super::panel;
use super::{SeoPanel, previews, rows, scanned_ago};

impl SeoPanel {
    pub(super) fn overview(&self, report: &Report, cx: &mut Context<Self>) -> impl IntoElement {
        let f = &report.facts;
        let theme = cx.theme();
        let [passed, warned, failed] = report.counts(true);
        let fixes: Vec<&CheckResult> = report
            .checks
            .iter()
            .filter(|c| c.seo && matches!(c.status, Status::Fail | Status::Warn))
            .collect();
        let mut sorted = fixes.clone();
        sorted.sort_by_key(|c| (c.status != Status::Fail, c.tier as u8));
        let ago = self
            .scanned_at
            .map(|t| scanned_ago(t.elapsed().as_secs()))
            .unwrap_or_default();
        let url = report.url.clone();
        let all_prompt = sawit_core::web::seo::fix_prompt_all(report, true);
        v_flex()
            .gap_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .child(previews::favicon_slot(self.favicon.clone(), 16., cx))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(f.host.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_color(theme.muted_foreground)
                            .child(ago),
                    )
                    .when(report.rendered, |el| {
                        el.child(panel::tag("Rendered page", cx))
                    }),
            )
            .child(rows::scores(
                vec![
                    ("Search and sharing".into(), report.seo_score, "/100".into()),
                    (
                        "Agent readiness".into(),
                        report.agent_score,
                        format!("· {}", report.grade).into(),
                    ),
                ],
                cx,
            ))
            .child(rows::counts(passed, warned, failed, cx))
            .child(panel::section(
                "To fix",
                (!sorted.is_empty()).then(|| {
                    Button::new("seo-copy-all")
                        .xsmall()
                        .outline()
                        .icon(Lucide::Copy)
                        .label("Copy Prompt")
                        .tooltip("Copy One Prompt That Lists Every Problem and Its Fix")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.copy(all_prompt.clone(), "Copied the fixes as a prompt", cx)
                        }))
                        .into_any_element()
                }),
                if sorted.is_empty() {
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child("Nothing to fix: every search and sharing check passes.")
                        .into_any_element()
                } else {
                    v_flex()
                        .gap_1()
                        .children(
                            sorted
                                .into_iter()
                                .take(8)
                                .map(|c| self.check_row(c, &url, cx).into_any_element()),
                        )
                        .into_any_element()
                },
                cx,
            ))
            .child(panel::section(
                "Page",
                None,
                v_flex()
                    .children(rows::fact_row("Title", f.title.clone(), cx))
                    .children(rows::fact_row("Description", f.description.clone(), cx))
                    .children(rows::fact_row("Canonical", f.canonical.clone(), cx))
                    .children(rows::fact_row("Language", f.lang.clone(), cx))
                    .children(rows::fact_row("Robots", f.robots.clone(), cx))
                    .children(rows::fact_row(
                        "Share image",
                        match (f.image_width, f.image_height) {
                            (Some(w), Some(h)) => Some(format!(
                                "{w}×{h} {}",
                                f.image_type
                                    .as_deref()
                                    .unwrap_or("")
                                    .trim_start_matches("image/")
                                    .to_ascii_uppercase()
                            )),
                            _ => f.og_image.clone().map(|_| "Doesn't load".to_string()),
                        },
                        cx,
                    ))
                    .children(rows::fact_row(
                        "Structured data",
                        (!f.schema_types.is_empty()).then(|| f.schema_types.join(", ")),
                        cx,
                    ))
                    .children(rows::fact_row(
                        "Headings",
                        (!f.headings.is_empty()).then(|| {
                            let h1 = f.headings.iter().filter(|(l, _)| *l == 1).count();
                            format!("{} in all, {h1} h1", f.headings.len())
                        }),
                        cx,
                    )),
                cx,
            ))
    }
}
