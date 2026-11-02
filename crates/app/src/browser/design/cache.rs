//! What the tabs keep between frames instead of working it out on every one: the Colors tab of
//! the scan on show, and the font families this Mac has, for the Fonts and Inspect tabs.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;
use sawit_core::site_design::scan::Scan;

use super::DesignPanel;
use super::colors::Palette;

/// What the panel keeps between frames.
#[derive(Default)]
pub(super) struct Cache {
    /// The Colors tab of the scan on show; it goes with the scan ([`Cache::scan_changed`]).
    palette: Option<Rc<Palette>>,
    /// The font families this Mac has: read on the first frame that shows a specimen, and again
    /// whenever the Mac's fonts change ([`DesignPanel::fonts_changed`]).
    faces: Option<Arc<Vec<String>>>,
    /// They are being read again, in the background.
    reading_faces: bool,
    /// The fonts changed again while they were being read: they are read once more after.
    read_faces_again: bool,
}

impl Cache {
    /// Another scan is on show: what was worked out of the last one goes.
    pub(super) fn scan_changed(&mut self) {
        self.palette = None;
    }
}

impl DesignPanel {
    /// The Colors tab of `scan`, the scan on show: worked out on its first frame, not on every
    /// one, as it reads every colour and every variable the page has.
    pub(super) fn palette(&mut self, scan: &Scan) -> Rc<Palette> {
        (self.cache.palette)
            .get_or_insert_with(|| Rc::new(Palette::of(scan)))
            .clone()
    }

    /// The font families this Mac has. CoreText takes tens of milliseconds to list them, too long
    /// for every frame: they are read once, then again, off the UI thread, when fonts are
    /// installed or removed.
    pub(super) fn installed_fonts(&mut self, cx: &mut Context<Self>) -> Arc<Vec<String>> {
        (self.cache.faces)
            .get_or_insert_with(|| Arc::new(cx.text_system().all_font_names()))
            .clone()
    }

    /// The Mac's fonts changed (CoreText's notification, which `engine::font_changes` hears): the
    /// families are read again, off the UI thread, and the tabs draw again if a face came or went.
    pub fn fonts_changed(&mut self, cx: &mut Context<Self>) {
        // Not read yet: the first frame that needs them reads them as they are then.
        if self.cache.faces.is_none() {
            return;
        }
        if self.cache.reading_faces {
            self.cache.read_faces_again = true;
            return;
        }
        self.cache.reading_faces = true;
        let text_system = cx.text_system().clone();
        let read = cx.background_spawn(async move { text_system.all_font_names() });
        cx.spawn(async move |this, cx| {
            let fresh = read.await;
            let _ = this.update(cx, |this, cx| {
                let cache = &mut this.cache;
                cache.reading_faces = false;
                if cache.faces.as_deref() != Some(&fresh) {
                    cache.faces = Some(Arc::new(fresh));
                    cx.notify();
                }
                if std::mem::take(&mut cache.read_faces_again) {
                    this.fonts_changed(cx);
                }
            });
        })
        .detach();
    }

    /// Whether the font families are being read again (for tests).
    #[cfg(test)]
    pub(super) fn reading_fonts(&self) -> bool {
        self.cache.reading_faces
    }
}
