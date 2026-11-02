//! Lane 1 (mesurer-capture): macOS's colour sampler (`NSColorSampler`) behind mesurer's
//! `EyeDropper`: the loupe over the whole screen, the colour picked in sRGB.

use std::cell::Cell;

use block2::RcBlock;
use objc2_app_kit::{NSColor, NSColorSampler, NSColorSpace};

use super::Engine;

impl Engine {
    /// Opens macOS's colour sampler (or joins the one open). `done` gets the colour picked, as
    /// sRGB components from 0 to 1, or `None` when the user cancels it (Esc); on the main thread.
    pub fn sample_color(&self, done: impl FnOnce(Option<[f64; 3]>) + 'static) {
        let done = Cell::new(Some(done));
        let handler = RcBlock::new(move |color: *mut NSColor| {
            let Some(done) = done.take() else {
                return;
            };
            let picked = unsafe { color.as_ref() }
                .and_then(|color| color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace()))
                .map(|srgb| {
                    [
                        srgb.redComponent(),
                        srgb.greenComponent(),
                        srgb.blueComponent(),
                    ]
                });
            done(picked);
        });
        // AppKit keeps the sampler until its session ends.
        let sampler = NSColorSampler::new();
        unsafe { sampler.showSamplerWithSelectionHandler(&handler) };
    }
}
