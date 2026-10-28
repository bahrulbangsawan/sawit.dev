mod assets;
mod changelog_page;
mod cli;
mod db;
mod editor;
mod file_icons;
mod files;
mod layout;
mod mcp_page;
mod prompts;
mod rt;
mod servers;
mod tabs;
mod terminal;
mod theme;
mod update;
mod viewers;
mod workspace;

use sawit_core::rt::Runtime;

/// GUI apps launched from Finder get a minimal PATH; adopt the login shell's
/// so docker, brew, herdr and friends are found (as Zed does).
fn adopt_login_shell_path() {
    let probe = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("probe runtime");
    if let Some(path) = probe.block_on(sawit_core::probe::login_shell_path()) {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { std::env::set_var("PATH", path) };
    }
}

fn main() {
    // `sawit .`, `sawit file.rs:42`, … like `zed`: from a terminal this
    // hands the paths to the running app and exits.
    let request = cli::command_line();
    adopt_login_shell_path();
    let runtime = Runtime::start();
    let app = gpui_kit::application().with_assets(assets::AppAssets);
    app.on_reopen(cli::reopen);
    app.run(move |cx| {
        gpui_kit::init(cx);
        editor::init(cx);
        db::init(cx);
        workspace::init(cx);
        theme::apply_zed_one_dark(cx);
        cx.set_global(rt::Rt(runtime));
        cli::init(cx);
        update::init(cx);
        cli::handle(request, None, cx);
        cli::serve(cx);
    });
}
