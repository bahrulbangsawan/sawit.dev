//! AppKit's keyboard kept in step with GPUI's: the one owner of which web view, if any, is the
//! window's first responder.
//!
//! GPUI moves its own keyboard (a `FocusHandle`) on clicks and shortcuts, but AppKit sends the key
//! events to the window's first responder, and a `WKWebView` that became first responder stays it
//! until something gives it back. When the two disagree, keys typed "in the terminal" land in the
//! browser's page or a canvas frame, and Tab, ⌃Tab and plain letters seem dead until a click.
//! WebKit also makes a page first responder on its own, when the page focuses a field: an agent's
//! `browser_type`, or a page that autofocuses one as it loads.
//!
//! The rules ([`verdict`]): a web view may be first responder only
//! - while GPUI's keyboard is on the view that hosts it (the browser tab, the canvas;
//!   [`Engine::set_keyboard_host`](super::Engine::set_keyboard_host)), or
//! - after the user's own press in it, until GPUI's keyboard moves (the page's `Pressed` message
//!   moves GPUI's keyboard to the host a moment after the press);
//! - and never while hidden, in no window, or off screen.
//!
//! They are applied when GPUI's keyboard moves or the workspace draws ([`reconcile`]), and, so that
//! no key is ever lost to a page that took the keyboard between two frames, to every key before
//! AppKit delivers it (a local key-down monitor, [`install_monitors`]). A mouse-down monitor tells
//! the user's presses in a page from everything else.
//!
//! The app's own shortcuts work in a page too, with nothing done here: AppKit offers ⌘ and ⌃ keys
//! (⌃Tab, ⌃⇧Tab, ⇧⌘↩) to GPUI's view as key equivalents before the page sees them (checked by the
//! `focus` end-to-end spec).
//!
//! Nothing here *gives* a web view the keyboard: that stays with the user's gestures (Return in the
//! address field, a press in the page, entering a canvas frame), never an agent's tool call.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ptr::NonNull;

use gpui_kit::{App, FocusHandle, WeakFocusHandle, Window, WindowId};
use objc2::rc::{Retained, Weak};
use objc2::{MainThreadMarker, Message as _};
use objc2_app_kit::{NSEvent, NSEventMask, NSResponder, NSView, NSWindow};
use wry::raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// Where AppKit's keyboard is, as [`verdict`] weighs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Responder {
    /// GPUI's own view, or nothing of ours: always fine.
    Gpui,
    /// A web view of ours.
    Page {
        /// GPUI's keyboard is on the view that hosts it.
        host_focused: bool,
        /// The user pressed in it, and GPUI's keyboard hasn't moved since.
        pressed: bool,
        /// It is drawn: in a window, not hidden, and some of it on screen.
        shown: bool,
    },
}

/// What happens to AppKit's keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep,
    /// GPUI's view becomes first responder again.
    GiveBack,
}

/// The rules.
pub fn verdict(responder: Responder) -> Verdict {
    match responder {
        Responder::Gpui => Verdict::Keep,
        // Hidden or off screen: never, whoever asked.
        Responder::Page { shown: false, .. } => Verdict::GiveBack,
        Responder::Page {
            host_focused: true, ..
        }
        | Responder::Page { pressed: true, .. } => Verdict::Keep,
        // WebKit's own doing (a field focused by script) or left over: back to GPUI.
        Responder::Page { .. } => Verdict::GiveBack,
    }
}

/// A web view of ours, and the GPUI view that may give it the keyboard.
struct Page {
    view: Weak<NSView>,
    host: Option<WeakFocusHandle>,
    /// Whether the host had GPUI's keyboard at the last [`reconcile`]: what the key monitor,
    /// which can't ask GPUI, goes by.
    host_focused: bool,
}

thread_local! {
    static PAGES: RefCell<Vec<Page>> = const { RefCell::new(Vec::new()) };
    /// What had GPUI's keyboard at each window's last check.
    static LAST: RefCell<HashMap<WindowId, Option<WeakFocusHandle>>> = RefCell::new(HashMap::new());
    /// GPUI's view of each window, by the `NSWindow`'s address: where the keyboard goes back to.
    static GPUI_VIEWS: RefCell<Vec<(usize, Weak<NSView>)>> = const { RefCell::new(Vec::new()) };
    /// The page the user last pressed in, until GPUI's keyboard moves.
    static PRESSED: RefCell<Option<Weak<NSView>>> = const { RefCell::new(None) };
    static MONITORS: Cell<bool> = const { Cell::new(false) };
}

