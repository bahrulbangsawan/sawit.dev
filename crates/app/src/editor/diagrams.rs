//! Mermaid diagrams in previews, drawn by Pele (`sawit_core::mermaid`):
//! ```` ```mermaid ```` blocks in Markdown become the diagram plus a
//! "Save Diagram as SVG…" link, and a `.mmd` file previews as its diagram with a
//! download button in the toolbar. Colors follow the app theme.

use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::files::FileKind;
use sawit_core::mermaid::{self, Diagram, Palette};

use super::{FileEditor, error};
use crate::rt;

#[derive(Default)]
pub(super) struct Diagrams {
    /// The Markdown the preview shows: fences swapped for diagram images.
    pub markdown: SharedString,
    /// Per fence (one for a `.mmd` file); `None` where it couldn't be drawn.
    pub images: Arc<Vec<Option<Arc<Image>>>>,
    svgs: Vec<Option<String>>,
    /// Why a `.mmd` file's diagram can't be drawn (the last good one stays).
    pub error: Option<String>,
    /// Bumped per render so a slow result never replaces a newer one.
    generation: u64,
}

impl Diagrams {
    /// For the preview's `image_source`: the diagram behind a
    /// `sawit-mermaid:<index>` URL.
    pub fn image(images: &[Option<Arc<Image>>], uri: &str) -> Option<ImageSource> {
        let ix = mermaid::index_in(uri, mermaid::IMAGE_SCHEME)?;
        images.get(ix)?.clone().map(ImageSource::Image)
    }
}

/// Pele's colors from the app theme.
pub(crate) fn palette(cx: &App) -> Palette {
    let theme = cx.theme();
    let bg = theme.background;
    Palette {
        background: hex(bg, bg),
        foreground: hex(theme.foreground, bg),
        muted: hex(theme.muted_foreground, bg),
        line: hex(theme.foreground.opacity(0.75), bg),
        surface: hex(theme.secondary, bg),
        surface_alt: hex(theme.accent, bg),
        border: hex(theme.muted_foreground.opacity(0.6), bg),
        accent: hex(theme.primary, bg),
        critical: hex(theme.danger, bg),
    }
}

/// `#rrggbb` of `color` composited over `bg`.
fn hex(color: Hsla, bg: Hsla) -> String {
    let (c, b) = (Rgba::from(color), Rgba::from(bg));
    let a = c.a.clamp(0., 1.);
    let channel = |c: f32, b: f32| ((c * a + b * (1. - a)) * 255.).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(c.r, b.r),
        channel(c.g, b.g),
        channel(c.b, b.b)
    )
}

/// Renders every diagram in `text` (the whole text for a `.mmd` file).
fn render_all(
    text: &str,
    whole_file: bool,
    palette: &Palette,
) -> (String, Vec<Result<Diagram, String>>) {
    if whole_file {
        return (String::new(), vec![mermaid::render(text, palette)]);
    }
    let fences = mermaid::fences(text);
    let results: Vec<_> = fences
        .iter()
        .map(|f| mermaid::render(&f.source, palette))
        .collect();
    (mermaid::rewrite_markdown(text, &fences, &results), results)
}

impl FileEditor {
    /// Re-renders the diagrams after `self.preview` changed.
    pub(super) fn refresh_diagrams(&mut self, cx: &mut Context<Self>) {
        let whole_file = self.kind == FileKind::Mermaid;
        if self.kind != FileKind::Markdown && !whole_file {
            return;
        }
        let text = self.preview.clone();
        self.diagrams.generation += 1;
        let generation = self.diagrams.generation;
        if !whole_file && mermaid::fences(&text).is_empty() {
            self.diagrams.markdown = text;
            self.diagrams.images = Arc::default();
            self.diagrams.svgs.clear();
            return;
        }
        // Until the first render lands, show the source as plain Markdown.
        if self.diagrams.markdown.is_empty() {
            self.diagrams.markdown = text.clone();
        }
        let palette = palette(cx);
        let rx = rt::spawn_blocking(cx, move || render_all(&text, whole_file, &palette));
        cx.spawn(async move |this, cx| {
            let Ok((markdown, results)) = rx.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let d = &mut this.diagrams;
                if d.generation != generation {
                    return;
                }
                if whole_file && let Some(Err(e)) = results.first() {
                    d.error = Some(e.clone());
                    cx.notify();
                    return;
                }
                d.error = None;
                d.markdown = markdown.into();
                d.svgs = results.into_iter().map(|r| r.ok().map(|d| d.svg)).collect();
                d.images = Arc::new(
                    d.svgs
                        .iter()
                        .map(|svg| {
                            svg.as_ref().map(|svg| {
                                Arc::new(Image::from_bytes(
                                    ImageFormat::Svg,
                                    svg.as_bytes().to_vec(),
                                ))
                            })
                        })
                        .collect(),
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// Handles a `sawit-mermaid-save:<index>` link; false for other URLs.
    pub(super) fn diagram_link(&mut self, url: &str, cx: &mut Context<Self>) -> bool {
        match mermaid::index_in(url, mermaid::SAVE_SCHEME) {
            Some(ix) => {
                self.save_diagram(ix, cx);
                true
            }
            None => false,
        }
    }

    /// Asks where to save diagram `ix` and writes it as `.svg`.
    pub(super) fn save_diagram(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(Some(svg)) = self.diagrams.svgs.get(ix).cloned() else {
            return;
        };
        let stem = self
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "diagram".into());
        let name = if self.kind == FileKind::Mermaid {
            format!("{stem}.svg")
        } else {
            format!("{stem}-diagram-{}.svg", ix + 1)
        };
        let dir = self
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let picked = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let Ok(rx) = this.update(cx, |_, cx| {
                rt::spawn(cx, async move { std::fs::write(&target, svg) })
            }) else {
                return;
            };
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match result {
                    Ok(Ok(())) => this.flash(format!("Saved {name}."), cx),
                    Ok(Err(e)) => this.notice = Some(error(format!("Unable to save {name}. {e}"))),
                    Err(_) => this.notice = Some(error("Unable to save. Try again.".into())),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Toolbar button that saves a `.mmd` file's diagram.
    pub(super) fn download_button(&self, cx: &mut Context<Self>) -> Option<Button> {
        let ready = self.diagrams.svgs.first().is_some_and(Option::is_some);
        (self.kind == FileKind::Mermaid && ready).then(|| {
            Button::new("download-svg")
                .ghost()
                .xsmall()
                .icon(Lucide::Download)
                .tooltip("Save Diagram as SVG…")
                .on_click(cx.listener(|this, _, _, cx| this.save_diagram(0, cx)))
        })
    }

    /// A `.mmd` file's preview: the diagram, scaled down to fit, with the
    /// error above it while the source can't be drawn.
    pub(super) fn render_diagram(&self, cx: &mut Context<Self>) -> AnyElement {
        let image = self.diagrams.images.first().cloned().flatten();
        v_flex()
            .id("diagram")
            .size_full()
            .overflow_y_scroll()
            .bg(cx.theme().background)
            .when_some(self.diagrams.error.clone(), |el, e| {
                el.child(
                    div()
                        .px_4()
                        .py_2()
                        .text_xs()
                        .whitespace_normal()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(cx.theme().danger)
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(e),
                )
            })
            .child(
                div()
                    .w_full()
                    .p_6()
                    .flex()
                    .justify_center()
                    .when(self.diagrams.error.is_some(), |el| el.opacity(0.4))
                    .when_some(image, |el, image| el.child(img(image).max_w_full())),
            )
            .into_any_element()
    }
}
