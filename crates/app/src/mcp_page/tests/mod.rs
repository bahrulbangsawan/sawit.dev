//! Tests for the MCP page, one file per topic, and the window they open it in.

use std::path::Path;

use gpui_kit::{
    AppContext as _, Bounds, Entity, Modifiers, TestAppContext, VisualTestContext, WindowBounds,
    WindowOptions, point, px, size,
};
use sawit_core::mcp_clients::{Client, Status, Target};

use super::{Detected, McpPage, SERVERS, Sheet};

mod github;
mod installs;
mod keyboard;
mod region;

fn target(client: Client, config: &Path) -> Target {
    Target {
        client,
        profile: "default".into(),
        config: config.to_path_buf(),
        profile_dir: None,
    }
}

/// The page in a window, with `targets` detected and `setup` applied, before any reload.
fn open(
    cx: &mut TestAppContext,
    targets: Vec<Target>,
    setup: impl FnOnce(&mut McpPage),
) -> (Entity<McpPage>, &mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start()));
    });
    // Installs and removals finish on the Tokio runtime's threads.
    cx.executor().allow_parking();
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(1200.), px(900.)),
        })),
        ..Default::default()
    };
    let (window, page) = cx
        .update(|cx| {
            gpui_kit::open_window(options, cx, |_, cx| {
                cx.new(|cx| {
                    let mut page = McpPage::blank(cx);
                    page.targets = targets;
                    page.loading = false;
                    setup(&mut page);
                    page
                })
            })
        })
        .unwrap();
    let cx = VisualTestContext::from_window(window, cx).into_mut();
    cx.run_until_parked();
    (page, cx)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} isn't drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}
