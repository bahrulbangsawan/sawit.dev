//! The `sawit` command's tests: where folders, files and agents' commands go, `--wait`, the
//! menus and their keys, and finding the app's bundle.

use super::open::window_with_root;
use super::server::{canvas_file_window, window_holding, window_naming};
use super::{bundle_of, still_open};
use gpui_kit::{KeyContext, MenuItem, OperatingSystem, OsAction, TestAppContext};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// CANVAS-UI-7: `sawit --wait` on a canvas file waits while its canvas is open: an
/// `.excalidraw` file's canvas is bound to the `.sawit` file beside it, which its tab
/// shows, and a project's canvas file is shown by the project's canvas tab. It answered "done"
/// at the first change of the window.
#[test]
fn a_wait_on_a_canvas_file_lasts_while_its_canvas_is_open() {
    let open: HashSet<PathBuf> = [
        PathBuf::from("/p/sketch.sawit"),
        PathBuf::from("/p/.nyawit/canvas.sawit"),
        PathBuf::from("/p/notes.md"),
    ]
    .into();
    let waits = |file: &str| still_open(&[PathBuf::from(file)], &open);
    assert!(waits("/p/sketch.excalidraw"));
    assert!(waits("/p/sketch.sawit"));
    assert!(waits("/p/.nyawit/canvas.sawit"));
    assert!(waits("/p/notes.md"));
    assert!(!waits("/p/other.excalidraw"));
    assert!(!waits("/p/notes.txt"));
    // Waiting on several, one of them open is enough.
    let files = [
        PathBuf::from("/p/gone.md"),
        PathBuf::from("/p/sketch.excalidraw"),
    ];
    assert!(still_open(&files, &open));
}

/// Control commands go to the window whose project holds the folder asked about (a project
/// named, or the agent's own folder): the deepest project wins.
#[test]
fn a_folder_goes_to_the_window_with_its_deepest_project() {
    let windows = vec![
        ("front", vec![PathBuf::from("/code/shop")]),
        (
            "back",
            vec![PathBuf::from("/code"), PathBuf::from("/code/api")],
        ),
    ];
    let window = |path: &str| window_holding(&windows, Path::new(path));
    assert_eq!(window("/code/api/src"), Some("back"));
    assert_eq!(window("/code/shop/web"), Some("front"));
    assert_eq!(window("/code/other"), Some("back"));
    assert_eq!(window("/tmp/elsewhere"), None);
}

/// An agent's op on a canvas file goes to the window that shows that file's canvas, whatever
/// project it names or works in: a second canvas of the file in another window would save
/// over the first's edits. Else to the window whose project holds the file, as `sawit <file>`
/// opens it there; a file that no window holds leaves the choice to the op's project or the
/// agent's folder.
#[test]
fn a_canvas_files_op_goes_to_the_window_that_shows_or_holds_it() {
    let windows = vec![
        ("front", vec![PathBuf::from("/code/shop")]),
        ("back", vec![PathBuf::from("/code/blog")]),
    ];
    let file = Path::new("/code/blog/design/home.sawit");
    assert_eq!(canvas_file_window(None, &windows, file), Some("back"));
    assert_eq!(
        canvas_file_window(Some("front"), &windows, file),
        Some("front"),
        "the window that shows it"
    );
    let elsewhere = Path::new("/elsewhere/home.sawit");
    assert_eq!(canvas_file_window(None, &windows, elsewhere), None);
}

/// SITE-1: ⌘Q, the app's own Quit (as the menu's Quit Sawit sends it), asks to save a file
/// with unsaved changes first. It quit at once, and the changes were lost.
#[gpui_kit::test]
fn the_quit_key_asks_to_save_unsaved_changes(cx: &mut gpui_kit::TestAppContext) {
    use crate::editor::testing::{open_editor, scratch_dir, type_into};
    cx.update(|cx| {
        crate::editor::testing::init(cx);
        super::init(cx);
    });
    let path = scratch_dir("quit-key").join("notes.txt");
    std::fs::write(&path, "on disk\n").unwrap();
    let (window, editor) = open_editor(cx, &path);
    type_into(cx, window, &editor, "typed ");

    cx.simulate_keystrokes(window, "cmd-q");
    cx.run_until_parked();
    let (question, _) = cx.pending_prompt().expect("⌘Q asked nothing");
    assert_eq!(question, "Save changes to notes.txt before quitting?");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "on disk\n");
}

/// The app's key bindings, bound as `main` binds them, then taken as the keymap's defaults
/// (with a keymap file of the test's own, which it never writes).
fn bind_like_the_app(cx: &mut gpui_kit::App, keymap_file: &Path) {
    gpui_kit::init(cx);
    crate::editor::init(cx);
    crate::files::init(cx);
    crate::shell_config::init(cx);
    crate::db::init(cx);
    crate::git::init(cx);
    crate::workspace::init(cx);
    crate::terminal::init(cx);
    super::init(cx);
    crate::keymap::init_at(keymap_file.to_path_buf(), cx);
}