/// Counts `view` as one of ours, hosted by nothing yet.
pub(super) fn register(view: &NSView) {
    install_monitors();
    PAGES.with_borrow_mut(|pages| {
        pages.retain(|page| page.view.load().is_some());
        pages.push(Page {
            view: Weak::from(view),
            host: None,
            host_focused: false,
        });
    });
}

/// Names `host` as the GPUI view whose keyboard `view` may share; `None`: no view may.
fn set_host(view: &NSView, host: Option<&FocusHandle>) {
    PAGES.with_borrow_mut(|pages| {
        if let Some(page) = (pages.iter_mut()).find(|page| is(&page.view, view)) {
            page.host = host.map(FocusHandle::downgrade);
            if host.is_none() {
                page.host_focused = false;
            }
        }
    });
}

fn is(weak: &Weak<NSView>, view: &NSView) -> bool {
    weak.load().is_some_and(|v| std::ptr::eq(&*v, view))
}

/// Whether `view`, or a view inside it, is its window's first responder.
fn holds_keyboard(view: &NSView) -> bool {
    view.window()
        .and_then(|window| window.firstResponder())
        .and_then(|responder| as_view(&responder))
        .is_some_and(|responder| responder.isDescendantOf(view))
}

fn as_view(responder: &NSResponder) -> Option<Retained<NSView>> {
    responder.downcast_ref::<NSView>().map(|view| view.retain())
}

/// Drawn: in a window, not hidden, and some of it inside what clips it (a canvas frame settling
/// off the canvas's bounds is not).
fn shown(view: &NSView) -> bool {
    view.window().is_some() && !view.isHiddenOrHasHiddenAncestor() && {
        let visible = view.visibleRect();
        visible.size.width > 0. && visible.size.height > 0.
    }
}

/// GPUI's own view of `window`.
fn gpui_view(window: &Window) -> Option<Retained<NSView>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: GPUI's own view, alive as long as its window.
    unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }
}

fn window_key(window: &NSWindow) -> usize {
    std::ptr::from_ref(window) as usize
}

/// GPUI's view of `ns_window`, as the last [`reconcile`] noted it.
fn gpui_view_of(ns_window: &NSWindow) -> Option<Retained<NSView>> {
    let key = window_key(ns_window);
    GPUI_VIEWS.with_borrow(|views| {
        (views.iter())
            .find(|(k, _)| *k == key)
            .and_then(|(_, view)| view.load())
    })
}

/// The page of ours that holds `ns_window`'s keyboard, with what [`verdict`] needs to know.
fn responder_in(ns_window: &NSWindow) -> Responder {
    let Some(responder) = ns_window.firstResponder().and_then(|r| as_view(&r)) else {
        return Responder::Gpui;
    };
    PAGES.with_borrow(|pages| {
        pages
            .iter()
            .find_map(|page| {
                let web = page.view.load()?;
                responder.isDescendantOf(&web).then(|| Responder::Page {
                    host_focused: page.host_focused,
                    pressed: PRESSED.with_borrow(|p| p.as_ref().is_some_and(|p| is(p, &web))),
                    shown: shown(&web),
                })
            })
            .unwrap_or(Responder::Gpui)
    })
}

/// Applies the rules to `ns_window` now: GPUI's view takes the keyboard back from a page that may
/// not have it. Whether it did.
fn enforce(ns_window: &NSWindow) -> bool {
    if verdict(responder_in(ns_window)) == Verdict::Keep {
        return false;
    }
    match gpui_view_of(ns_window) {
        Some(view) => ns_window.makeFirstResponder(Some(&view)),
        None => false,
    }
}

/// Applies the rules in every window now, without asking GPUI (its last [`reconcile`] stands): for
/// a page that may have focused a field on its own, after an agent's op or a load.
pub fn settle() {
    let windows: Vec<Retained<NSWindow>> = GPUI_VIEWS.with_borrow(|views| {
        (views.iter())
            .filter_map(|(_, view)| view.load().and_then(|view| view.window()))
            .collect()
    });
    for window in windows {
        enforce(&window);
    }
}

