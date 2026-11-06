//! A page loading, as the toolbar shows it: a thin bar along its foot that fills as WebKit
//! reports progress (`estimatedProgress`, read every tenth of a second while the page loads and
//! never otherwise), with Stop in Reload's place, so a slow page never looks hung.

use std::time::Duration;

use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, Size};
use gpui_kit::*;

use super::BrowserView;

/// How often WebKit's progress is read while the page loads.
const POLL: Duration = Duration::from_millis(100);
/// The least the bar shows once a load starts: WebKit reports 0.1 only after the first bytes,
/// and an empty bar says nothing.
const FLOOR: f64 = 0.08;

/// How far the page has come, and whether something is reading it.
#[derive(Default)]
pub(super) struct LoadProgress {
    /// 0 to 1.
    pub value: f64,
    polling: bool,
}

impl BrowserView {
    /// Reads WebKit's progress until the page has loaded: a load started (by the user, an agent,
    /// a link or a reload). One reader at a time; it ends itself once loading ends.
    pub(super) fn watch_progress(&mut self, cx: &mut Context<Self>) {
        self.progress.value = FLOOR;
        if self.progress.polling {
            return;
        }
        self.progress.polling = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                let going = this.update(cx, |this, cx| {
                    if !this.loading {
                        this.progress = LoadProgress::default();
                        cx.notify();
                        return false;
                    }
                    let read = this.engine.as_ref().map_or(0., |e| e.progress());
                    let value = shown_progress(this.progress.value, read);
                    if value != this.progress.value {
                        this.progress.value = value;
                        cx.notify();
                    }
                    true
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// The bar along the toolbar's foot while the page loads, over its border; nothing once it
    /// has. It eases to each new reading (at once under reduced motion: the component's own
    /// transition asks the theme's motion tokens).
    pub(super) fn progress_bar(&self, cx: &App) -> Option<impl IntoElement + use<>> {
        self.loading.then(|| {
            Progress::new("browser-loading")
                .accessibility_label("Loading the page")
                .value((self.progress.value.max(FLOOR) * 100.) as f32)
                .color(cx.theme().primary)
                .with_size(Size::Size(px(2.)))
                .rounded(px(0.))
                .absolute()
                .left_0()
                .bottom_0()
        })
    }
}

/// What the bar shows: WebKit's reading, never going back while one load lasts, and never under
/// the floor.
pub(super) fn shown_progress(shown: f64, read: f64) -> f64 {
    shown.max(read.clamp(0., 1.)).max(FLOOR)
}
