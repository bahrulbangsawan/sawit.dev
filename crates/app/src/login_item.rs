//! Open at Login: Sawit as a login item through `SMAppService.mainApp` (macOS 13+), off until
//! the user turns it on. The state is always read back from the system, never kept by Sawit, so
//! System Settings › Login Items and the switch agree.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{msg_send, sel};

#[link(name = "ServiceManagement", kind = "framework")]
unsafe extern "C" {}

/// `SMAppServiceStatus`: not registered, enabled, waiting for approval, not found.
const ENABLED: isize = 1;
const REQUIRES_APPROVAL: isize = 2;
const NOT_FOUND: isize = 3;

fn main_app() -> Option<Retained<AnyObject>> {
    let class = AnyClass::get(c"SMAppService")?;
    if !class.responds_to(sel!(mainAppService)) {
        return None;
    }
    unsafe { msg_send![class, mainAppService] }
}

/// Whether Sawit opens at login; `None` where it can't (a build run outside its app bundle).
pub fn status() -> Option<bool> {
    let app = main_app()?;
    let status: isize = unsafe { msg_send![&*app, status] };
    match status {
        NOT_FOUND => None,
        ENABLED | REQUIRES_APPROVAL => Some(true),
        _ => Some(false),
    }
}

/// Turns Open at Login on or off; the switch reads [`status`] again after.
pub fn set(on: bool) {
    let Some(app) = main_app() else { return };
    let mut error: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = unsafe {
        if on {
            msg_send![&*app, registerAndReturnError: &mut error]
        } else {
            msg_send![&*app, unregisterAndReturnError: &mut error]
        }
    };
    if !ok {
        log::warn!(
            "Open at Login couldn't be {}",
            if on { "turned on" } else { "turned off" }
        );
    }
}
