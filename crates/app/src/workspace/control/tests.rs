//! The window commands' tests: where an agent's browser, canvas and shell go, and what commands
//! answer.

use std::path::{Path, PathBuf};

use sawit_core::files::FileKind;
use serde_json::json;

use super::targets::canvas_file_kind;
use super::{
    BrowserRoute, agent_project, browser_route, close_answer, script_answer, shell_folder,
};
use super::{agent_shell_folder, split_dir};

/// An agent that opens the GitHub page while the user is signed out is told to wait for
/// them, in the words the other GitHub answers use: it can't sign in itself.
#[test]
fn the_github_page_tells_agents_to_wait_for_the_users_sign_in() {
    assert!(super::GITHUB_SIGNED_OUT.starts_with("Sign in to GitHub in Sawit first"));
    assert!(super::GITHUB_SIGNED_OUT.contains("Agents can't sign in"));
    assert!(super::GITHUB_SIGNED_OUT.contains("app_state"));
}

/// An agent's new terminal (terminal_run with new_tab and no cwd) starts in the agent's own
/// project, as its browser opens on that project's dev server: with projects A and B in one
/// window and B shown, an agent in A ran its command in B's folder. A folder it names wins,
/// and an agent that works in no project of the window gets the active one.
#[test]
fn an_agents_new_terminal_starts_in_its_own_project() {
    let roots = [PathBuf::from("/code/shop"), PathBuf::from("/code/blog")];
    let shown = Some(PathBuf::from("/code/blog"));
    let folder = |cwd: Option<&str>, agent: Option<&str>| {
        agent_shell_folder(
            cwd.map(PathBuf::from),
            &roots,
            agent.map(Path::new),
            shown.clone(),
        )
    };
    assert_eq!(
        folder(None, Some("/code/shop/web")),
        Some(PathBuf::from("/code/shop"))
    );
    assert_eq!(
        folder(Some("/code/blog/api"), Some("/code/shop")),
        Some(PathBuf::from("/code/blog/api"))
    );
    assert_eq!(folder(None, Some("/elsewhere")), shown);
    assert_eq!(folder(None, None), shown);
}

/// terminal_split's right and down are the pane menu's Split right (⌘D) and Split down (⇧⌘D).
#[test]
fn a_split_goes_the_way_the_agent_names() {
    use sawit_core::app_control::SplitDirection;
    use sawit_core::terminal::SplitDir;
    assert_eq!(split_dir(SplitDirection::Right), SplitDir::Right);
    assert_eq!(split_dir(SplitDirection::Down), SplitDir::Down);
}

/// MCP-4: a shell an agent starts in a folder starts there or not at all. The PTY ignores a
/// `chdir` that fails, so a relative folder or one that isn't there started the shell in the
/// app's own folder (`/` for Sawit.app), and the agent's command ran there.
#[test]
fn a_shell_starts_in_a_folder_that_is_there_or_not_at_all() {
    let here = std::env::temp_dir().canonicalize().unwrap();
    assert_eq!(shell_folder(None), Ok(None));
    assert_eq!(shell_folder(Some(here.clone())), Ok(Some(here)));
    for (dir, why) in [
        ("web", "isn't an absolute path"),
        ("/no/such/folder", "isn't a folder"),
    ] {
        let refused = shell_folder(Some(PathBuf::from(dir))).unwrap_err();
        assert!(refused.contains(why), "{dir}: {refused}");
    }
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert!(shell_folder(Some(file)).is_err());
}

/// app_run_script names the terminal its script runs in, or each terminal of a saved command
/// that runs several: the agent's terminal_read and terminal_input need it, as the terminal in
/// front may be the one the user types in.
#[test]
fn a_scripts_answer_says_where_it_runs() {
    assert_eq!(
        script_answer("dev", false, &[("dev".into(), 2, 0)]),
        json!({ "started": "dev", "terminal": 2, "pane": 0 })
    );
    assert_eq!(
        script_answer("dev", true, &[("dev".into(), 2, 1)]),
        json!({
            "running": "dev",
            "note": "It was running already, so it wasn't started again. app_stop_script stops it.",
            "terminal": 2,
            "pane": 1,
        })
    );
    let both = [("both 1".to_string(), 3, 0), ("both 2".to_string(), 4, 0)];
    assert_eq!(
        script_answer("both", false, &both),
        json!({
            "started": "both",
            "terminals": [
                { "name": "both 1", "terminal": 3, "pane": 0 },
                { "name": "both 2", "terminal": 4, "pane": 0 },
            ],
        })
    );
}

