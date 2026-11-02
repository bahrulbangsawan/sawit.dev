//! The browser tab, without GPUI: the ops an agent sends it (over
//! `app_control`'s `Command::Browser`), what the address field turns typed
//! text into, the console and network logs the page agent reports, and the
//! page agent itself ([`AGENT_JS`]).
//!
//! The agent is injected into every page. It answers ops synchronously
//! (through `evaluate_script_with_callback`) and reports console messages,
//! requests and URL changes over IPC as `<token>\n<kind>\n<json>`; messages
//! without the webview's random token come from page scripts and are dropped.

mod address;
mod base64;
mod evaluate;
mod ipc;
mod logs;
pub mod mesurer;
mod ops;
mod page;
mod project;
mod redact;
mod viewport;

#[cfg(test)]
mod tests;

pub use address::{normalize_address, search_url};
pub use base64::{base64_decode, base64_encode};
pub use evaluate::{evaluate_as_body, evaluate_call};
pub use ipc::{PageEvent, parse_ipc};
pub use logs::{ConsoleEntry, Logs, MAX_CONSOLE, MAX_NETWORK, NetworkEntry};
pub use ops::{BrowserOp, NOT_OPEN, NOT_READY, Target, UNREACHABLE};
pub use page::{
    AGENT_JS, DESIGN_JS, MODULES_PLACEHOLDER, OVERLAY_JS, TOKEN_PLACEHOLDER, page_script,
};
pub use project::{
    data_store_id, dev_server_url, dev_servers, front_project, pick_dev_server, project_holding,
};
pub use redact::REDACTED;
pub(crate) use redact::redact_url;
pub use viewport::{DEVICES, Device, Fit, ViewportMode, device_by_name, fit, resize_mode};