/// From the workspace's render: notes where GPUI's keyboard is (which page hosts have it, and
/// GPUI's view of the window), forgets the user's press once GPUI's keyboard moved, and applies the
/// rules. Cheap when nothing changed.
pub fn reconcile(window: &Window, cx: &App) {
    let focused = window.focused(cx);
    let id = window.window_handle().window_id();
    let moved = LAST.with_borrow_mut(|last| {
        let before =
            (last.get(&id)).and_then(|weak| weak.as_ref().and_then(WeakFocusHandle::upgrade));
        let moved = before != focused;
        last.insert(id, focused.as_ref().map(FocusHandle::downgrade));
        moved
    });
    if moved {
        PRESSED.with_borrow_mut(|pressed| *pressed = None);
    }
    PAGES.with_borrow_mut(|pages| {
        pages.retain(|page| page.view.load().is_some());
        for page in pages.iter_mut() {
            page.host_focused = (page.host.as_ref())
                .and_then(WeakFocusHandle::upgrade)
                .is_some_and(|host| host.contains_focused(window, cx));
        }
    });
    let Some(view) = gpui_view(window) else {
        return;
    };
    let Some(ns_window) = view.window() else {
        return;
    };
    let key = window_key(&ns_window);
    GPUI_VIEWS.with_borrow_mut(|views| {
        views.retain(|(_, view)| view.load().is_some());
        if !views.iter().any(|(k, _)| *k == key) {
            views.push((key, Weak::from(&*view)));
        }
    });
    enforce(&ns_window);
}

/// The two local event monitors, once per process: a mouse down notes the page the user pressed in
/// (or none), and a key down applies the rules before AppKit hands the key to the first responder,
/// so the key goes where GPUI's keyboard is. Neither changes the event.
fn install_monitors() {
    if MONITORS.replace(true) {
        return;
    }
    let Some(mtm) = MainThreadMarker::new() else {
        MONITORS.set(false);
        return;
    };
    let presses = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor a valid event for the length of the call.
        let ev = unsafe { event.as_ref() };
        let at = ev.locationInWindow();
        let window = ev.window(mtm);
        let hit = PAGES.with_borrow(|pages| {
            pages.iter().find_map(|page| {
                let web = page.view.load()?;
                let same = web
                    .window()
                    .zip(window.as_ref())
                    .is_some_and(|(a, b)| a == *b);
                let local = web.convertPoint_fromView(at, None);
                let inside =
                    same && shown(&web) && objc2_foundation::NSPointInRect(local, web.bounds());
                inside.then(|| Weak::from(&*web))
            })
        });
        PRESSED.with_borrow_mut(|pressed| *pressed = hit);
        event.as_ptr()
    });
    let keys = block2::RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: as above.
        if let Some(window) = unsafe { event.as_ref() }.window(mtm) {
            enforce(&window);
        }
        event.as_ptr()
    });
    let downs =
        NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown | NSEventMask::OtherMouseDown;
    // SAFETY: both blocks return the event they were given. The monitors live as long as the app.
    unsafe {
        std::mem::forget(NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            downs, &presses,
        ));
        std::mem::forget(NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::KeyDown,
            &keys,
        ));
    }
}

/// The page's side of the rules: what the browser and the canvas ask of it.
impl super::Engine {
    /// Makes the page AppKit's first responder. Only for the user's own gestures (Return in the
    /// address field, entering a canvas frame), never for an agent's tool call (the module's rules).
    /// Nothing happens while the view is in no window (wry's own `focus` would panic).
    pub fn focus(&self) {
        use wry::WebViewExtMacOS as _;
        let wk = self.webview.webview();
        if let Some(window) = wk.window() {
            window.makeFirstResponder(Some(&wk));
        }
    }

    /// Gives AppKit's keyboard back to GPUI's view, if the page has it; another page's or GPUI's
    /// own keyboard is left where it is.
    pub fn focus_parent(&self) {
        if self.has_keyboard() {
            let _ = self.webview.focus_parent();
        }
    }

    /// Whether the page (or a view inside it) is its window's first responder.
    pub fn has_keyboard(&self) -> bool {
        use wry::WebViewExtMacOS as _;
        holds_keyboard(&self.webview.webview())
    }

    /// Names the GPUI view whose keyboard the page may share (`None`: none): while GPUI's keyboard
    /// is elsewhere, the page gives AppKit's back ([`reconcile`]).
    pub fn set_keyboard_host(&self, host: Option<&gpui_kit::FocusHandle>) {
        use wry::WebViewExtMacOS as _;
        set_host(&self.webview.webview(), host);
    }
}
