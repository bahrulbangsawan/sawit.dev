//! What the canvas's web frames (screens and route frames) ask of a web view: a snapshot as a
//! `CGImage`, drawn into BGRA off the main thread ([`to_bgra`]); a placement in a clipping
//! [`Host`] view, to the fraction of a point; and the web content process's id, for CPU readings.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::Window;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly as _, msg_send, sel};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSImage, NSView};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDisplayCopyColorSpace, CGImage,
    CGMainDisplayID, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSError, NSNumber, NSObjectProtocol as _};
use objc2_web_kit::WKSnapshotConfiguration;
use wry::WebViewExtMacOS as _;
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::{Engine, Frame};

/// A canvas screen a view shows: its name, the host of its URL, and its folder.
#[derive(Clone, Debug)]
pub struct ScreenSource {
    pub name: String,
    pub folder: std::path::PathBuf,
}

/// `builder`, serving `screen`'s folder through the screens' URL scheme: its files, with the
/// policy that keeps the page off the network ([`serve::CSP`]), and nothing outside the folder.
/// Its pages go only to pages of the screen ([`serve::may_navigate`]), and open no windows.
pub fn serve_screen(
    builder: wry::WebViewBuilder<'_>,
    screen: ScreenSource,
) -> wry::WebViewBuilder<'_> {
    use sawit_board::screens::{SCHEME, serve};
    let ScreenSource { name, folder } = screen;
    let host = name.clone();
    builder
        .with_custom_protocol(SCHEME.into(), move |_, request| {
            let url = request.uri().to_string();
            let served = serve::serve(&folder, &name, request.method().as_str(), &url);
            wry::http::Response::builder()
                .status(served.status)
                .header("Content-Type", served.mime)
                .header("Content-Security-Policy", serve::CSP)
                .header("Cache-Control", "no-store")
                .body(std::borrow::Cow::Owned(served.body))
                .unwrap_or_default()
        })
        .with_navigation_handler(move |url| serve::may_navigate(&host, &url))
}

/// BGRA, alpha first and premultiplied, little endian: the bytes GPUI's `RenderImage` takes.
const BGRA: u32 = 2 /* PremultipliedFirst */ | (2 << 12) /* ByteOrder32Little */;

/// A snapshot as WebKit gave it, and how long it took.
pub struct Snap {
    image: Retained<CGImage>,
    /// From the ask to WebKit's answer, on the main thread.
    pub took: Duration,
    /// Getting the `CGImage` out of the `NSImage`.
    pub unwrap: Duration,
}

impl Snap {
    pub fn size(&self) -> (usize, usize) {
        (
            CGImage::width(Some(&self.image)),
            CGImage::height(Some(&self.image)),
        )
    }

    /// The name of the snapshot's colour space (`kCGColorSpaceSRGB`), if it has one.
    pub fn color_space(&self) -> String {
        CGImage::color_space(Some(&self.image))
            .and_then(|space| CGColorSpace::name(Some(&space)))
            .map_or_else(|| "unnamed".into(), |name| name.to_string())
    }
}

/// The colour space a snapshot is drawn into for the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    /// sRGB, as WebKit renders it. GPUI's Metal layer, untagged, is shown as sRGB: these bytes
    /// match the live view's colours (measured: `#ff6600` is (237, 112, 45) on a P3 display, both
    /// ways).
    Srgb,
    /// The main display's space: converted twice on the way to the screen, so too pale
    /// (`#ff6600` showed as (221, 119, 63)), and 5× slower to convert. For comparison only.
    Display,
}

/// The colour space for `target`.
pub fn color_space(target: Target) -> Option<CFRetained<CGColorSpace>> {
    match target {
        Target::Srgb => CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })),
        Target::Display => Some(CGDisplayCopyColorSpace(CGMainDisplayID())),
    }
}

/// `snap` as BGRA bytes in `space`: width, height and pixels. Any thread.
pub fn to_bgra(snap: &Snap, space: &CGColorSpace) -> Option<(u32, u32, Vec<u8>)> {
    let (width, height) = snap.size();
    if width == 0 || height == 0 {
        return None;
    }
    let mut bytes = vec![0u8; width * height * 4];
    // SAFETY: `bytes` holds `height` rows of `width * 4` bytes and outlives the context.
    let context = unsafe {
        CGBitmapContextCreate(
            bytes.as_mut_ptr().cast(),
            width,
            height,
            8,
            width * 4,
            Some(space),
            BGRA,
        )
    }?;
    let rect = CGRect::new(CGPoint::ZERO, CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(&snap.image));
    drop(context);
    Some((width as u32, height as u32, bytes))
}

