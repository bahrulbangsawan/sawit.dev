//! Image viewer: fit to the pane or actual size, with pixel size and file
//! size. HEIC/AVIF are shown through a cached PNG copy.

use std::path::PathBuf;

use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use pgdesk_core::instances::AppPaths;
use pgdesk_core::media::{displayable, pixel_size};

use super::{file_size_label, toolbar};
use crate::rt;

pub struct ImageView {
    pub path: PathBuf,
    /// What GPUI draws (the file, or its PNG copy).
    shown: Option<PathBuf>,
    size: Option<(u32, u32)>,
    bytes: Option<u64>,
    actual_size: bool,
    error: Option<String>,
}

impl ImageView {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let source = path.clone();
        let cache = AppPaths::default_macos().cache_dir();
        let rx = rt::spawn(cx, async move {
            let shown = displayable(&source, &cache).await?;
            let size = pixel_size(&shown).await.ok();
            let bytes = std::fs::metadata(&source).map(|m| m.len()).ok();
            Ok::<_, pgdesk_core::CoreError>((shown, size, bytes))
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok((shown, size, bytes))) => {
                        this.shown = Some(shown);
                        this.size = size;
                        this.bytes = bytes;
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => this.error = Some("Runtime stopped.".into()),
                }
                cx.notify();
            });
        })
        .detach();
        Self {
            path,
            shown: None,
            size: None,
            bytes: None,
            actual_size: false,
            error: None,
        }
    }

    pub fn title(&self) -> String {
        file_name(&self.path)
    }
}

pub(super) fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl Render for ImageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let facts = [
            self.size.map(|(w, h)| format!("{w} × {h}")),
            self.bytes.map(file_size_label),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let bar = toolbar(self.title(), cx)
            .child(facts)
            .child(
                Button::new("img-fit")
                    .ghost()
                    .xsmall()
                    .label("Fit")
                    .selected(!self.actual_size)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.actual_size = false;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("img-actual")
                    .ghost()
                    .xsmall()
                    .label("100%")
                    .selected(self.actual_size)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.actual_size = true;
                        cx.notify();
                    })),
            );
        let body = match (&self.shown, &self.error) {
            (_, Some(e)) => div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(e.clone())
                .into_any_element(),
            (None, None) => div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Loading…")
                .into_any_element(),
            (Some(shown), None) => {
                let image = img(shown.clone());
                if self.actual_size {
                    // Natural size (in points), scrollable both ways.
                    let image = match self.size {
                        Some((w, h)) => image.w(px(w as f32)).h(px(h as f32)),
                        None => image,
                    };
                    div()
                        .id("img-scroll")
                        .size_full()
                        .overflow_scroll()
                        .p_4()
                        .child(image.flex_none())
                        .into_any_element()
                } else {
                    div()
                        .size_full()
                        .p_4()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            image
                                .max_w_full()
                                .max_h_full()
                                // Intrinsic size, shrunk to fit: small
                                // images are never upscaled.
                                .object_fit(ObjectFit::Contain),
                        )
                        .into_any_element()
                }
            }
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(bar)
            .child(div().flex_1().min_h_0().bg(cx.theme().muted).child(body))
    }
}
