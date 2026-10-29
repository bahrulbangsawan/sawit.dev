//! Bottom-dock terminal: tabs of real terminals (shell, herdr or SSH).

mod panel;
mod view;

pub use panel::{JobTag, Remote, TerminalPanel, ToggleMaximize};

pub fn init(cx: &mut gpui_kit::App) {
    cx.bind_keys(view::bindings());
}
