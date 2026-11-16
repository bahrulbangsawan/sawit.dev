mod accounts;
mod activity;
mod agents;
mod assets;
mod board;
mod browser;
mod catalog;
mod changelog_page;
mod cli;
mod code_actions;
mod confirm;
mod db;
mod editor;
mod file_icons;
mod files;
mod find;
mod git;
mod highlight;
mod keymap;
#[cfg(test)]
mod keys_tests;
mod layout;
mod local_explorer;
mod login_item;
mod mcp_page;
mod memory;
mod menu_icons;
#[cfg(test)]
mod menu_tests;
mod menus;
mod native_cover;
mod network;
mod new_project_page;
mod orchestra;
mod package_managers;
mod permissions_page;
mod pomodoro;
mod ports;
mod prompts;
mod rt;
mod run;
mod sawit_skills;
mod servers;
mod shell_config;
mod skills_page;
mod ssh;
mod states;
mod tab_groups;
mod tabs;
mod terminal;
#[cfg(test)]
mod testing;
mod theme;
mod tools_page;
mod update;
mod viewers;
mod welcome;
mod workspace;

use sawit_core::rt::Runtime;

/// Large blocks go back to the system when freed ([`memory`]).
#[global_allocator]
static ALLOCATOR: memory::SawitAlloc = memory::SawitAlloc;

/// GUI apps launched from Finder get a minimal PATH; adopt the login shell's
/// so docker, brew and friends are found (as Zed does).
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
/// Whether this process was started under the name `name` (a link to the one binary).
fn invoked_as(name: &str) -> bool {
    std::env::args_os()
        .next()
        .is_some_and(|arg0| std::path::Path::new(&arg0).file_name() == Some(name.as_ref()))
}

fn invoked_as_mcp() -> bool {
    std::env::args_os()
        .next()
        .is_some_and(|arg0| std::path::Path::new(&arg0).file_name() == Some("sawit-mcp".as_ref()))
}

fn main() -> std::process::ExitCode {
    // An Agent Account's `statusLine` runs Sawit as its usage collector: first, and nothing else,
    // since Claude Code runs it on every status line update.
    if invoked_as(sawit_core::agent_accounts::collector::PROGRAM) {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::process::exit(sawit_core::agent_accounts::collector::main(&args));
    }
    // `cp2`: Claude Code as account 2, in this terminal (a link to this binary).
    if let Some(n) = std::env::args_os()
        .next()
        .and_then(|a| {
            std::path::Path::new(&a)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
        })
        .and_then(|name| sawit_core::agent_accounts::shell::shortcut_number(&name))
    {
        let rest: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        std::process::exit(sawit_core::agent_accounts::cli::shortcut_main(n, &rest));
    }
    // `sawit account …`: the Accounts page from a terminal.
    if std::env::args_os().nth(1).is_some_and(|a| a == "account") {
        let rest: Vec<std::ffi::OsString> = std::env::args_os().skip(2).collect();
        std::process::exit(sawit_core::agent_accounts::cli::main(&rest));
    }
    // `RUST_LOG=sawit=debug` prints the debug log (the canvas's render times, …) on stderr. Without
    // RUST_LOG no logger is installed, and log calls do nothing.
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = env_logger::try_init();
    }
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
        find::init(cx);
        files::init(cx);
        shell_config::init(cx);
        catalog::init(cx);
        catalog::keep_page_sizes(
            sawit_core::instances::AppPaths::default_macos()
                .root
                .join("page-sizes.json"),
            cx,
        );
        db::init(cx);
        git::init(cx);
        workspace::init(cx);
        agents::init(cx);
        agents::keep_setting(
            sawit_core::instances::AppPaths::default_macos()
                .root
                .join("agent-cursors.json"),
            cx,
        );
        pomodoro::keep_setting(
            sawit_core::instances::AppPaths::default_macos()
                .root
                .join("pomodoro.json"),
            cx,
        );
        terminal::init(cx);
        theme::apply_zed_one_dark(cx);
        cx.set_global(rt::Rt(runtime));
        // The agent runtime: after the Tokio runtime it runs on.
        orchestra::init(cx);
        // The saved GitHub sign-in, before any window shows it. It never starts a sign-in.
        git::account::init(cx);
        accounts::init(cx);
        highlight::prewarm(cx);
        cli::init(cx);
        sawit_skills::init(cx);
        update::init(cx);
        // Last of the key bindings: what every feature bound above is the defaults.
        keymap::init(cx);
        cli::handle(request, None, cx);
        // Unsaved changes the last quit kept, because it couldn't ask, come back.
        cli::restore_kept(cx);
        cli::serve(cx);
    });
    std::process::ExitCode::SUCCESS
}
