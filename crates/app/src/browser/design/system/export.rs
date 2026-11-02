//! The System tab's export block: what an export writes, its button, its progress, and where it
//! went.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::progress::Progress;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::super::{DesignEvent, DesignPanel, ExportShown};

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
pub(super) fn export_block(
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
