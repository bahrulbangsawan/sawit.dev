//! What the tabs keep between frames instead of working it out on every one: the font families
//! this Mac has, for the specimens of the Fonts and Inspect tabs.

use std::sync::Arc;

use gpui_kit::*;

use super::DesignPanel;

/// What the panel keeps between frames.
#[derive(Default)]
pub(super) struct Cache {
    /// The font families this Mac has: read on the first frame that shows a specimen, and again
    /// whenever the Mac's fonts change ([`DesignPanel::fonts_changed`]).
    faces: Option<Arc<Vec<String>>>,
    /// They are being read again, in the background.
    reading_faces: bool,
    /// The fonts changed again while they were being read: they are read once more after.
    read_faces_again: bool,
}

impl DesignPanel {
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
