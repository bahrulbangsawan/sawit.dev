//! PDF viewer: a virtual list of pages; only pages on screen are rendered
//! (at the display's pixel density) and a few are kept in memory.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::*;
use gpui_kit::component::*;
use gpui_kit::*;
use pgdesk_core::pdf::{Bitmap, page_sizes, render_page};

use super::image::file_name;
use super::toolbar;
use crate::rt;

/// Page width at 100%, in points.
const BASE_WIDTH: f32 = 820.;
const ZOOMS: [f32; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0];
/// Rendered pages kept in memory (each is a few MB at Retina density).
const CACHE_PAGES: usize = 8;
const GAP: f32 = 12.;

type PageKey = (usize, u32);

pub struct PdfView {
    pub path: PathBuf,
    sizes: Option<Vec<(f64, f64)>>,
    error: Option<String>,
    zoom_ix: usize,
    list: ListState,
    pages: HashMap<PageKey, Arc<RenderImage>>,
    recent: VecDeque<PageKey>,
    pending: HashSet<PageKey>,
}

impl PdfView {
    pub fn new(path: PathBuf, cx: &mut Context<Self>) -> Self {
        let source = path.clone();
        let rx = rt::spawn(cx, async move { page_sizes(&source) });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(Ok(sizes)) => {
                        this.list.reset(sizes.len());
                        this.sizes = Some(sizes);
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
            sizes: None,
            error: None,
            zoom_ix: 2,
            list: ListState::new(0, ListAlignment::Top, px(1200.)),
            pages: HashMap::new(),
            recent: VecDeque::new(),
            pending: HashSet::new(),
        }
    }

    pub fn title(&self) -> String {
        file_name(&self.path)
    }

    fn width(&self) -> f32 {
        BASE_WIDTH * ZOOMS[self.zoom_ix]
    }

    fn zoom(&mut self, delta: isize, cx: &mut Context<Self>) {
        let ix = (self.zoom_ix as isize + delta).clamp(0, ZOOMS.len() as isize - 1) as usize;
        if ix != self.zoom_ix {
            self.zoom_ix = ix;
            // Item heights changed; keep the reading position by page.
            let top = self.list.logical_scroll_top();
            self.list.reset(self.sizes.as_ref().map_or(0, Vec::len));
            self.list.scroll_to(top);
            cx.notify();
        }
    }

    fn request(&mut self, key: PageKey, cx: &mut Context<Self>) {
        if self.pages.contains_key(&key) || !self.pending.insert(key) {
            return;
        }
        let path = self.path.clone();
        let rx = rt::spawn(cx, async move { render_page(&path, key.0, key.1) });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.pending.remove(&key);
                match result {
                    Ok(Ok(bitmap)) => {
                        if let Some(image) = to_render_image(bitmap) {
                            this.pages.insert(key, Arc::new(image));
                            this.recent.push_back(key);
                            while this.recent.len() > CACHE_PAGES {
                                if let Some(old) = this.recent.pop_front() {
                                    this.pages.remove(&old);
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => this.error = Some(e.to_string()),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn render_page_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(&(pw, ph)) = self.sizes.as_ref().and_then(|s| s.get(ix)) else {
            return div().into_any_element();
        };
        let width = self.width();
        let height = width * (ph / pw) as f32;
        let pixels = (width * window.scale_factor()).round() as u32;
        let key = (ix, pixels);
        if let Some(pos) = self.recent.iter().position(|k| *k == key) {
            // Most recently seen goes last, so it is evicted last.
            self.recent.remove(pos);
            self.recent.push_back(key);
        }
        let page = self.pages.get(&key).cloned();
        if page.is_none() {
            self.request(key, cx);
        }
        div()
            .w_full()
            .flex()
            .justify_center()
            .pt(px(GAP))
            .child(
                div()
                    .w(px(width))
                    .h(px(height))
                    .flex_none()
                    .bg(gpui_kit::white())
                    .shadow_md()
                    .children(page.map(|image| img(image).size_full())),
            )
            .into_any_element()
    }
}

/// Wraps rendered pixels (already BGRA, as GPUI stores images) for `img()`.
fn to_render_image(bitmap: Bitmap) -> Option<RenderImage> {
    let buffer = ::image::RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.bgra)?;
    Some(RenderImage::new([::image::Frame::new(buffer)]))
}

impl Render for PdfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = self.sizes.as_ref().map(Vec::len);
        let bar = toolbar(self.title(), cx)
            .children(pages.map(|n| format!("{n} pages")))
            .child(
                Button::new("pdf-zoom-out")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Minus)
                    .tooltip("Zoom Out")
                    .disabled(self.zoom_ix == 0)
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(-1, cx))),
            )
            .child(
                div()
                    .min_w(px(40.))
                    .text_center()
                    .child(format!("{:.0}%", ZOOMS[self.zoom_ix] * 100.)),
            )
            .child(
                Button::new("pdf-zoom-in")
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Plus)
                    .tooltip("Zoom In")
                    .disabled(self.zoom_ix + 1 == ZOOMS.len())
                    .on_click(cx.listener(|this, _, _, cx| this.zoom(1, cx))),
            );
        let body = if let Some(e) = &self.error {
            div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().danger)
                .child(e.clone())
                .into_any_element()
        } else if self.sizes.is_none() {
            div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("Loading…")
                .into_any_element()
        } else {
            let this = cx.entity();
            list(self.list.clone(), move |ix, window, cx| {
                this.update(cx, |view, cx| view.render_page_item(ix, window, cx))
            })
            .size_full()
            .pb(px(GAP))
            .into_any_element()
        };
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(bar)
            .child(div().flex_1().min_h_0().bg(cx.theme().muted).child(body))
    }
}
