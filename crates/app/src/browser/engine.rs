//! The web view behind the browser tab, and news of the Mac's fonts ([`font_changes`]). This is
//! the only file that names wry, objc2 or block2, with [`web_frame`], what the canvas's web frames
//! ask of a view; the rest of the tab talks to [`Engine`].

pub(crate) mod web_frame;

use std::ffi::c_void;
use std::sync::Mutex;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures::channel::oneshot;
use gpui_kit::Window;
use objc2_core_foundation::{
    CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
    CFString,
};
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
    /// A canvas screen the view shows: its own folder, served through the screens' URL scheme
    /// ([`web_frame::serve_screen`]), and nothing else.
    pub screen: Option<web_frame::ScreenSource>,
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
    /// The page asked for a new window (`window.open`, a `target=_blank` link): web
    /// pages open in the tab instead (`new_window_action`).
    NewWindow(String),
    /// The page (or a frame in it) went to a link another app handles
    /// (`mailto:`, `slack://…`).
    External(String),
}

pub struct Engine {
    webview: WebView,
}

/// A capture as WebKit took it, at the display's scale: a PNG to bring to `width` CSS px wide
/// ([`Capture::at_1x`]), slow work that stays off the main thread.
pub struct Capture {
    png: Vec<u8>,
    width: u32,
}

impl Capture {
    /// The capture at the page's CSS size, answered as `png_base64` with its size.
    pub fn at_1x(self) -> Result<Value, String> {
        let (png, w, h) = at_1x(self.png, self.width)?;
        Ok(json!({
            "png_base64": sawit_core::browser::base64_encode(&png),
            "width": w,
            "height": h,
        }))
    }
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
        let external = on_event.clone();
        let opens = on_event;
        // wry unwraps the `NSURL` it makes of the address.
        let url = if loadable(&spec.url) {
            spec.url.as_str()
        } else {
            "about:blank"
        };
        let mut builder = WebViewBuilder::new()
            .with_url(url)
            .with_initialization_script_for_main_only(spec.init_script, true)
            .with_ipc_handler(move |request| ipc(EngineEvent::Ipc(request.body().clone())))
            .with_on_page_load_handler(move |event, url| {
                let started = matches!(event, PageLoadEvent::Started);
                loads(EngineEvent::Load { started, url });
            })
            .with_document_title_changed_handler(move |title| titles(EngineEvent::Title(title)))
            .with_navigation_handler(move |url| navigation(&url, &external))
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
        if let Some(screen) = spec.screen {
            builder = web_frame::serve_screen(builder, screen);
        }
        let webview = builder.build_as_child(window).map_err(|e| e.to_string())?;
        Ok(Engine { webview })
    }

    /// Loads `url`; text that isn't a URL WebKit reads ([`loadable`]) is refused.
    pub fn load_url(&self, url: &str) -> Result<(), String> {
        if !loadable(url) {
            return Err(format!("WebKit can't load “{url}”: it isn't a URL."));
        }
        self.webview.load_url(url).map_err(|e| e.to_string())
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

    /// Runs `script`; `callback` gets its value when that is a string (`None` for any other),
    /// or the exception WebKit reports, a `SyntaxError: …` when the script doesn't parse.
    pub fn eval_text(
        &self,
        script: &str,
        callback: impl FnOnce(Result<Option<String>, String>) + 'static,
    ) {
        use objc2::runtime::AnyObject;
        use objc2_foundation::{NSError, NSString};
        use wry::WebViewExtMacOS as _;

        let callback = std::cell::Cell::new(Some(callback));
        let handler = block2::RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
            let Some(callback) = callback.take() else {
                return;
            };
            if let Some(error) = unsafe { error.as_ref() } {
                callback(Err(exception_message(error)));
                return;
            }
            let text = unsafe { value.as_ref() }
                .and_then(|v| v.downcast_ref::<NSString>())
                .map(|s| s.to_string());
            callback(Ok(text));
        });
        let wk = self.webview.webview();
        unsafe {
            wk.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&handler))
        };
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

    /// Captures the view (or `css_rect` of it, in CSS px) as a PNG, `css_width` px wide at the
    /// display's scale; [`Capture::at_1x`] brings it to that size. The rect is scaled by `scale`
    /// (the page zoom) into the view's points.
    pub fn snapshot(
        &self,
        css_rect: Option<(f64, f64, f64, f64)>,
        css_width: f64,
        scale: f64,
        reply: oneshot::Sender<Result<Capture, String>>,
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
                Ok(Capture {
                    png: png.to_vec(),
                    width: css_width.round() as u32,
                })
            })();
            let _ = reply.send(result);
        });
        let wk = self.webview.webview();
        unsafe { wk.takeSnapshotWithConfiguration_completionHandler(Some(&config), &handler) };
    }
}

/// What WebKit says went wrong running a script: the exception's own message
/// (`SyntaxError: Unexpected keyword 'return'`) when it gives one.
fn exception_message(error: &objc2_foundation::NSError) -> String {
    use objc2_foundation::NSString;
    let key = NSString::from_str("WKJavaScriptExceptionMessage");
    error
        .userInfo()
        .objectForKey(&key)
        .and_then(|m| m.downcast::<NSString>().ok())
        .map(|m| m.to_string())
        .unwrap_or_else(|| error.localizedDescription().to_string())
}

