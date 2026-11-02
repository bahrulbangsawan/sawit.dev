//! The System tab: the export first, then the design tokens derived from the page: palette
//! ramps and semantic colours, type, spacing, radius, shadows, breakpoints and the page's own
//! variables. Every value copies on click.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::tokens::Tokens;

use super::colors::rgba_u32;
use super::{DesignEvent, DesignPanel, ExportShown, copy_row, copy_row_at, hex_and_alpha, swatch};
use crate::browser::panel;

/// The largest size a type-scale line is drawn at.
const MAX_SAMPLE_PX: f32 = 40.;
/// The semantic colours' name column: wide enough for `primary-foreground`.
const SEMANTIC_LABEL_WIDTH: f32 = 140.;
/// The ramp steps' names, under the swatches.
const STEPS: [&str; 11] = [
    "50", "100", "200", "300", "400", "500", "600", "700", "800", "900", "950",
];

/// What the export block says.
pub(super) fn export_lines(e: &ExportShown) -> Vec<String> {
    match &e.outcome {
        None if e.total == 0 => vec!["Reading the page…".into()],
        None => vec![format!("Downloading assets… {} / {}", e.done, e.total)],
        Some(Err(err)) => vec![err.clone()],
        Some(Ok(done)) => {
            let mut lines = vec![format!(
                "Exported {} file{} to {}",
                done.files,
                if done.files == 1 { "" } else { "s" },
                e.folder.display()
            )];
            if done.failed > 0 {
                lines.push(format!(
                    "{} of {} assets couldn't be downloaded",
                    done.failed,
                    e.total.saturating_sub(done.skipped_videos)
                ));
            }
            match done.skipped_videos {
                0 => {}
                1 => lines.push("1 video wasn't included".into()),
                n => lines.push(format!("{n} videos weren't included")),
            }
            if let Some(err) = &done.screenshot_error {
                lines.push(format!("No screenshot: {err}"));
            }
            lines
        }
    }
}

/// The export: what it writes, the button, its progress and where it went.
fn export_block(
    export: Option<&ExportShown>,
    include_videos: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let running = export.is_some_and(|e| e.outcome.is_none());
    let finished = export.and_then(|e| match &e.outcome {
        Some(Ok(_)) => Some(e.folder.clone()),
        _ => None,
    });
    let progress = export
        .filter(|e| e.outcome.is_none() && e.total > 0)
        .map(|e| e.done as f32 / e.total as f32 * 100.);
    v_flex()
        .gap_3()
        .p_3()
        .rounded(theme.radius)
        .border_1()
        .border_color(theme.border)
        .child(
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Export the design system"),
                )
                .child(div().text_xs().text_color(muted).child(
                    "DESIGN.md, DTCG tokens, a Tailwind v4 theme, shadcn/ui variables, sample \
                     components and the assets, in the project's folder:",
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .font_family(theme.mono_font_family.clone())
                        .child(".nyawit/design/<host>/"),
                ),
        )
        .child(
            h_flex()
                .gap_3()
                .items_center()
                .child(
                    Button::new("design-export")
                        .primary()
                        .small()
                        .icon(Lucide::Download)
                        .label(if running { "Exporting…" } else { "Export" })
                        .loading(running)
                        .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                            cx.emit(DesignEvent::Export { include_videos });
                        })),
                )
                .child(
                    Checkbox::new("design-export-videos")
                        .label("Include videos")
                        .checked(include_videos)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.include_videos = *checked;
                            cx.notify();
                        })),
                ),
        )
        .children(progress.map(|value| {
            Progress::new("design-export-progress")
                .value(value)
                .accessibility_label("Export progress")
        }))
        .children(export.map(|e| {
            v_flex().gap_0p5().children(
                export_lines(e)
                    .into_iter()
                    .map(|l| div().text_xs().text_color(muted).child(l)),
            )
        }))
        .when_some(finished, |el, folder| {
            let reveal = folder.join("DESIGN.md");
            let path = folder.display().to_string();
            el.child(
                h_flex()
                    .gap_1()
                    .child(
                        Button::new("design-export-reveal")
                            .outline()
                            .xsmall()
                            .icon(Lucide::FolderOpen)
                            .label("Reveal in Finder")
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(DesignEvent::Reveal(reveal.clone()));
                            })),
                    )
                    .child(
                        Button::new("design-export-copy")
                            .outline()
                            .xsmall()
                            .icon(Lucide::Copy)
                            .label("Copy path")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.copy(path.clone(), cx);
                            })),
                    ),
            )
        })
}

/// The radius a sample is drawn with, capped so a pill stays a pill in 28 px.
fn sample_radius(value: &str) -> Pixels {
    value
        .trim_end_matches("px")
        .parse::<f32>()
        .map_or(px(14.), |n| px(n.min(14.)))
}

