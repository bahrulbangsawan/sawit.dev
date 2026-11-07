//! Agents' clicks as real mouse events (`NSEvent`s handed to the `WKWebView`), so the page gets
//! trusted events: `isTrusted`, real `:active`, native controls, user activation, sites that
//! ignore synthetic events. (A mouse-moved event handed to the view this way never reaches the
//! page, so hovers stay the page agent's.)
//!
//! The events go straight to the view's own `mouseMoved:`, `mouseDown:` and `mouseUp:`, never
//! through the window (`sendEvent:`): AppKit doesn't make the view first responder for them, the
//! app's mouse-down monitor ([`super::keyboard`]) doesn't count them as the user's press, and the
//! pointer the user sees doesn't move. They need the view on screen; a hidden tab keeps the page
//! agent's synthetic events.

use objc2::rc::Retained;
use objc2_app_kit::{NSEvent, NSEventModifierFlags, NSEventType, NSView};
use objc2_foundation::{NSPoint, NSProcessInfo};
use wry::WebViewExtMacOS as _;

use super::Engine;

impl Engine {
    /// The view, when it can take real mouse events: in a window, not hidden, and some of it on
    /// screen.
    fn pointer_view(&self) -> Option<Retained<NSView>> {
        let wk = self.webview.webview();
        let view: Retained<NSView> = Retained::into_super(Retained::into_super(wk));
        let shown = view.window().is_some() && !view.isHiddenOrHasHiddenAncestor() && {
            let visible = view.visibleRect();
            visible.size.width > 0. && visible.size.height > 0.
        };
        shown.then_some(view)
    }

    /// Whether [`Self::native_click`] can reach the page now.
    pub fn takes_native_pointer(&self) -> bool {
        self.pointer_view().is_some()
    }

    /// A left click (twice for `double`) at (`x`, `y`), in CSS pixels of the page's viewport,
    /// which is `viewport` CSS pixels (`innerWidth`, `innerHeight`) over the view's bounds,
    /// whatever its zoom or device scale: the pointer moves there, then presses and lets go.
    pub fn native_click(
        &self,
        double: bool,
        (x, y): (f64, f64),
        viewport: (f64, f64),
    ) -> Result<(), String> {
        let view = self
            .pointer_view()
            .ok_or("The page isn't on screen.".to_string())?;
        let window = view.window().ok_or("The page isn't in a window.")?;
        let bounds = view.bounds();
        if viewport.0 <= 0. || viewport.1 <= 0. {
            return Err("The page has no size.".into());
        }
        let (px, py) = (
            x * bounds.size.width / viewport.0,
            y * bounds.size.height / viewport.1,
        );
        let local = if view.isFlipped() {
            NSPoint::new(bounds.origin.x + px, bounds.origin.y + py)
        } else {
            NSPoint::new(
                bounds.origin.x + px,
                bounds.origin.y + bounds.size.height - py,
            )
        };
        let at = view.convertPoint_toView(local, None);
        let number = window.windowNumber();
        let event = |kind: NSEventType, clicks: isize| {
            let time = NSProcessInfo::processInfo().systemUptime();
            NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                kind,
                at,
                NSEventModifierFlags::empty(),
                time,
                number,
                None,
                0,
                clicks,
                if kind == NSEventType::LeftMouseDown { 1. } else { 0. },
            )
            .ok_or_else(|| "macOS couldn't make the mouse event.".to_string())
        };
        let moved: Retained<NSEvent> = event(NSEventType::MouseMoved, 0)?;
        view.mouseMoved(&moved);
        for clicks in 1..=if double { 2 } else { 1 } {
            let down: Retained<NSEvent> = event(NSEventType::LeftMouseDown, clicks)?;
            view.mouseDown(&down);
            let up: Retained<NSEvent> = event(NSEventType::LeftMouseUp, clicks)?;
            view.mouseUp(&up);
        }
        Ok(())
    }
}
