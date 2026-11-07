//! The page's favicon on its tab (`sawit_core::web::favicon`): when a page has loaded, its script
//! reports its icon `<link>` tags and Rust fetches, decodes and caches the icon per origin; a tab
//! brought back from the last run, with no page yet, has Rust read the page's HTML instead. The
//! tab strip draws it, or a globe when there is none.

use std::sync::Arc;

use gpui_kit::*;
use sawit_core::web::favicon::{self, Favicon, ICON_LINKS_JS};

use super::{BrowserView, FromPage};
use crate::rt;

/// The tab's favicon and the origin it is for.
#[derive(Default)]
pub(super) struct TabIcon {
    pub origin: Option<String>,
    pub image: Option<Arc<RenderImage>>,
}

/// The icon's pixels as GPUI draws them (BGRA).
pub(super) fn render_image(icon: &Favicon) -> Option<Arc<RenderImage>> {
    let mut bgra = icon.rgba.clone();
    for px in bgra.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
    let pixels = image::RgbaImage::from_raw(icon.width, icon.height, bgra)?;
    Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
}

impl BrowserView {
    /// The favicon to draw on the tab, if the page has one.
    pub fn favicon(&self) -> Option<Arc<RenderImage>> {
        self.icon.image.clone()
    }

    /// The address moved: an icon for another origin goes (the cached one of the new origin
    /// shows at once, if any).
    pub(super) fn favicon_url_changed(&mut self, url: &str) {
        let origin = sawit_core::web::origin(url);
        if origin == self.icon.origin {
            return;
        }
        self.icon.image = favicon::cached(url).and_then(|i| render_image(&i));
        self.icon.origin = origin;
    }

    /// The page has loaded: asks it for its icon links, then fetches the icon.
    pub(super) fn favicon_after_load(&mut self, cx: &mut Context<Self>) {
        let url = self.current_url();
        self.favicon_url_changed(&url);
        let Some(engine) = &self.engine else {
            self.fetch_favicon(None, cx);
            return;
        };
        let to_view = self.to_view.clone();
        let asked = engine.eval_with_callback(ICON_LINKS_JS, move |json| {
            let _ = to_view.unbounded_send(FromPage::IconLinks(json));
        });
        if asked.is_err() {
            self.fetch_favicon(None, cx);
        }
    }

    /// A tab brought back from the last run, with no page yet: its icon from its HTML.
    pub fn restored_favicon(&mut self, cx: &mut Context<Self>) {
        let url = self.current_url();
        self.favicon_url_changed(&url);
        self.fetch_favicon(None, cx);
    }

    /// The page answered with its icon links (`ICON_LINKS_JS`).
    pub(super) fn favicon_links(&mut self, json: String, cx: &mut Context<Self>) {
        self.fetch_favicon(favicon::parse_reported(&json), cx);
    }

    /// Fetches the icon of the page on show, from its reported `links` or its HTML.
    pub(super) fn fetch_favicon(
        &mut self,
        links: Option<Vec<sawit_core::web::head::Link>>,
        cx: &mut Context<Self>,
    ) {
        let url = self.current_url();
        if !url.starts_with("http") {
            self.icon = TabIcon::default();
            cx.notify();
            return;
        }
        let task = rt::spawn(cx, {
            let url = url.clone();
            async move { favicon::favicon(&url, links).await }
        });
        cx.spawn(async move |this, cx| {
            let Ok(icon) = task.await else { return };
            let _ = this.update(cx, |this, cx| {
                // Still on that origin.
                if sawit_core::web::origin(&this.current_url()) != sawit_core::web::origin(&url) {
                    return;
                }
                let image = icon.as_deref().and_then(render_image);
                if image.is_some() || this.icon.image.is_none() {
                    this.icon.image = image;
                }
                this.icon.origin = sawit_core::web::origin(&url);
                cx.notify();
            });
        })
        .detach();
    }
}
