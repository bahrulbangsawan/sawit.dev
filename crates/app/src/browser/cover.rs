//! The page under something GPUI draws over it. The webview draws above the window's drawing, so a
//! menu, popover, dialog or notification opened over the page (the account menu, the footer's
//! Pages menu, a tab's menu) would open unseen behind it. While anything covers the page
//! ([`crate::native_cover`]) the webview steps back, and a picture of the page as it was takes
//! its place; the webview comes back as soon as nothing covers it.

use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui_kit::*;
use sawit_core::browser::ViewportMode;

use super::BrowserView;
use super::engine::web_frame::{self, Target};
use super::viewport::{frame, page_frame};

/// How long the page waits for its picture before it steps back without one: the menu shouldn't
/// stay hidden longer than that.
const PICTURE_WAIT: Duration = Duration::from_millis(250);

/// The page while something covers it.
#[derive(Default)]
pub(super) struct Frozen {
    /// The page as it was when something came over it.
    image: Option<Arc<RenderImage>>,
    /// The webview is hidden for what covers it.
    pub hidden: bool,
    /// Pictures no longer shown, which leave GPUI's atlas at the next paint.
    stale: Vec<Arc<RenderImage>>,
}

impl BrowserView {
    /// The watch over the page, put last in the page's element so only what GPUI draws above
    /// the whole tab counts.
    pub(super) fn cover_watch(cx: &Context<Self>) -> impl IntoElement + use<> {
        crate::native_cover::watch(
            cx.entity().downgrade(),
            |this: &mut Self| &mut this.cover,
            Self::covered_changed,
        )
    }

    /// Something came over the page, or what covered it went. Over it: the page's picture is
    /// taken, then the webview hides with the picture in its place (or hides without one once
    /// [`PICTURE_WAIT`] is over). Gone: the webview shows again.
    fn covered_changed(&mut self, covered: bool, _: &mut Window, cx: &mut Context<Self>) {
        if !covered {
            self.unfreeze(cx);
            return;
        }
        let (Some(engine), Some(bounds)) = (self.engine.clone(), self.bounds) else {
            return;
        };
        if !self.page_shown() {
            return;
        }
        let width = f64::from(page_frame(frame(bounds), &self.viewport).0.width).max(1.);
        let (reply, answer) = oneshot::channel();
        engine.snapshot_cg(width, reply);
        cx.spawn(async move |this, cx| {
            let image = match answer.await {
                Ok(Ok(snap)) => {
                    cx.background_spawn(async move {
                        let space = web_frame::color_space(Target::Srgb)?;
                        let (w, h, bgra) = web_frame::to_bgra(&snap, &space)?;
                        let pixels = image::RgbaImage::from_raw(w, h, bgra)?;
                        Some(Arc::new(RenderImage::new([image::Frame::new(pixels)])))
                    })
                    .await
                }
                _ => None,
            };
            let _ = this.update(cx, |this, cx| this.step_back(image, cx));
        })
        .detach();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PICTURE_WAIT).await;
            let _ = this.update(cx, |this, cx| {
                if !this.frozen.hidden {
                    this.step_back(None, cx);
                }
            });
        })
        .detach();
    }

    /// Hides the webview while something still covers the page, with `image` of the page, when
    /// there is one, in its place.
    fn step_back(&mut self, image: Option<Arc<RenderImage>>, cx: &mut Context<Self>) {
        if !self.cover.covered {
            if let Some(image) = image {
                self.frozen.stale.push(image);
            }
            return;
        }
        if let Some(image) = image
            && let Some(old) = self.frozen.image.replace(image)
        {
            self.frozen.stale.push(old);
        }
        if !self.frozen.hidden {
            let was = self.page_shown();
            self.frozen.hidden = true;
            self.show_page(was);
        }
        cx.notify();
    }

    /// Shows the webview again, its picture gone.
    fn unfreeze(&mut self, cx: &mut Context<Self>) {
        let was = self.page_shown();
        self.frozen.hidden = false;
        if let Some(old) = self.frozen.image.take() {
            self.frozen.stale.push(old);
        }
        self.show_page(was);
        cx.notify();
    }

    /// Forgets what covered the page, as its tab goes off screen: what is over it then is seen
    /// afresh when it comes back.
    pub(super) fn forget_cover(&mut self) {
        self.cover = Default::default();
        self.frozen.hidden = false;
        if let Some(old) = self.frozen.image.take() {
            self.frozen.stale.push(old);
        }
    }

    /// What the page's element paints this frame: the picture in the webview's place while the
    /// webview is hidden for what covers it, and the pictures to drop.
    pub(super) fn frozen_paint(&mut self) -> FrozenPaint {
        FrozenPaint {
            image: (self.frozen.hidden)
                .then(|| self.frozen.image.clone())
                .flatten(),
            stale: std::mem::take(&mut self.frozen.stale),
            viewport: self.viewport.clone(),
        }
    }
}

/// [`BrowserView::frozen_paint`]'s answer.
pub(super) struct FrozenPaint {
    image: Option<Arc<RenderImage>>,
    stale: Vec<Arc<RenderImage>>,
    viewport: ViewportMode,
}

impl FrozenPaint {
    /// Paints the picture where the webview goes in the page's `bounds`, and drops the stale ones.
    pub(super) fn paint(self, bounds: Bounds<Pixels>, window: &mut Window) {
        for image in self.stale {
            let _ = window.drop_image(image);
        }
        let Some(image) = self.image else {
            return;
        };
        let (at, _) = page_frame(frame(bounds), &self.viewport);
        let at = Bounds::new(point(px(at.x), px(at.y)), size(px(at.width), px(at.height)));
        if let Err(error) = window.paint_image(bounds, at, Corners::default(), image, 0, false) {
            log::warn!("the page's picture couldn't be painted: {error}");
        }
    }
}