/// The Edit menu's Cut, Copy, Paste and Select All carry ⌘X, ⌘C, ⌘V and ⌘A and go to the
/// view with AppKit's keyboard (`cut:`, `copy:`, `paste:`, `selectAll:`): a web page's
/// clipboard keys reach it that way, once no binding of Sawit's takes them.
#[gpui_kit::test]
fn the_edit_menu_gives_a_web_page_its_clipboard_keys(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("sawit-edit-menu-{}", std::process::id()));
    cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let menus = super::menus(cx);
        let edit = menus.iter().find(|m| m.name.as_ref() == "Edit");
        let found: Vec<(String, Option<String>, String)> = edit
            .map(|edit| {
                edit.items
                    .iter()
                    .filter_map(|item| match item {
                        MenuItem::Action {
                            name,
                            action,
                            os_action,
                            ..
                        } => Some((
                            name.to_string(),
                            os_action.map(|a| {
                                match a {
                                    OsAction::Cut => "Cut",
                                    OsAction::Copy => "Copy",
                                    OsAction::Paste => "Paste",
                                    OsAction::SelectAll => "SelectAll",
                                    OsAction::Undo => "Undo",
                                    OsAction::Redo => "Redo",
                                }
                                .to_string()
                            }),
                            keymap
                                .bindings_for_action(action.as_ref())
                                .next()
                                .map(|b| crate::keymap::names::unparse(b.keystrokes()))
                                .unwrap_or_default(),
                        )),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let expected = [
            ("Cut", "Cut", "cmd-x"),
            ("Copy", "Copy", "cmd-c"),
            ("Paste", "Paste", "cmd-v"),
            ("Select All", "SelectAll", "cmd-a"),
        ]
        .map(|(n, a, k)| (n.to_string(), Some(a.to_string()), k.to_string()));
        assert_eq!(found, expected);
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// The keymap editor shows each binding's action and context, and writes them into
/// keymap.json, where a later rename would break the user's binding: none of them still
/// calls the Config Files panel by its old name.
#[gpui_kit::test]
fn the_keymap_calls_config_files_by_its_name(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("sawit-config-names-{}", std::process::id()));
    cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let old: Vec<String> = keymap
            .bindings()
            .flat_map(|b| {
                [
                    b.action().name().to_string(),
                    b.predicate().map(|p| p.to_string()).unwrap_or_default(),
                ]
            })
            .filter(|name| name.contains("ShellConfig") || name.contains("shell_config"))
            .collect();
        assert!(old.is_empty(), "{old:?}");
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// A menu item carries the key of the first binding of its action that GPUI's macOS menu
/// builder finds in a Workspace > Pane > Editor focus, else of the first binding at all
/// (`find_or_first`), and AppKit runs the item for that key wherever the item's action is
/// handled: in Sawit, everywhere the workspace is. So each item's action has no binding, or
/// one that holds everywhere in a window. A key bound only in a view (the keymap editor's
/// ⌘E) would otherwise run the item from every other view, a web page's included.
#[gpui_kit::test]
fn menu_keys_hold_everywhere_in_a_window(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("sawit-menu-keys-{}", std::process::id()));
    cx.update(|cx| bind_like_the_app(cx, &dir.join("keymap.json")));
    let workspace = {
        let mut workspace = KeyContext::new_with_defaults(OperatingSystem::Mac);
        workspace.add("Workspace");
        let mut pane = KeyContext::new_with_defaults(OperatingSystem::Mac);
        pane.add("Pane");
        let mut editor = KeyContext::new_with_defaults(OperatingSystem::Mac);
        editor.add("Editor");
        pane.extend(&editor);
        workspace.extend(&pane);
        vec![workspace]
    };
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        for menu in super::menus(cx) {
            for item in &menu.items {
                // A system item (the Edit menu's) goes to the view with AppKit's keyboard,
                // not to a GPUI action, and only when no binding of Sawit's takes its key.
                let MenuItem::Action {
                    name,
                    action,
                    os_action: None,
                    ..
                } = item
                else {
                    continue;
                };
                let bindings: Vec<_> = keymap.bindings_for_action(action.as_ref()).collect();
                let everywhere = bindings
                    .iter()
                    .any(|b| b.predicate().is_none_or(|p| p.eval(&workspace)));
                assert!(
                    bindings.is_empty() || everywhere,
                    "{name}: {:?}",
                    bindings
                        .iter()
                        .map(|b| (
                            crate::keymap::names::unparse(b.keystrokes()),
                            b.predicate().map(|p| p.to_string())
                        ))
                        .collect::<Vec<_>>()
                );
            }
        }
    });
    let _ = std::fs::remove_dir_all(&dir);
}

/// MCP-2, MCP-3: an agent names an open project by another name of its folder: through a
/// link (`/tmp/p` for the project at `/private/tmp/p` on macOS), in another letter case (a
/// Mac's volumes ignore it), or with `..` in it. That is the open project, as it is for
/// `app_open` and the CLI, and the command goes to its window, named as the window names it.
/// Matched as written, it was "No Sawit window has … open. Open it with app_open first.",
/// while `app_open` of the same name answered that it had opened it.
#[test]
fn a_project_named_another_way_is_the_open_one() {
    let dir = std::env::temp_dir().join(format!("sawit-cli-{}-named", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("shop");
    std::fs::create_dir_all(project.join("web")).unwrap();
    let project = project.canonicalize().unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&project, &link).unwrap();
    let windows = vec![("window", vec![project.clone()])];
    let named = |path: &Path| window_naming(&windows, path);

    assert_eq!(named(&project), Some(("window", project.clone())));
    for other in [
        link.clone(),
        dir.join("SHOP"),
        project.join("web").join(".."),
    ] {
        assert_eq!(
            named(&other),
            Some(("window", project.clone())),
            "{}",
            other.display()
        );
    }
    // A folder in it, through the link, names it as the window does.
    assert_eq!(
        named(&link.join("web")),
        Some(("window", project.join("web")))
    );
    // `..` out of the project is another folder, though the path starts with the project's.
    std::fs::create_dir_all(dir.join("api")).unwrap();
    assert_eq!(named(&project.join("..").join("api")), None);
    // Neither a folder of no open project nor a relative name is taken for one.
    assert_eq!(named(&dir.join("other")), None);
    assert_eq!(named(Path::new("shop")), None);
    // A window that names its project through the link has it named so.
    let windows = vec![("window", vec![link.clone()])];
    assert_eq!(
        window_naming(&windows, &project.join("web")),
        Some(("window", link.join("web")))
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A folder opened through a link (the Open dialog keeps a link picked there as the link,
/// and Recent Projects reopens folders as they were remembered) is the project open at the
/// folder it leads to, which is how a window holds its projects: through the links, in the
/// letter case on disk (review-mcp M1). Compared as written, the folder was added to the
/// window a second time.
#[test]
fn a_folder_opened_through_a_link_is_the_project_open_there() {
    let dir = std::env::temp_dir().join(format!("sawit-cli-{}-root", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let project = dir.join("shop");
    std::fs::create_dir_all(&project).unwrap();
    let project = project.canonicalize().unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&project, &link).unwrap();
    std::fs::create_dir_all(dir.join("api")).unwrap();
    let windows = vec![
        ("first", vec![dir.join("api").canonicalize().unwrap()]),
        ("second", vec![project.clone()]),
    ];
    assert_eq!(window_with_root(&windows, &project), Some("second"));
    assert_eq!(window_with_root(&windows, &link), Some("second"));
    // In another letter case, on a volume that ignores case (a Mac's, as a rule).
    if dir.join("SHOP").exists() {
        assert_eq!(
            window_with_root(&windows, &dir.join("SHOP")),
            Some("second")
        );
    }
    // A folder inside an open project isn't that project.
    std::fs::create_dir_all(project.join("web")).unwrap();
    assert_eq!(window_with_root(&windows, &link.join("web")), None);
    assert_eq!(window_with_root(&windows, &dir.join("other")), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn bundle_detection() {
    assert_eq!(
        bundle_of(Path::new("/Applications/Sawit.app/Contents/MacOS/sawit")),
        Some(Path::new("/Applications/Sawit.app"))
    );
    assert_eq!(bundle_of(Path::new("/repo/target/release/sawit")), None);
}

/// An answer to `sawit-mcp` is written off the UI thread. A screenshot's answer is far more than
/// the socket holds, and written on the UI thread it held the UI until `sawit-mcp` had read it
/// all. Here `sawit-mcp` starts reading only once the answer has been handed over, and gets it
/// whole.
#[gpui_kit::test]
fn a_large_answer_is_written_off_the_ui_thread(cx: &mut TestAppContext) {
    use std::io::{BufRead, BufReader};
    use std::os::unix::net::UnixStream;
    use std::time::Duration;
    cx.update(|cx| cx.set_global(crate::rt::Rt(sawit_core::rt::Runtime::start())));
    let shot = "A".repeat(1 << 20);
    let (ui, mcp) = UnixStream::pair().unwrap();
    // Written on the UI thread, the answer would wait for a reader that hasn't started yet: it
    // gives up after a while instead of hanging, and the line read below comes out cut short.
    ui.set_write_timeout(Some(Duration::from_secs(10))).unwrap();
    let answer = serde_json::json!({ "png_base64": shot });
    cx.update(|cx| super::server::answer(ui, Ok(answer), cx));
    let mut line = String::new();
    BufReader::new(mcp).read_line(&mut line).unwrap();
    let json = line.strip_prefix(sawit_core::app_control::REPLY_OK);
    let value: serde_json::Value =
        serde_json::from_str(json.unwrap_or_default()).unwrap_or_default();
    assert_eq!(value["png_base64"].as_str().map(str::len), Some(1 << 20));
}
