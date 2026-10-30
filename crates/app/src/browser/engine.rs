//! The web view behind the browser tab. This is the only file that names
//! wry, objc2 or block2; the rest of the tab talks to [`Engine`].

use std::sync::Mutex;

use futures::channel::oneshot;
use gpui_kit::Window;
use serde_json::{Value, json};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder};

/// A box in the window, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct EngineSpec {
    pub url: String,
    pub init_script: String,
    /// Keeps cookies and storage apart per project.
    pub data_store: Option<[u8; 16]>,
    pub visible: bool,
    pub frame: Frame,
}

/// What the page reports, from handlers on any thread.
pub enum EngineEvent {
    /// A message the page agent posted.
    Ipc(String),
    Load {
        started: bool,
        url: String,
    },
    Title(String),
    /// A link asked for a new window: it opens in the tab instead.
    NewWindow(String),
}

pub struct Engine {
    webview: WebView,
}

impl Engine {
    pub fn build(
        spec: EngineSpec,
        on_event: impl Fn(EngineEvent) + Clone + 'static,
        window: &Window,
    ) -> Result<Engine, String> {
        let ipc = on_event.clone();
        let loads = on_event.clone();
        let titles = on_event.clone();
        let opens = on_event;
        let mut builder = WebViewBuilder::new()
            .with_url(&spec.url)
            .with_initialization_script_for_main_only(spec.init_script, true)
            .with_ipc_handler(move |request| ipc(EngineEvent::Ipc(request.body().clone())))
            .with_on_page_load_handler(move |event, url| {
                let started = matches!(event, PageLoadEvent::Started);
                loads(EngineEvent::Load { started, url });
            })
            .with_document_title_changed_handler(move |title| titles(EngineEvent::Title(title)))
            .with_navigation_handler(|url| {
                if is_web(&url) {
                    return true;
                }
                // mailto:, tel:, app links: the system's handler.
                let _ = std::process::Command::new("open").arg(&url).spawn();
                false
            })
            .with_new_window_req_handler(move |url, _| {
                opens(EngineEvent::NewWindow(url));
                NewWindowResponse::Deny
            })
            .with_devtools(true)
            .with_back_forward_navigation_gestures(true)
            .with_accept_first_mouse(true)
            .with_focused(spec.visible)
            .with_visible(spec.visible)
            .with_bounds(rect(spec.frame));
        if let Some(id) = spec.data_store {
            use wry::WebViewBuilderExtDarwin as _;
            builder = builder.with_data_store_identifier(id);
        }
        let webview = builder.build_as_child(window).map_err(|e| e.to_string())?;
        Ok(Engine { webview })
    }

    pub fn load_url(&self, url: &str) {
        let _ = self.webview.load_url(url);
    }

    pub fn reload(&self) {
        let _ = self.webview.reload();
    }

    pub fn eval(&self, script: &str) {
        let _ = self.webview.evaluate_script(script);
    }

    /// Runs `script`; `callback` gets its result as WebKit serializes it (JSON).
    pub fn eval_with_callback(
        &self,
        script: &str,
        callback: impl Fn(String) + Send + 'static,
    ) -> Result<(), String> {
        self.webview
            .evaluate_script_with_callback(script, callback)
            .map_err(|e| e.to_string())
    }

    pub fn url(&self) -> Option<String> {
        self.webview.url().ok()
    }

    pub fn set_frame(&self, frame: Frame) {
        let _ = self.webview.set_bounds(rect(frame));
    }

    pub fn set_visible(&self, visible: bool) {
        let _ = self.webview.set_visible(visible);
    }

    /// Page zoom: 1.0 is the page's own size.
    #[allow(dead_code)] // device sizes (a later task) scale pages with it
    pub fn set_zoom(&self, scale: f64) {
        let _ = self.webview.zoom(scale);
    }

    pub fn focus(&self) {
        let _ = self.webview.focus();
    }

    pub fn focus_parent(&self) {
        let _ = self.webview.focus_parent();
    }

    pub fn open_devtools(&self) {
        self.webview.open_devtools();
    }

