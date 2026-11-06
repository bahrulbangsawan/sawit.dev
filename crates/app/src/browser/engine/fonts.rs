//! The Mac's font changes, for the Design panel: CoreText's notification, observed once on the
//! local notification center and handed to every listener.

use std::ffi::c_void;
use std::sync::Mutex;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2_core_foundation::{
    CFDictionary, CFNotificationCenter, CFNotificationName, CFNotificationSuspensionBehavior,
    CFString,
};

/// Who hears of the Mac's font changes ([`font_changes`]).
static FONT_LISTENERS: Listeners = Mutex::new(Vec::new());

/// Who hears of a notification: a sender each, dropped once its receiver goes.
type Listeners = Mutex<Vec<UnboundedSender<()>>>;

/// A message each time fonts are installed on this Mac or removed from it: CoreText's
/// `kCTFontManagerRegisteredFontsChangedNotification`, observed once for the whole app on the
/// local notification center. A listener whose receiver went is dropped as the next listener
/// comes or the fonts next change.
pub fn font_changes() -> UnboundedReceiver<()> {
    static OBSERVED: std::sync::Once = std::sync::Once::new();
    // SAFETY: CoreText's own name.
    OBSERVED.call_once(|| {
        observe(&FONT_LISTENERS, unsafe {
            kCTFontManagerRegisteredFontsChangedNotification
        })
    });
    listen(&FONT_LISTENERS)
}

/// Has the local notification center tell `listeners` of each `name` posted.
fn observe(listeners: &'static Listeners, name: &CFString) {
    let Some(center) = CFNotificationCenter::local_center() else {
        return;
    };
    // SAFETY: the observer is `listeners`, a static, which the callback reads and nothing else.
    unsafe {
        center.add_observer(
            (listeners as *const Listeners).cast(),
            Some(notified),
            Some(name),
            std::ptr::null(),
            CFNotificationSuspensionBehavior::DeliverImmediately,
        );
    }
}

/// A new listener on `listeners`. Closed listeners (a browser tab's that went) are dropped first,
/// so they don't pile up until the next notification.
fn listen(listeners: &Listeners) -> UnboundedReceiver<()> {
    let (tx, rx) = unbounded();
    let mut listeners = listeners.lock().unwrap_or_else(|e| e.into_inner());
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

/// A notification [`observe`]d was posted, on whichever thread posted it: every listener of the
/// observer's list (`observer`) hears of it.
unsafe extern "C-unwind" fn notified(
    _: *mut CFNotificationCenter,
    observer: *mut c_void,
    _: *const CFNotificationName,
    _: *const c_void,
    _: *const CFDictionary,
) {
    // SAFETY: `observe` registers a `&'static Listeners` as the observer.
    let Some(listeners) = (unsafe { observer.cast::<Listeners>().as_ref() }) else {
        return;
    };
    let mut listeners = listeners.lock().unwrap_or_else(|e| e.into_inner());
    listeners.retain(|tx| tx.unbounded_send(()).is_ok());
}

#[cfg(test)]
mod tests {
    /// A notification observed reaches every listener; one that went is left out.
    ///
    /// The test observes a name of its own, with a list of its own: posting CoreText's real
    /// notification would also reach the app's listener list, which other tests' browser views
    /// (on their own GPUI test schedulers, on other threads) listen on, and wake their tasks from
    /// this thread, which their scheduler reports as non-deterministic and fails them for.
    #[test]
    fn notifications_reach_every_listener() {
        static LISTENERS: super::Listeners = std::sync::Mutex::new(Vec::new());
        let name = objc2_core_foundation::CFString::from_str("dev.sawit.test.notified");
        super::observe(&LISTENERS, &name);
        let mut first = super::listen(&LISTENERS);
        let mut second = super::listen(&LISTENERS);
        drop(super::listen(&LISTENERS));
        let center = objc2_core_foundation::CFNotificationCenter::local_center().unwrap();
        // SAFETY: a name of the test's own, with no object and no user info.
        unsafe { center.post_notification(Some(&name), std::ptr::null(), None, true) };
        assert_eq!(first.try_recv(), Ok(()));
        assert_eq!(second.try_recv(), Ok(()));
        assert_eq!(
            LISTENERS.lock().unwrap().len(),
            2,
            "the dropped one is gone"
        );
    }
}
