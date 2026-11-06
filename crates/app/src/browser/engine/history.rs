//! The page's history as WebKit keeps it (`interactionState`: the back and forward list, the
//! scroll position and form state), so a tab can give its web view up and get the same page and
//! history back, in this run or the next.

use objc2::rc::Retained;
use objc2_foundation::NSData;
use wry::WebViewExtMacOS as _;

use super::Engine;

impl Engine {
    /// The page's history, as opaque bytes; `None` when WebKit has none to give (a blank page).
    pub fn history(&self) -> Option<Vec<u8>> {
        let wk = self.webview.webview();
        let state = unsafe { wk.interactionState() }?;
        let data: Retained<NSData> = state.downcast::<NSData>().ok()?;
        let bytes = data.to_vec();
        (!bytes.is_empty()).then_some(bytes)
    }

    /// Puts back a history [`Self::history`] gave: WebKit goes to its current page, with back
    /// and forward as they were.
    pub fn restore_history(&self, bytes: &[u8]) {
        let wk = self.webview.webview();
        let data = NSData::with_bytes(bytes);
        unsafe { wk.setInteractionState(Some(&data)) };
    }
}