    /// Captures the view (or `css_rect` of it, in CSS px) as a PNG, resized
    /// to `css_width` px wide, answered as `png_base64` with its size. The
    /// rect is scaled by `scale` (the page zoom) into the view's points.
    pub fn snapshot(
        &self,
        css_rect: Option<(f64, f64, f64, f64)>,
        css_width: f64,
        scale: f64,
        reply: oneshot::Sender<Result<Value, String>>,
    ) {
        use objc2::MainThreadMarker;
        use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
        use objc2_core_foundation::{CGPoint, CGRect, CGSize};
        use objc2_foundation::{NSDictionary, NSError, NSNumber};
        use objc2_web_kit::WKSnapshotConfiguration;
        use wry::WebViewExtMacOS as _;

        let Some(mtm) = MainThreadMarker::new() else {
            let _ = reply.send(Err("Screenshots are taken on the main thread.".into()));
            return;
        };
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            if let Some((x, y, w, h)) = css_rect {
                config.setRect(CGRect::new(
                    CGPoint::new(x * scale, y * scale),
                    CGSize::new(w * scale, h * scale),
                ));
            }
            config.setSnapshotWidth(Some(&NSNumber::new_f64(css_width)));
            config.setAfterScreenUpdates(true);
        }
        let reply = Mutex::new(Some(reply));
        let handler = block2::RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
            let Some(reply) = reply.lock().ok().and_then(|mut r| r.take()) else {
                return;
            };
            let result = (|| {
                if image.is_null() {
                    let reason = unsafe { error.as_ref() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "unknown error".into());
                    return Err(format!("WebKit couldn't take the screenshot: {reason}"));
                }
                let image = unsafe { &*image };
                let tiff = image
                    .TIFFRepresentation()
                    .ok_or("The screenshot has no pixels.")?;
                let rep = NSBitmapImageRep::imageRepWithData(&tiff)
                    .ok_or("The screenshot has no bitmap.")?;
                let png = unsafe {
                    rep.representationUsingType_properties(
                        NSBitmapImageFileType::PNG,
                        &NSDictionary::new(),
                    )
                }
                .ok_or("The screenshot couldn't be saved as PNG.")?;
                let (png, w, h) = at_1x(png.to_vec(), css_width.round() as u32)?;
                Ok(json!({
                    "png_base64": sawit_core::browser::base64_encode(&png),
                    "width": w,
                    "height": h,
                }))
            })();
            let _ = reply.send(result);
        });
        let wk = self.webview.webview();
        unsafe { wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &handler) };
    }
}

fn is_web(url: &str) -> bool {
    ["http:", "https:", "about:", "data:", "blob:", "file:"]
        .iter()
        .any(|scheme| url.starts_with(scheme))
}

fn rect(frame: Frame) -> Rect {
    Rect {
        position: LogicalPosition::new(frame.x, frame.y).into(),
        size: LogicalSize::new(frame.width.max(1.), frame.height.max(1.)).into(),
    }
}

/// Retina captures are twice the size in pixels; agents get them at the
/// page's CSS size, which is plenty and costs a quarter of the tokens.
fn at_1x(png: Vec<u8>, width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)
        .map_err(|e| format!("The screenshot couldn't be read: {e}"))?;
    if width == 0 || image.width() <= width + width / 10 {
        let (w, h) = (image.width(), image.height());
        return Ok((png, w, h));
    }
    let height =
        (u64::from(image.height()) * u64::from(width) / u64::from(image.width())).max(1) as u32;
    let small = image.resize_exact(width, height, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    small
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| format!("The screenshot couldn't be saved: {e}"))?;
    Ok((out.into_inner(), width, height))
}

#[cfg(test)]
mod tests {
    use super::{at_1x, is_web};

    #[test]
    fn web_schemes_stay_in_the_tab() {
        assert!(is_web("https://a.dev") && is_web("about:blank") && is_web("file:///x"));
        assert!(!is_web("mailto:a@b.c") && !is_web("slack://open"));
    }

    #[test]
    fn retina_screenshots_shrink_to_css_size() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::new(200, 100)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let (small, w, h) = at_1x(png.into_inner(), 100).unwrap();
        assert_eq!((w, h), (100, 50));
        assert_eq!(image::load_from_memory(&small).unwrap().width(), 100);
    }
}
