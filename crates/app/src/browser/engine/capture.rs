//! Lane 1 (mesurer-capture): the page as WebKit draws it, for mesurer. The view as it shows, at
//! the display's scale, for `captureVisibleTab` ([`Engine::capture_visible`]); a recording's
//! frames, from WebKit's last drawing and at most so wide ([`Engine::capture_frame`]). WebKit
//! hands the pixels over on the main thread; they are encoded off it ([`Pixels`]).

use std::sync::Mutex;

use futures::channel::oneshot;
use objc2::AllocAnyThread as _;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSImageCompressionFactor};
use objc2_core_foundation::CGSize;
use objc2_core_graphics::CGImage;
use objc2_foundation::{NSDictionary, NSError, NSNumber};
use objc2_web_kit::WKSnapshotConfiguration;
use sawit_core::browser::mesurer::files::{FrameSize, frame_size};
use wry::WebViewExtMacOS as _;

use super::Engine;

/// A snapshot's pixels, at the scale WebKit drew them.
pub struct Pixels(Retained<CGImage>);

// SAFETY: a CGImage is immutable once made, and Core Graphics reads one from any thread.
unsafe impl Send for Pixels {}

impl Pixels {
    /// Its size in pixels.
    pub fn size(&self) -> (u32, u32) {
        let width = CGImage::width(Some(&self.0));
        let height = CGImage::height(Some(&self.0));
        (width as u32, height as u32)
    }

    /// As a PNG. Slow for a big view: off the main thread.
    pub fn png(&self) -> Result<Vec<u8>, String> {
        self.encode(NSBitmapImageFileType::PNG, None)
    }

    /// As a JPEG of `quality` (0 to 1). Off the main thread.
    pub fn jpeg(&self, quality: f64) -> Result<Vec<u8>, String> {
        self.encode(NSBitmapImageFileType::JPEG, Some(quality.clamp(0.1, 1.)))
    }

    fn encode(&self, kind: NSBitmapImageFileType, quality: Option<f64>) -> Result<Vec<u8>, String> {
        objc2::rc::autoreleasepool(|_| {
            let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &self.0);
            let data = match quality {
                Some(quality) => {
                    let factor = NSNumber::new_f64(quality);
                    let properties = NSDictionary::from_slices(
                        &[unsafe { NSImageCompressionFactor }],
                        &[factor.as_ref()],
                    );
                    unsafe { rep.representationUsingType_properties(kind, &properties) }
                }
                None => unsafe {
                    rep.representationUsingType_properties(kind, &NSDictionary::new())
                },
            };
            data.map(|d| d.to_vec())
                .ok_or_else(|| "The capture couldn't be encoded.".to_string())
        })
    }
}

impl Engine {
    /// The view's size in points, and the display's scale.
    fn view_metrics(&self) -> (f64, f64, f64) {
        let wk = self.webview.webview();
        let frame = wk.frame();
        let backing = wk.convertSizeToBacking(CGSize::new(1., 1.)).width;
        (frame.size.width, frame.size.height, backing)
    }

    /// The view as it shows (page zoom, Sawit's overlay and all), at the display's scale, once
    /// WebKit has drawn what changed: `captureVisibleTab`'s picture.
    pub fn capture_visible(&self, reply: oneshot::Sender<Result<Pixels, String>>) {
        self.snapshot_pixels(None, true, reply);
    }

    /// A recording's frame: the view as WebKit last drew it (fast), at the display's scale
    /// brought down to `max_width` pixels wide. Its size is [`Engine::frame_size`]'s.
    pub fn capture_frame(
        &self,
        max_width: Option<u32>,
        reply: oneshot::Sender<Result<Pixels, String>>,
    ) {
        let size = self.frame_size(max_width);
        self.snapshot_pixels(Some(size.points), false, reply);
    }

    /// The frame [`Engine::capture_frame`] takes.
    pub fn frame_size(&self, max_width: Option<u32>) -> FrameSize {
        let (width, height, backing) = self.view_metrics();
        frame_size(width, height, backing, max_width)
    }

    /// Snapshots the whole view, `width` points wide (the view's own width when `None`).
    fn snapshot_pixels(
        &self,
        width: Option<f64>,
        after_screen_updates: bool,
        reply: oneshot::Sender<Result<Pixels, String>>,
    ) {
        use objc2::MainThreadMarker;

        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(Err("Captures are taken on the main thread.".into()));
            return;
        };
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            if let Some(width) = width {
                config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
            }
            config.setAfterScreenUpdates(after_screen_updates);
        }
        let reply = Mutex::new(Some(reply));
        let handler = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
                return;
            };
            let Some(image) = (unsafe { image.as_ref() }) else {
                let reason = unsafe { error.as_ref() }
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "unknown error".into());
                let _ = reply.send(Err(format!("WebKit couldn't capture the page: {reason}")));
                return;
            };
            // The image WebKit made, at the scale it drew it: no copy.
            let pixels = unsafe {
                image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
            };
            let _ = reply.send(
                pixels
                    .map(Pixels)
                    .ok_or_else(|| "The capture has no pixels.".to_string()),
            );
        });
        let wk = self.webview.webview();
        unsafe { wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &handler) };
    }
}
