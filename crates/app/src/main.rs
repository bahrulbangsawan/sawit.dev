mod assets;
mod db;
mod editor;
mod file_icons;
mod files;
mod layout;
mod mcp_page;
mod rt;
mod servers;
mod tabs;
mod terminal;
mod theme;
mod viewers;
mod workspace;

use gpui_kit::component::TitleBar;
use gpui_kit::*;
use pgdesk_core::rt::Runtime;

/// GUI apps launched from Finder get a minimal PATH; adopt the login shell's
/// so docker, brew, herdr and friends are found (as Zed does).
fn adopt_login_shell_path() {
    let probe = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("probe runtime");
    if let Some(path) = probe.block_on(pgdesk_core::probe::login_shell_path()) {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { std::env::set_var("PATH", path) };
    }
}

fn main() {
    // `pgdesk [folder]` opens a project, like `zed .`.
    let project = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .and_then(|p| p.canonicalize().ok())
        .filter(|p| p.is_dir());
    adopt_login_shell_path();
    let runtime = Runtime::start();
    gpui_kit::application()
        .with_assets(assets::AppAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            editor::init(cx);
            db::init(cx);
            workspace::init(cx);
            theme::apply_zed_one_dark(cx);
            cx.set_global(rt::Rt(runtime));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
                window_min_size: Some(size(px(720.), px(480.))),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                window.set_window_title("PG Desk");
                cx.new(|cx| {
                    let mut workspace = workspace::Workspace::new(window, cx);
                    if let Some(project) = project.clone() {
                        workspace.open_project(project, cx);
                    }
                    workspace
                })
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