/// Whether WebKit can be asked to load `url`: wry makes an `NSURL` of it and unwraps that,
/// and there is none for text that isn't a URL (`file://my host/x`, a second `:port`), so
/// loading such text would abort the app.
pub fn loadable(url: &str) -> bool {
    use objc2_foundation::{NSString, NSURL};
    NSURL::URLWithString(&NSString::from_str(url)).is_some()
}

/// Whether a navigation goes ahead in the tab: web pages do. A link another app handles
/// (`mailto:`, `slack://…`) is handed to the tab, which asks before opening it: WebKit asks
/// for every frame's navigations, with no word of a click, so a page could otherwise
/// launch any app with arguments of its choosing.
fn navigation(url: &str, send: &impl Fn(EngineEvent)) -> bool {
    if is_web(url) {
        return true;
    }
    send(EngineEvent::External(url.to_string()));
    false
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

/// Who hears of the Mac's font changes ([`font_changes`]).
static FONT_LISTENERS: Mutex<Vec<UnboundedSender<()>>> = Mutex::new(Vec::new());

/// A message each time fonts are installed on this Mac or removed from it: CoreText's
/// `kCTFontManagerRegisteredFontsChangedNotification`, observed once for the whole app on the
/// local notification center. A listener whose receiver went is dropped as the next listener
/// comes or the fonts next change.
pub fn font_changes() -> UnboundedReceiver<()> {
    static OBSERVED: std::sync::Once = std::sync::Once::new();
    OBSERVED.call_once(|| {
        let Some(center) = CFNotificationCenter::local_center() else {
            return;
        };
        // SAFETY: the observer stands for the listeners' list, a static; the callback reads
        // nothing else, and the name is CoreText's own.
        unsafe {
            center.add_observer(
                (&raw const FONT_LISTENERS).cast(),
                Some(fonts_changed),
                Some(kCTFontManagerRegisteredFontsChangedNotification),
                std::ptr::null(),
                CFNotificationSuspensionBehavior::DeliverImmediately,
            );
        }
    });
    let (tx, rx) = unbounded();
    let mut listeners = FONT_LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
    // Closed browser tabs' listeners don't pile up until the fonts change.
    listeners.retain(|tx| !tx.is_closed());
    listeners.push(tx);
    rx
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    /// Posted to the local notification center when fonts are added to the font registry or
    /// removed from it.
    static kCTFontManagerRegisteredFontsChangedNotification: &'static CFString;
}

/// CoreText says the fonts changed, on whichever thread posted it: every listener hears of it.
unsafe extern "C-unwind" fn fonts_changed(
    _: *mut CFNotificationCenter,
    _: *mut c_void,
    _: *const CFNotificationName,
    _: *const c_void,
    _: *const CFDictionary,
) {
    let mut listeners = FONT_LISTENERS.lock().unwrap_or_else(|e| e.into_inner());
    listeners.retain(|tx| tx.unbounded_send(()).is_ok());
}

#[cfg(test)]
mod tests {
    use super::{EngineEvent, at_1x, is_web, navigation};

    #[test]
    fn web_schemes_stay_in_the_tab() {
        assert!(is_web("https://a.dev") && is_web("about:blank") && is_web("file:///x"));
        assert!(!is_web("mailto:a@b.c") && !is_web("slack://open"));
    }

    /// A page, or a frame it adds with no click, can't launch another app: a link to one is
    /// handed to the tab to ask about, never opened from here.
    #[test]
    fn links_to_other_apps_are_handed_over_not_opened() {
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |e: EngineEvent| sent.borrow_mut().push(e);
        // A scheme no app handles: opening it launches nothing.
        let link = "sawit-test-no-handler://x?i=1";
        assert!(!navigation(link, &send));
        assert!(
            matches!(&sent.borrow()[..], [EngineEvent::External(u)] if u == link),
            "the tab is told"
        );
        sent.borrow_mut().clear();
        for web in [
            "https://a.dev",
            "about:blank",
            "data:text/html,x",
            "blob:https://a.dev/1",
            "file:///x",
        ] {
            assert!(navigation(web, &send), "{web}");
        }
        assert!(sent.borrow().is_empty());
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

    /// A change to the Mac's fonts, as CoreText posts it, reaches every listener; one that went
    /// is left out.
    #[test]
    fn font_changes_reach_every_listener() {
        let mut first = super::font_changes();
        let mut second = super::font_changes();
        drop(super::font_changes());
        let center = objc2_core_foundation::CFNotificationCenter::local_center().unwrap();
        // SAFETY: CoreText's own name, with no object and no user info.
        unsafe {
            center.post_notification(
                Some(super::kCTFontManagerRegisteredFontsChangedNotification),
                std::ptr::null(),
                None,
                true,
            )
        };
        assert_eq!(first.try_recv(), Ok(()));
        assert_eq!(second.try_recv(), Ok(()));
    }
}
