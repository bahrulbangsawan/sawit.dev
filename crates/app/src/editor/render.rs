//! Drawing the editor: its toolbar, the code, and the preview beside it or in its place.

use std::path::Path;
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::input::Editor;
use gpui_kit::component::text::TextView;
use gpui_kit::component::*;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use sawit_core::files::{FileKind, language_label};
use sawit_core::format::tool_for;

use super::{FileEditor, Format, ViewMode, diagrams};
use crate::code_actions::code_block_actions;

impl FileEditor {
    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let muted = cx.theme().muted_foreground;
        let crumbs = breadcrumb(&self.path, self.root.as_deref());
        let last = crumbs.len().saturating_sub(1);
        let cursor = self.state.as_ref().map(|s| s.read(cx).cursor_position());
        let mode_button =
            |id: &'static str, icon: Lucide, tooltip: &'static str, mode: ViewMode, current| {
                Button::new(id)
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(tooltip)
                    .selected(current == mode)
            };
        h_flex()
            .h(px(28.))
            .px_3()
            .gap_2()
            .text_xs()
            .text_color(muted)
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .overflow_x_hidden()
                    .children(crumbs.into_iter().enumerate().map(|(ix, part)| {
                        h_flex()
                            .gap_1()
                            .flex_none()
                            .when(ix > 0, |el| el.child("›"))
                            .child(
                                div()
                                    .when(ix == last, |el| el.text_color(cx.theme().foreground))
                                    .child(part),
                            )
                    })),
            )
            .when(self.previewable(), |el| {
                let current = self.mode;
                el.child(
                    h_flex()
                        .gap_0p5()
                        .child(
                            mode_button("mode-code", Lucide::Code, "Code", ViewMode::Code, current)
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Code, cx)),
                                ),
                        )
                        .child(
                            mode_button(
                                "mode-split",
                                Lucide::Columns2,
                                "Side by Side",
                                ViewMode::Split,
                                current,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Split, cx)),
                            ),
                        )
                        .child(
                            mode_button(
                                "mode-preview",
                                Lucide::Eye,
                                "Preview",
                                ViewMode::Preview,
                                current,
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_mode(ViewMode::Preview, cx)),
                            ),
                        ),
                )
            })
            .when_some(cursor, |el, pos| {
                el.child(format!("Ln {}, Col {}", pos.line + 1, pos.character + 1))
            })
            .child(language_label(&self.language))
            .children(self.download_button(cx))
            .when(self.read_only, |el| {
                el.child(Icon::new(Lucide::Lock).xsmall())
            })
            .when(
                tool_for(&self.path).is_some() && self.state.is_some(),
                |el| {
                    el.child(
                        Button::new("format")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::WandSparkles)
                            .tooltip_with_action("Format Document", &Format, Some("FileEditor"))
                            .loading(self.formatting)
                            .disabled(self.read_only)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.format(&Format, window, cx)),
                            ),
                    )
                },
            )
    }

    fn render_preview(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let id = ElementId::Name(format!("preview\u{0}{}", self.path.display()).into());
        let dir = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let body = match self.kind {
            FileKind::Svg => {
                // Built once per preview text, not per frame: a fresh Image
                // copies and hashes the whole file every render.
                let image = match &self.svg_image {
                    Some((text, image)) if *text == self.preview => image.clone(),
                    _ => {
                        let image = Arc::new(Image::from_bytes(
                            ImageFormat::Svg,
                            self.preview.as_bytes().to_vec(),
                        ));
                        self.svg_image = Some((self.preview.clone(), image.clone()));
                        image
                    }
                };
                return div()
                    .size_full()
                    .p_6()
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(cx.theme().muted)
                    .child(img(image).max_w_full().max_h_full())
                    .into_any_element();
            }
            FileKind::Mermaid => return self.render_diagram(cx),
            FileKind::Html => TextView::html(id, self.preview.clone()),
            _ => TextView::markdown(id, self.diagrams.markdown.clone()),
        };
        let images = self.diagrams.images.clone();
        let this = cx.entity().downgrade();
        div()
            .size_full()
            .bg(cx.theme().background)
            .child(
                body.scrollable(true)
                    .selectable(true)
                    .code_block_actions(code_block_actions)
                    .image_source(move |uri| {
                        diagrams::Diagrams::image(&images, uri)
                            .unwrap_or_else(|| resolve_image(&dir, uri))
                    })
                    .on_link_click(move |url, _, _, cx| {
                        if url.starts_with("http://") || url.starts_with("https://") {
                            cx.open_url(url);
                        } else {
                            let _ = this.update(cx, |this, cx| this.diagram_link(url, cx));
                        }
                    })
                    .p_6(),
            )
            .into_any_element()
    }
}

/// Images in previews: URLs and data URIs as-is, everything else relative
/// to the file's folder.
pub(super) fn resolve_image(dir: &Path, uri: &SharedUri) -> ImageSource {
    let s = uri.as_ref();
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("data:") {
        return ImageSource::from(uri.clone());
    }
    let local = s.strip_prefix("file://").unwrap_or(s);
    let path = Path::new(local);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    };
    ImageSource::from(path)
}

/// Path parts shown above the editor: relative to the project when inside it.
pub(super) fn breadcrumb(path: &Path, root: Option<&Path>) -> Vec<String> {
    let shown = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    shown
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .filter(|c| c != "/")
        .collect()
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title();
        let editor = self.state.clone().map(|state| {
            div().size_full().child(
                Editor::new(&state)
                    // A screen reader names the text by its file.
                    .aria_label(title)
                    .size_full()
                    .bordered(false)
                    .readonly(self.read_only),
            )
        });
        let mode = if self.previewable() {
            self.mode
        } else {
            ViewMode::Code
        };
        let body: AnyElement = match (editor, mode) {
            (None, _) => div().into_any_element(),
            (Some(editor), ViewMode::Code) => editor.into_any_element(),
            (Some(_), ViewMode::Preview) => self.render_preview(cx),
            (Some(editor), ViewMode::Split) => h_flex()
                .size_full()
                .child(div().flex_1().min_w_0().h_full().child(editor))
                .child(div().w(px(1.)).h_full().bg(cx.theme().border))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .child(self.render_preview(cx)),
                )
                .into_any_element(),
        };
        v_flex()
            .key_context("FileEditor")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::format))
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_toolbar(cx))
            .when_some(self.notice.clone(), |el, n| {
                el.child(
                    div()
                        .px_3()
                        .py_1p5()
                        .text_xs()
                        .whitespace_normal()
                        .text_color(if n.error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .when(n.error, |el| {
                            el.font_family(cx.theme().mono_font_family.clone())
                        })
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .child(n.text),
                )
            })
            .when(self.loading, |el| {
                el.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Opening…"),
                )
            })
            .child(div().flex_1().min_h_0().child(body))
    }
}
