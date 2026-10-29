mod assets;
mod canvas;
mod changelog_page;
mod cli;
mod code_actions;
mod db;
mod editor;
mod file_icons;
mod files;
mod git;
mod highlight;
mod layout;
mod mcp_page;
mod network;
mod ports;
mod prompts;
mod rt;
mod run;
mod servers;
mod shell_config;
mod ssh;
mod states;
mod tab_groups;
mod tabs;
mod terminal;
mod theme;
mod tools_page;
mod update;
mod viewers;
mod welcome;
mod workspace;

use sawit_core::rt::Runtime;

/// GUI apps launched from Finder get a minimal PATH; adopt the login shell's
/// so docker, brew, herdr and friends are found (as Zed does).
///
/// Asking the login shell takes 0.3–1.5 s with a heavy shell config, and
/// PATH must be set before any other thread exists. So the answer is cached:
/// only the first launch waits for the shell; later ones use the cached PATH
/// at once and refresh the cache in the background (`refresh_login_path`).
/// Returns whether the cached PATH was used.
fn adopt_login_shell_path() -> bool {
    let cache = login_path_cache();
    let cached = std::fs::read_to_string(&cache)
        .ok()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let from_cache = cached.is_some();
    let path = cached.or_else(|| {
        let probe = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("probe runtime");
        let path = probe.block_on(sawit_core::probe::login_shell_path())?;
        save_login_path(&cache, &path);
        Some(path)
    });
    if let Some(path) = path {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { std::env::set_var("PATH", path) };
    }
    from_cache
}

/// Asks the login shell again and caches its PATH for the next launch (this
/// process keeps the one it started with: PATH can't change safely now).
fn refresh_login_path(runtime: &Runtime) {
    let cache = login_path_cache();
    drop(runtime.spawn(async move {
        if let Some(path) = sawit_core::probe::login_shell_path().await {
            save_login_path(&cache, &path);
        }
    }));
}

fn login_path_cache() -> std::path::PathBuf {
    sawit_core::instances::AppPaths::default_macos()
        .cache_dir()
        .join("login-path")
}

fn save_login_path(cache: &std::path::Path, path: &str) {
    if let Some(dir) = cache.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(cache, path);
}

/// Whether this process was started as `sawit-mcp`. The app bundle ships
/// one binary: `sawit-mcp` is a symlink to `sawit`, so the MCP server
/// doesn't carry a second copy of sawit-core (QuickJS, Postgres, TLS, …).
fn invoked_as_mcp() -> bool {
    std::env::args_os()
        .next()
        .is_some_and(|arg0| std::path::Path::new(&arg0).file_name() == Some("sawit-mcp".as_ref()))
}

fn main() -> std::process::ExitCode {
    if invoked_as_mcp() {
        return sawit_mcp::main();
    }
    // Run by `ssh` as SSH_ASKPASS for a server whose password is saved:
    // answer the prompt and exit (see sawit_core::ssh::askpass_main).
    if let Some(account) = std::env::var_os(sawit_core::ssh::ASKPASS_ENV) {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        std::process::exit(sawit_core::ssh::askpass_main(
            &account.to_string_lossy(),
            &prompt,
        ));
    }
    // `sawit .`, `sawit file.rs:42`, … like `zed`: from a terminal this
    // hands the paths to the running app and exits.
    let request = cli::command_line();
    let path_from_cache = adopt_login_shell_path();
    let runtime = Runtime::start();
    if path_from_cache {
        refresh_login_path(&runtime);
    }
    let app = gpui_kit::application().with_assets(assets::AppAssets);
    app.on_reopen(cli::reopen);
    app.run(move |cx| {
        gpui_kit::init(cx);
        editor::init(cx);
        files::init(cx);
        shell_config::init(cx);
        db::init(cx);
        workspace::init(cx);
        theme::apply_zed_one_dark(cx);
        cx.set_global(rt::Rt(runtime));
        highlight::prewarm(cx);
        cli::init(cx);
        update::init(cx);
        cli::handle(request, None, cx);
        cli::serve(cx);
    });
    std::process::ExitCode::SUCCESS
}