/// app_close_tab says "closed" only when the tab closed. A tab that would lose something (a
/// file's unsaved changes, the browser's notes not yet sent, a Dev Tools install) asks the
/// user first and closes only if they agree, so its answer says a question is up: it said
/// "closed" before the user had answered.
#[test]
fn closing_a_tab_says_when_the_user_is_asked_first() {
    assert_eq!(close_answer(2, 4, None), json!({ "closed": 2, "tabs": 4 }));
    let asked = close_answer(2, 5, Some("the file has unsaved changes"));
    assert_eq!(asked.get("closed"), None, "{asked}");
    assert_eq!(asked["asking"], json!(2), "{asked}");
    assert_eq!(asked["tabs"], json!(5), "{asked}");
    let note = asked["note"].as_str().unwrap_or_default();
    assert!(note.contains("the file has unsaved changes"), "{asked}");
    assert!(note.contains("app_state"), "{asked}");
}

/// An agent's `browser_console` or `browser_annotations` with no browser open opened one
/// over the user's tab, to say "No annotations yet"; only ops that show a page open it.
#[test]
fn only_ops_that_show_a_page_open_the_browser() {
    use BrowserRoute::{Closed, Show, Use};
    // open, shown, shows_tab
    assert_eq!(browser_route(true, false, false), Use, "in the background");
    assert_eq!(browser_route(true, true, true), Use);
    assert_eq!(browser_route(true, false, true), Show, "brought forward");
    assert_eq!(browser_route(false, false, true), Show, "opened");
    assert_eq!(browser_route(false, false, false), Closed);
}

/// An agent's browser is the browser of the project it works in (final-seams m1), whose dev
/// server its browser_navigate opens: with projects A and B in one window and B shown, an
/// agent in A opened A's dev server in B's browser, with B's cookies, and its screenshots and
/// exports went into B's `.nyawit`. An agent that names its project, or runs in no project
/// of the window, gets the active project's: the one it named, made active.
#[test]
fn an_agents_browser_is_its_own_projects() {
    let roots = [
        PathBuf::from("/code/shop"),
        PathBuf::from("/code/shop/admin"),
        PathBuf::from("/code/blog"),
    ];
    let shown = Some(PathBuf::from("/code/blog"));
    let browser = |agent: Option<&str>| agent_project(&roots, agent.map(Path::new), shown.clone());
    assert_eq!(
        browser(Some("/code/shop/web")),
        Some(PathBuf::from("/code/shop"))
    );
    assert_eq!(
        browser(Some("/code/shop")),
        Some(PathBuf::from("/code/shop"))
    );
    assert_eq!(
        browser(Some("/code/shop/admin/src")),
        Some(PathBuf::from("/code/shop/admin")),
        "the deepest project holding it"
    );
    assert_eq!(browser(None), shown, "named, or run from no project");
    assert_eq!(browser(Some("/elsewhere")), shown);
    assert_eq!(
        agent_project(&roots, None, None),
        None,
        "the browser for no project"
    );
}

/// Q12: an agent's canvas is the canvas of the project it works in, as its browser is. With
/// projects A and B in one window and B shown, an agent in A drew on (or cleared) B's canvas,
/// while with the app closed the same call edited A's: it got another board depending on
/// whether Sawit ran. An agent that names its project gets that one's, made active.
#[test]
fn an_agents_canvas_is_its_own_projects() {
    let (a, b) = (PathBuf::from("/code/a"), PathBuf::from("/code/b"));
    let roots = [a.clone(), b.clone()];
    let canvas = |agent: Option<&str>, active: &Path| {
        agent_project(&roots, agent.map(Path::new), Some(active.to_path_buf()))
    };
    assert_eq!(canvas(Some("/code/a"), &b), Some(a.clone()));
    assert_eq!(canvas(Some("/code/a/web/src"), &b), Some(a.clone()));
    assert_eq!(canvas(None, &a), Some(a.clone()), "named, and made active");
    assert_eq!(
        canvas(Some("/elsewhere"), &b),
        Some(b),
        "run from no project of it"
    );
}

/// An agent's canvas op names a canvas file by its absolute path: a `.sawit` file, or an
/// `.excalidraw` file, whose canvas is bound to the `.sawit` file beside it. Anything else is
/// the op's error, and no tab opens for it.
#[test]
fn an_agents_canvas_file_is_a_sawit_or_excalidraw_file_by_its_absolute_path() {
    let kind = |file: &str| canvas_file_kind(Path::new(file));
    assert_eq!(kind("/p/design/home.sawit"), Ok(FileKind::Canvas));
    assert_eq!(kind("/p/Old.EXCALIDRAW"), Ok(FileKind::Excalidraw));
    for (file, says) in [
        ("design/home.sawit", "isn't an absolute path"),
        ("/p/notes.md", "isn't a canvas file"),
        ("/p/.nyawit/canvas.sawit.bak", "isn't a canvas file"),
        // Regression: the canvas engine's own extension is refused like any other file.
        (
            "/p/design/home.figcraft",
            "name a .sawit or .excalidraw file",
        ),
    ] {
        let refused = kind(file).unwrap_err();
        assert!(refused.contains(says), "{file}: {refused}");
    }
}