pub(super) fn render(
    tokens: &Tokens,
    export: Option<&ExportShown>,
    include_videos: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mono = theme.mono_font_family.clone();
    let border = theme.border;
    let foreground = theme.foreground;
    let data = theme.primary;

    let ramps: Vec<AnyElement> = tokens
        .palette
        .iter()
        .enumerate()
        .map(|(ri, r)| {
            let (hex, _) = hex_and_alpha(&r.base);
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .items_baseline()
                        .text_xs()
                        .child(div().font_weight(FontWeight::MEDIUM).child(r.role.clone()))
                        .child(div().font_family(mono.clone()).text_color(muted).child(hex)),
                )
                .child(
                    h_flex()
                        .gap(px(2.))
                        .children(r.steps.iter().enumerate().map(|(si, c)| {
                            let copy = c.to_hex();
                            let name = format!("{}-{}", r.role, STEPS.get(si).unwrap_or(&""));
                            v_flex()
                                .id(("design-ramp", ri * 16 + si))
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .cursor_pointer()
                                .child(
                                    div()
                                        .h(px(24.))
                                        .rounded(px(3.))
                                        .border_1()
                                        .border_color(border)
                                        .bg(gpui_kit::rgba(rgba_u32(c))),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(muted)
                                        .text_center()
                                        .child(STEPS.get(si).copied().unwrap_or_default()),
                                )
                                .tooltip(move |window, cx| {
                                    gpui_kit::component::tooltip::Tooltip::new(format!(
                                        "{name} {copy}"
                                    ))
                                    .build(window, cx)
                                })
                                .on_click(cx.listener({
                                    let copy = c.to_hex();
                                    move |this, _: &ClickEvent, _, cx| {
                                        this.copy(copy.clone(), cx);
                                    }
                                }))
                        })),
                )
                .into_any_element()
        })
        .collect();

    let semantic: Vec<AnyElement> = tokens
        .semantic
        .iter()
        .enumerate()
        .map(|(ix, (name, c))| {
            let (hex, alpha) = hex_and_alpha(c);
            copy_row_at(
                ("design-semantic", ix),
                name.clone(),
                SEMANTIC_LABEL_WIDTH,
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(swatch(c, px(14.), cx))
                    .child(div().font_family(mono.clone()).child(hex))
                    .children(alpha.map(|a| div().text_color(muted).child(format!("· {a}")))),
                c.to_hex(),
                cx,
            )
        })
        .collect();

    let families: Vec<AnyElement> = tokens
        .families
        .iter()
        .enumerate()
        .map(|(ix, (role, fam))| {
            copy_row(
                ("design-family", ix),
                role.clone(),
                div().truncate().child(fam.clone()),
                fam.clone(),
                cx,
            )
        })
        .collect();

    let scale: Vec<AnyElement> = tokens
        .type_scale
        .iter()
        .enumerate()
        .map(|(ix, (name, size))| {
            copy_row(
                ("design-type", ix),
                format!("{name} · {size}px"),
                div()
                    .truncate()
                    .text_color(foreground)
                    .text_size(px((*size as f32).min(MAX_SAMPLE_PX)))
                    .line_height(relative(1.2))
                    .child("Sphinx of black quartz"),
                format!("{size}px"),
                cx,
            )
        })
        .collect();

    let largest = tokens
        .spacing
        .iter()
        .map(|(_, v)| *v)
        .fold(1., f64::max)
        .max(1.);
    let spacing: Vec<AnyElement> = tokens
        .spacing
        .iter()
        .enumerate()
        .map(|(ix, (name, v))| {
            let share = (*v / largest).clamp(0.02, 1.) as f32;
            copy_row(
                ("design-space", ix),
                format!("{name} · {v}px"),
                div()
                    .h(px(6.))
                    .w(relative(share))
                    .rounded(px(2.))
                    .bg(data.opacity(0.7)),
                format!("{v}px"),
                cx,
            )
        })
        .collect();

    let radius = h_flex()
        .gap_3()
        .flex_wrap()
        .children(tokens.radius.iter().enumerate().map(|(ix, (name, v))| {
            let copy = v.clone();
            v_flex()
                .id(("design-radius", ix))
                .gap_1()
                .items_center()
                .cursor_pointer()
                .child(
                    div()
                        .size(px(28.))
                        .border_1()
                        .border_color(foreground.opacity(0.6))
                        .rounded(sample_radius(v)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{name} {v}")),
                )
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.copy(copy.clone(), cx);
                }))
        }));

    let shadows: Vec<AnyElement> = tokens
        .shadows
        .iter()
        .enumerate()
        .map(|(ix, (n, v))| {
            copy_row(
                ("design-shadow", ix),
                n.clone(),
                div().truncate().font_family(mono.clone()).child(v.clone()),
                v.clone(),
                cx,
            )
        })
        .collect();

    let breakpoints = h_flex().gap_1().flex_wrap().children(
        tokens
            .breakpoints
            .iter()
            .map(|b| panel::tag(format!("{b} px"), cx)),
    );

    let variables: Vec<AnyElement> = tokens
        .variables
        .iter()
        .take(200)
        .enumerate()
        .map(|(ix, (n, v))| {
            let line = format!("{n}: {v};");
            copy_row(
                ("design-variable", ix),
                n.clone(),
                div().truncate().font_family(mono.clone()).child(v.clone()),
                line,
                cx,
            )
        })
        .collect();

    let nothing = tokens.palette.is_empty()
        && tokens.semantic.is_empty()
        && tokens.families.is_empty()
        && tokens.type_scale.is_empty()
        && tokens.spacing.is_empty()
        && tokens.radius.is_empty();

    let section = |title: &'static str, count: usize, body: AnyElement, cx: &App| {
        (count > 0).then(|| {
            panel::section(
                title,
                Some(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(count.to_string())
                        .into_any_element(),
                ),
                body,
                cx,
            )
        })
    };
    let palette_count = tokens.palette.len();
    let semantic_count = semantic.len();
    let families_count = families.len();
    let scale_count = scale.len();
    let spacing_count = spacing.len();
    let radius_count = tokens.radius.len();
    let shadow_count = shadows.len();
    let breakpoint_count = tokens.breakpoints.len();
    let variable_count = variables.len();
    v_flex()
        .gap_4()
        .child(export_block(export, include_videos, cx))
        .when(nothing, |el| {
            el.child(panel::empty(
                "No tokens derived",
                "The page sets too little color and type for a palette or a scale. The export \
                 still writes what was seen.",
                None,
                cx,
            ))
        })
        .children(section(
            "Palette",
            palette_count,
            v_flex().gap_3().children(ramps).into_any_element(),
            cx,
        ))
        .children(section(
            "Semantic colors",
            semantic_count,
            v_flex().children(semantic).into_any_element(),
            cx,
        ))
        .children(section(
            "Font families",
            families_count,
            v_flex().children(families).into_any_element(),
            cx,
        ))
        .children(section(
            "Type scale",
            scale_count,
            v_flex().children(scale).into_any_element(),
            cx,
        ))
        .children(section(
            "Spacing",
            spacing_count,
            v_flex().children(spacing).into_any_element(),
            cx,
        ))
        .children(section(
            "Radius",
            radius_count,
            radius.into_any_element(),
            cx,
        ))
        .children(section(
            "Shadows",
            shadow_count,
            v_flex().children(shadows).into_any_element(),
            cx,
        ))
        .children(section(
            "Breakpoints",
            breakpoint_count,
            breakpoints.into_any_element(),
            cx,
        ))
        .children(section(
            "Variables",
            variable_count,
            div()
                .id("design-variables")
                .max_h(px(240.))
                .overflow_y_scroll()
                .child(v_flex().children(variables))
                .into_any_element(),
            cx,
        ))
}