impl Engine {
    /// Snapshots the whole view, `width` points wide (at the display's scale), after its next
    /// screen update: WebKit answers on the main thread.
    pub fn snapshot_cg(&self, width: f64, reply: oneshot::Sender<Result<Snap, String>>) {
        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(Err("Snapshots are taken on the main thread.".into()));
            return;
        };
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            config.setSnapshotWidth(Some(&NSNumber::new_f64(width)));
            config.setAfterScreenUpdates(true);
        }
        let asked = Instant::now();
        let reply = Mutex::new(Some(reply));
        let handler = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
                return;
            };
            let took = asked.elapsed();
            let result = match unsafe { image.as_ref() } {
                None => Err(unsafe { error.as_ref() }.map_or_else(
                    || "no image".into(),
                    |e| e.localizedDescription().to_string(),
                )),
                Some(image) => {
                    let started = Instant::now();
                    let cg = unsafe {
                        image.CGImageForProposedRect_context_hints(std::ptr::null_mut(), None, None)
                    };
                    cg.map(|image| Snap {
                        image,
                        took,
                        unwrap: started.elapsed(),
                    })
                    .ok_or_else(|| "the snapshot has no CGImage".into())
                }
            };
            let _ = reply.send(result);
        });
        let wk = self.webview.webview();
        unsafe { wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &handler) };
    }

    /// Moves the view into `host`.
    pub fn adopt_into(&self, host: &Host) {
        let wk = self.webview.webview();
        wk.removeFromSuperview();
        // Pinned to the host's top as the host resizes: AppKit's origin is its bottom-left, and a
        // view parked above the canvas would otherwise slide onto it when the canvas grows.
        wk.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinYMargin);
        host.view.addSubview(&wk);
    }

    /// Places the view at `frame` in its host (top-left origin, points), unrounded: wry's
    /// `set_bounds` rounds sizes to whole points.
    pub fn place_in(&self, host: &Host, frame: Frame) {
        let wk = self.webview.webview();
        let rect = host.child_rect(frame);
        if wk.frame() != rect {
            wk.setFrame(rect);
        }
    }

    /// Scales the view as WebKit's own scaled previews do: the private layout mode
    /// `_WKLayoutModeDynamicSizeComputedFromViewScale` (2) and `_viewScale` = `scale`. The page lays
    /// out at the view's size divided by `scale`, exactly as it does unscaled, and renders at the
    /// display's density. `pageZoom` keeps the viewport but clamps text to WebKit's 9 px smart
    /// minimum below about 0.65 (rows 34 → 68 px at 0.25); `magnification` keeps the text but lays
    /// the page out at the view's own width in points (360 CSS px at 0.25). Whether WebKit has it.
    pub fn set_view_scale(&self, scale: f64) -> bool {
        let wk = self.webview.webview();
        let has = wk.respondsToSelector(sel!(_setLayoutMode:))
            && wk.respondsToSelector(sel!(_setViewScale:));
        if has {
            unsafe {
                let _: () = msg_send![&*wk, _setLayoutMode: 2isize];
                let _: () = msg_send![&*wk, _setViewScale: scale];
            }
        }
        has
    }

    /// Scales the view as a pinch does (`magnification`): text unclamped, but the page lays out at
    /// the view's own width in points ([`Self::set_view_scale`]).
    pub fn set_magnification(&self, scale: f64) {
        let wk = self.webview.webview();
        unsafe { wk.setMagnification(scale) };
    }

    /// The web content process's id (`_webProcessIdentifier`, private but long-standing).
    pub fn web_process(&self) -> i32 {
        let wk = self.webview.webview();
        let responds = wk.respondsToSelector(sel!(_webProcessIdentifier));
        if !responds {
            return 0;
        }
        unsafe { msg_send![&*wk, _webProcessIdentifier] }
    }
}

/// A view over GPUI's, clipping what it holds to its bounds (the canvas): the web frames' views
/// live in it, off its bounds while they settle and on them while one is live.
pub struct Host {
    view: Retained<NSView>,
    parent: Retained<NSView>,
}

impl Host {
    /// A host in `window`, over GPUI's view, with no size yet.
    pub fn new(window: &Window) -> Option<Host> {
        let mtm = MainThreadMarker::new()?;
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: GPUI's own view, alive as long as its window.
        let parent = unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }?;
        let view = NSView::initWithFrame(NSView::alloc(mtm), CGRect::ZERO);
        view.setWantsLayer(true);
        // macOS 14 stopped clipping subviews by default.
        if view.respondsToSelector(sel!(setClipsToBounds:)) {
            let _: () = unsafe { msg_send![&*view, setClipsToBounds: true] };
        }
        parent.addSubview(&view);
        Some(Host { view, parent })
    }

    /// Places the host at `frame` in the window (top-left origin, points).
    pub fn place(&self, frame: Frame) {
        let height = self.parent.frame().size.height;
        let y = if self.parent.isFlipped() {
            f64::from(frame.y)
        } else {
            height - f64::from(frame.y) - f64::from(frame.height)
        };
        let rect = CGRect::new(
            CGPoint::new(f64::from(frame.x), y),
            CGSize::new(f64::from(frame.width), f64::from(frame.height)),
        );
        if self.view.frame() != rect {
            self.view.setFrame(rect);
        }
    }

    pub fn set_hidden(&self, hidden: bool) {
        if self.view.isHidden() != hidden {
            self.view.setHidden(hidden);
        }
    }

    /// `frame` (top-left origin, in the host) in the host's own coordinates.
    fn child_rect(&self, frame: Frame) -> CGRect {
        let height = self.view.frame().size.height;
        let y = if self.view.isFlipped() {
            f64::from(frame.y)
        } else {
            height - f64::from(frame.y) - f64::from(frame.height)
        };
        CGRect::new(
            CGPoint::new(f64::from(frame.x), y),
            CGSize::new(f64::from(frame.width), f64::from(frame.height)),
        )
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.view.removeFromSuperview();
    }
}
