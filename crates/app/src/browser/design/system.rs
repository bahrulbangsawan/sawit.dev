//! The System segment: the derived design tokens, and the export.

use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::site_design::color::Rgba;
use sawit_core::site_design::tokens::Tokens;

use super::colors::rgba_u32;
use super::{DesignEvent, DesignPanel, ExportShown};

/// The largest size a type-scale line is drawn at.
const MAX_SAMPLE_PX: f32 = 48.;

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
                    done.failed, e.total
                ));
            }
            if let Some(err) = &done.screenshot_error {
                lines.push(format!("No screenshot: {err}"));
            }
            lines
        }
    }
}

fn swatch(c: &Rgba, size: Pixels, border: Hsla) -> impl IntoElement {
    div()
        .size(size)
        .flex_none()
        .rounded(px(3.))
        .border_1()
        .border_color(border)
        .bg(gpui_kit::rgba(rgba_u32(c)))
}

fn heading(text: &'static str, muted: Hsla) -> impl IntoElement {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(muted)
        .pt_1()
        .child(text)
}

fn line(label: String, value: String, muted: Hsla) -> impl IntoElement {
    h_flex()
        .gap_2()
        .text_xs()
        .child(
            div()
                .w(px(80.))
                .flex_none()
                .truncate()
                .text_color(muted)
                .child(label),
        )
        .child(div().flex_1().min_w_0().child(value))
}

fn export_block(
    export: Option<&ExportShown>,
    include_videos: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let running = export.is_some_and(|e| e.outcome.is_none());
    let finished = export.and_then(|e| match &e.outcome {
        Some(Ok(_)) => Some(e.folder.clone()),
        _ => None,
    });
    v_flex()
        .gap_1p5()
        .child(
            h_flex()
                .gap_1()
                .child(
                    Button::new("design-export")
                        .xsmall()
                        .label("Export")
                        .disabled(running)
                        .tooltip("Write DESIGN.md, tokens and assets to the export folder")
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
                )
                .when_some(finished, |el, folder| {
                    let reveal = folder.join("DESIGN.md");
                    let path = folder.display().to_string();
                    el.child(
                        Button::new("design-export-reveal")
                            .xsmall()
                            .label("Reveal in Finder")
                            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                cx.emit(DesignEvent::Reveal(reveal.clone()));
                            })),
                    )
                    .child(
                        Button::new("design-export-copy")
                            .xsmall()
                            .label("Copy path")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.copy(path.clone(), cx);
                            })),
                    )
                }),
        )
        .children(export.map(|e| {
            v_flex().gap_0p5().children(
                export_lines(e)
                    .into_iter()
                    .map(|l| div().text_xs().text_color(muted).child(l)),
            )
        }))
}

pub(super) fn render(
    tokens: &Tokens,
    export: Option<&ExportShown>,
    include_videos: bool,
    cx: &mut Context<DesignPanel>,
) -> impl IntoElement {
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let mono_font = theme.mono_font_family.clone();
    let none = |text: &'static str| div().text_xs().text_color(muted).child(text);

    let ramps = tokens.palette.iter().map(|r| {
        v_flex()
            .gap_0p5()
            .child(
                div()
                    .text_xs()
                    .child(format!("{} {}", r.role, r.base.to_hex())),
            )
            .child(
                h_flex().children(
                    r.steps
                        .iter()
                        .map(|c| div().flex_1().h(px(20.)).bg(gpui_kit::rgba(rgba_u32(c)))),
                ),
            )
    });
    let semantic = h_flex()
        .gap_2()
        .flex_wrap()
        .children(tokens.semantic.iter().map(|(name, c)| {
            h_flex()
                .gap_1()
                .items_center()
                .text_xs()
                .child(swatch(c, px(14.), border))
                .child(format!("{name} {}", c.to_hex()))
        }));
    let scale = v_flex()
        .gap_0p5()
        .children(tokens.type_scale.iter().map(|(name, size)| {
            h_flex()
                .gap_2()
                .items_baseline()
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{name} {size}")),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px((*size as f32).min(MAX_SAMPLE_PX)))
                        .child("Sphinx of black quartz"),
                )
        }));
    let spacing = v_flex()
        .gap_0p5()
        .children(tokens.spacing.iter().map(|(name, v)| {
            h_flex()
                .gap_2()
                .items_center()
                .text_xs()
                .child(
                    div()
                        .w(px(48.))
                        .flex_none()
                        .text_color(muted)
                        .child(format!("{name} {v}")),
                )
                .child(
                    div()
                        .h(px(6.))
                        .w(px((*v as f32).clamp(1., 200.)))
                        .rounded(px(1.))
                        .bg(theme.primary),
                )
        }));
    let radius = h_flex()
        .gap_2()
        .flex_wrap()
        .children(tokens.radius.iter().map(|(name, v)| {
            let r = v
                .trim_end_matches("px")
                .parse::<f32>()
                .map_or(px(4.), |n| px(n.min(24.)));
            v_flex()
                .gap_0p5()
                .items_center()
                .child(
                    div()
                        .size(px(28.))
                        .border_1()
                        .border_color(theme.foreground)
                        .rounded(r),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("{name} {v}")),
                )
        }));
    let variables = div()
        .id("design-variables")
        .max_h(px(160.))
        .overflow_y_scroll()
        .font_family(mono_font)
        .text_xs()
        .children(
            tokens
                .variables
                .iter()
                .take(200)
                .map(|(n, v)| div().truncate().child(format!("{n}: {v}"))),
        );

    v_flex()
        .gap_2()
        .child(export_block(export, include_videos, cx))
        .child(heading("Palette", muted))
        .children(ramps)
        .when(tokens.palette.is_empty(), |el| {
            el.child(none("No palette derived."))
        })
        .when(!tokens.semantic.is_empty(), |el| el.child(semantic))
        .child(heading("Font families", muted))
        .children(
            tokens
                .families
                .iter()
                .map(|(role, fam)| line(role.clone(), fam.clone(), muted)),
        )
        .when(!tokens.type_scale.is_empty(), |el| {
            el.child(heading("Type scale", muted)).child(scale)
        })
        .when(!tokens.spacing.is_empty(), |el| {
            el.child(heading("Spacing", muted)).child(spacing)
        })
        .when(!tokens.radius.is_empty(), |el| {
            el.child(heading("Radius", muted)).child(radius)
        })
        .when(!tokens.shadows.is_empty(), |el| {
            el.child(heading("Shadows", muted)).children(
                tokens
                    .shadows
                    .iter()
                    .map(|(n, v)| line(n.clone(), v.clone(), muted)),
            )
        })
        .when(!tokens.breakpoints.is_empty(), |el| {
            el.child(heading("Breakpoints", muted)).child(
                div().text_xs().child(
                    tokens
                        .breakpoints
                        .iter()
                        .map(|b| format!("{b} px"))
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
            )
        })
        .when(!tokens.variables.is_empty(), |el| {
            el.child(heading("Variables", muted)).child(variables)
        })
}

#[cfg(test)]
mod tests {
    use super::export_lines;
    use crate::browser::design::{ExportDone, ExportShown};
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
            screenshot_error: Some("off screen".into()),
        };
        let lines = export_lines(&shown(Some(Ok(done)), 40, 40));
        assert_eq!(lines[0], "Exported 30 files to /p/.nyawit/design/a.dev");
        assert_eq!(lines[1], "12 of 40 assets couldn't be downloaded");
        assert_eq!(lines[2], "No screenshot: off screen");
    }
}