#[cfg(test)]
mod tests {
    use super::{export_lines, sample_radius};
    use crate::browser::design::{ExportDone, ExportShown};
    use gpui_kit::px;
    use std::path::PathBuf;

    fn shown(
        outcome: Option<Result<ExportDone, String>>,
        done: usize,
        total: usize,
    ) -> ExportShown {
        ExportShown {
            folder: PathBuf::from("/p/.nyawit/design/a.dev"),
            done,
            total,
            outcome,
        }
    }

    #[test]
    fn export_text_follows_the_job() {
        assert_eq!(
            export_lines(&shown(None, 18, 40)),
            ["Downloading assets… 18 / 40"]
        );
        let done = ExportDone {
            files: 30,
            failed: 12,
            skipped_videos: 0,
            screenshot_error: Some("off screen".into()),
        };
        let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
        assert_eq!(lines[0], "Exported 30 files to /p/.nyawit/design/a.dev");
        assert_eq!(lines[1], "12 of 40 assets couldn't be downloaded");
        assert_eq!(lines[2], "No screenshot: off screen");
        // Skipped videos are said apart, not counted as failures.
        let done = ExportDone {
            files: 30,
            failed: 0,
            skipped_videos: 3,
            screenshot_error: None,
        };
        let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
        assert_eq!(lines[1..], ["3 videos weren't included"]);
        let done = ExportDone {
            files: 30,
            failed: 2,
            skipped_videos: 1,
            screenshot_error: None,
        };
        let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
        assert_eq!(
            lines[1..],
            [
                "2 of 39 assets couldn't be downloaded",
                "1 video wasn't included"
            ]
        );
    }

    #[test]
    fn radius_samples_stay_readable() {
        assert_eq!(sample_radius("6px"), px(6.));
        assert_eq!(sample_radius("980px"), px(14.));
        assert_eq!(sample_radius("50%"), px(14.));
    }
}
