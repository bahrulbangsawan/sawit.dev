//! The capture bar under the toolbar: what is being captured, the recording with Stop, or the
//! capture made with Copy, Reveal in Finder and Open.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::browser::capture::ImageFormat;

use super::super::super::BrowserView;
use super::super::record::RecordMode;
use super::Bar;
use crate::menu_icons;

impl BrowserView {
    /// The capture bar: what is being captured, the recording with Stop, or the capture made.
    pub(in crate::browser) fn capture_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme();
        let (muted, border, bg, danger) = (
            theme.muted_foreground,
            theme.border,
            theme.secondary,
            theme.danger,
        );
        let bar = || {
            h_flex()
                .id("browser-capture-bar")
                .h(px(44.))
                .flex_none()
                .pl_3()
                .pr_2()
                .gap_2()
                .items_center()
                .text_xs()
                .bg(bg)
                .border_b_1()
                .border_color(border)
        };
        let meta = |title: SharedString, detail: Option<SharedString>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(title),
                )
                .children(detail.map(|d| {
                    // One line in the bar; the tooltip has all of it (a long encoder error).
                    let tip = d.clone();
                    div()
                        .id("browser-capture-detail")
                        .text_color(muted)
                        .truncate()
                        .tooltip(move |window, cx| {
                            gpui_kit::component::tooltip::Tooltip::new(tip.clone())
                                .build(window, cx)
                        })
                        .child(d)
                }))
        };
        let close = Button::new("browser-capture-close")
            .ghost()
            .xsmall()
            .icon(menu_icons::CLOSE)
            .accessibility_label("Close the Capture Bar")
            .tooltip("Close")
            .on_click(cx.listener(|this, _, _, cx| {
                this.capture_ui.bar = Bar::Hidden;
                cx.notify();
            }));
        if let Some(r) = &self.recorder.current {
            let what = match r.mode {
                RecordMode::FullPage => "Recording the full page",
                RecordMode::Element => "Recording an element",
                RecordMode::Region => "Recording a region",
                RecordMode::View => "Recording the visible area",
            };
            let time = self.recording_time().unwrap_or_default();
            let cap = r.max.as_secs();
            return Some(
                bar()
                    .child(div().size(px(8.)).flex_none().rounded_full().bg(danger))
                    .child(meta(
                        format!("{what} · {time}").into(),
                        Some(
                            format!(
                                "{} × {} {} · stops by itself at {}:{:02}",
                                r.width,
                                r.height,
                                r.format.extension().to_uppercase(),
                                cap / 60,
                                cap % 60
                            )
                            .into(),
                        ),
                    ))
                    .child(
                        Button::new("browser-capture-bar-stop")
                            .small()
                            .danger()
                            .icon(Lucide::Square)
                            .label("Stop Recording")
                            .on_click(cx.listener(|this, _, _, cx| this.ui_stop(cx))),
                    )
                    .into_any_element(),
            );
        }
        match &self.capture_ui.bar {
            Bar::Hidden => None,
            Bar::Working(text) => Some(
                bar()
                    .child(
                        gpui_kit::component::spinner::Spinner::new()
                            .xsmall()
                            .color(muted),
                    )
                    .child(meta(text.clone(), None))
                    .into_any_element(),
            ),
            Bar::Failed(e) => Some(
                bar()
                    .child(Icon::new(Lucide::CircleX).small().text_color(danger))
                    .child(meta(
                        "The capture didn't work".into(),
                        Some(format!("{e} Try again from the camera menu.").into()),
                    ))
                    .child(close)
                    .into_any_element(),
            ),
            Bar::Done(done) => {
                let name: SharedString = if done.several > 1 {
                    format!("{} screenshots", done.several).into()
                } else {
                    done.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                        .into()
                };
                let path = done.path.clone();
                let reveal = path.clone();
                let open = path.clone();
                let video = done.video;
                Some(
                    bar()
                        .child(match &done.thumb {
                            Some(image) => div()
                                .w(px(48.))
                                .h(px(30.))
                                .flex_none()
                                .rounded(px(4.))
                                .overflow_hidden()
                                .border_1()
                                .border_color(border)
                                .child(img(image.clone()).size_full().object_fit(ObjectFit::Cover))
                                .into_any_element(),
                            None => Icon::new(if video {
                                Lucide::Film
                            } else {
                                menu_icons::IMAGE
                            })
                            .small()
                            .text_color(muted)
                            .into_any_element(),
                        })
                        .child(meta(name, Some(done.detail.clone())))
                        .when(done.several <= 1, |el| {
                            el.child(
                                Button::new("browser-capture-copy")
                                    .xsmall()
                                    .outline()
                                    .icon(menu_icons::COPY)
                                    .label("Copy")
                                    .tooltip(if video {
                                        "Copy the file's path"
                                    } else {
                                        "Copy the image"
                                    })
                                    .on_click(move |_, _, cx| copy(&path, video, cx)),
                            )
                        })
                        .child(
                            Button::new("browser-capture-reveal")
                                .xsmall()
                                .outline()
                                .icon(menu_icons::REVEAL)
                                .label("Reveal in Finder")
                                .on_click(move |_, _, cx| cx.reveal_path(&reveal)),
                        )
                        .when(done.several <= 1, |el| {
                            el.child(
                                Button::new("browser-capture-open")
                                    .xsmall()
                                    .outline()
                                    .icon(menu_icons::OPEN_EXTERNAL)
                                    .label("Open")
                                    .on_click(move |_, _, cx| cx.open_with_system(&open)),
                            )
                        })
                        .child(close)
                        .into_any_element(),
                )
            }
        }
    }
}

/// Copy: the image itself, or a video's path.
fn copy(path: &std::path::Path, video: bool, cx: &mut App) {
    if !video && let Ok(bytes) = std::fs::read(path) {
        let format = if path.extension().is_some_and(|e| e == "jpg") {
            ImageFormat::Jpeg
        } else {
            ImageFormat::Png
        };
        let kind = match format {
            ImageFormat::Jpeg => gpui_kit::ImageFormat::Jpeg,
            _ => gpui_kit::ImageFormat::Png,
        };
        cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(kind, bytes)));
        return;
    }
    cx.write_to_clipboard(ClipboardItem::new_string(path.display().to_string()));
}
