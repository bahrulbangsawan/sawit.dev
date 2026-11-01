//! `sawit list keys` and `sawit browse`, against a stand-in for the running app.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::app_control::{self, Command, Request};
use crate::browser::BrowserOp;
use crate::cli::BrowseTarget;
use crate::instances::AppPaths;
use crate::kebun::browse::{dev_server_folders, dev_server_in, own_folder};
use crate::kebun::ink::Ink;
use crate::kebun::keys::{key_line, list_keys};
use crate::ports::PortJob;

use super::*;

/// A Sawit home of the test's own, whose socket is in the test's folder: no app listens
/// there but the one a test stands in.
fn scratch_home(dir: &Path) -> AppPaths {
    let paths = AppPaths {
        root: dir.join("home"),
    };
    assert!(
        paths.cli_socket().starts_with(dir),
        "the socket left the test's folder"
    );
    paths
}

/// The defaults are bound in the app's code, so without the app `sawit list keys` fails and
/// says so, rather than start it.
#[test]
fn list_keys_without_the_app_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let paths = scratch_home(dir.path());
    assert_eq!(list_keys(&Ink::new(false), &paths), 1);
}

/// With the app running, `sawit list keys` asks it for the keymap editor's rows, as
/// `keymap_list` does.
#[test]
fn list_keys_asks_the_running_app() {
    use std::io::Read as _;
    let dir = tempfile::tempdir().unwrap();
    let paths = scratch_home(dir.path());
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
    let list = crate::keymap::KeymapList {
        path: paths.keymap_file(),
        writable: true,
        errors: vec!["\"cmd-e\": no such action".into()],
        rows: vec![crate::keymap::KeymapRow {
            action: "workspace::CloseAllTabs".into(),
            label: "workspace: close all tabs".into(),
            args: None,
            keys: "cmd-k w".into(),
            display: "⌘K W".into(),
            context: Some("Workspace".into()),
            source: crate::keymap::KeySource::Default,
            conflict: false,
        }],
    };
    let answer = crate::app_control::reply_ok(&serde_json::to_value(&list).unwrap());
    let (asked, requests) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            // `is_running` only connects, and sends nothing.
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            let Some(Ok(request)) = crate::app_control::decode(&bytes) else {
                break;
            };
            let _ = asked.send(request.command);
            let _ = writeln!(stream, "{answer}");
            break;
        }
    });
    assert_eq!(list_keys(&Ink::new(false), &paths), 0);
    let asked = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("the app was asked");
    assert_eq!(
        asked,
        crate::app_control::Command::Keymap {
            op: crate::keymap::KeymapOp::List {
                filter: None,
                keys: None,
                unbound: false,
            },
        }
    );
}

/// A binding's line: the keys as a Mac menu writes them, in a column, then what they run,
/// then the keys as keymap.json writes them, where they work and whose binding it is.
#[test]
fn a_key_line_says_what_runs_where_and_whose_it_is() {
    let ink = Ink::new(false);
    let row = crate::keymap::KeymapRow {
        action: "workspace::CloseAllTabs".into(),
        label: "workspace: close all tabs".into(),
        args: None,
        keys: "cmd-k w".into(),
        display: "⌘K W".into(),
        context: Some("Workspace".into()),
        source: crate::keymap::KeySource::User,
        conflict: true,
    };
    let line = key_line(&ink, &row);
    assert!(line.starts_with("  ⌘K W "), "{line}");
    let label: String = line.chars().skip(15).collect();
    assert!(label.starts_with("workspace: close all tabs "), "{line}");
    assert!(
        line.ends_with("cmd-k w · Workspace  milikmu bentrok"),
        "{line}"
    );
    let anywhere = crate::keymap::KeymapRow {
        context: None,
        source: crate::keymap::KeySource::Removed,
        conflict: false,
        ..row
    };
    let line = key_line(&ink, &anywhere);
    assert!(line.ends_with("cmd-k w · di mana saja  dicabut"), "{line}");
}

/// A stand-in for the running app, listening where `paths` says the app does: it keeps each
/// request, and answers `state` for `state` and `{}` for the others.
fn fake_app(
    paths: &AppPaths,
    state: serde_json::Value,
) -> std::sync::Arc<std::sync::Mutex<Vec<Request>>> {
    use std::io::Read as _;
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    // In the test's own folder, never the shared one a long path falls back to.
    assert!(paths.cli_socket().starts_with(&paths.root));
    std::fs::create_dir_all(&paths.root).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(paths.cli_socket()).unwrap();
    let kept = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut bytes = Vec::new();
            // `is_running` only connects.
            if stream.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
                continue;
            }
            let Some(Ok(request)) = app_control::decode(&bytes) else {
                continue;
            };
            let answer = match request.command {
                Command::State => state.clone(),
                _ => serde_json::json!({}),
            };
            kept.lock().unwrap().push(request);
            let _ = writeln!(stream, "{}", app_control::reply_ok(&answer));
        }
    });
    seen
}

/// `sawit browse` sends what browser_navigate sends for an agent working in its folder, so
/// the page opens in the browser of the project that holds it, then brings Sawit forward. A
/// port is localhost's; an address is read as the address field reads it. Sawit is started
/// first when it isn't running, and when it won't start nothing is sent.
#[test]
fn browse_opens_the_page_in_the_folders_browser() {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        root: dir.path().join("home"),
    };
    let cwd = dir.path().join("shop");
    let navigate = |url: &str| Command::Browser {
        op: BrowserOp::Navigate { url: url.into() },
    };
    let sent = |seen: &std::sync::Mutex<Vec<Request>>| -> Vec<(Option<PathBuf>, Command)> {
        let seen = seen.lock().unwrap();
        seen.iter()
            .map(|r| (r.agent_folder.clone(), r.command.clone()))
            .collect()
    };
    let page = |a: &str| BrowseTarget::Address(a.into());

    assert_eq!(
        browse(
            page("localhost:5173"),
            &paths,
            &cwd,
            || Err("no app".into())
        ),
        1
    );
    let mut started = None;
    let code = browse(page("localhost:5173"), &paths, &cwd, || {
        started = Some(fake_app(&paths, serde_json::json!({})));
        Ok(())
    });
    assert_eq!(code, 0);
    let seen = started.expect("Sawit was started");
    assert_eq!(
        sent(&seen),
        [
            (Some(cwd.clone()), navigate("http://localhost:5173")),
            (Some(cwd.clone()), Command::Focus),
        ]
    );
    seen.lock().unwrap().clear();
    let code = browse(page("https://example.com/a"), &paths, &cwd, || {
        panic!("started while it runs")
    });
    assert_eq!(code, 0);
    assert_eq!(sent(&seen)[0].1, navigate("https://example.com/a"));
}

/// With no address, `sawit browse` looks for the dev server in its folder, then in the open
/// project that holds it: the nearest one wins, two in one place are named for the user to
/// pick, and none says how to start one or give an address. In `/` or the home folder it
/// has no folder of its own, as an agent hasn't: the front window's project is used.
#[test]
fn browse_finds_the_nearest_dev_server() {
    let job = |cwd: &str, port: u16| PortJob {
        root: 1,
        pids: vec![1],
        label: "bun run dev".into(),
        tool: "bun".into(),
        cmdline: "bun run dev".into(),
        listeners: vec![(2, "bun".into())],
        ports: vec![(port, crate::ports::BindAddr::LoopbackV4)],
        cwd: Some(cwd.into()),
        dev: true,
    };
    let jobs = [job("/code/shop/web", 5173), job("/code/shop/api", 3000)];
    let folders = |list: &[&str]| list.iter().map(PathBuf::from).collect::<Vec<_>>();
    assert_eq!(
        dev_server_in(&jobs, &folders(&["/code/shop/web", "/code/shop"])),
        Ok("http://localhost:5173".into())
    );
    let two = dev_server_in(&jobs, &folders(&["/code/shop/docs", "/code/shop"])).unwrap_err();
    assert!(
        two.starts_with("Ada 2 dev server di /code/shop: :5173 (bun run dev), :3000")
            && two.ends_with("`sawit browse 5173`."),
        "{two}"
    );
    let none = dev_server_in(&jobs, &folders(&["/code/blog"])).unwrap_err();
    assert!(
        none.starts_with("Tidak ada dev server yang menyala di /code/blog."),
        "{none}"
    );

    let state = serde_json::json!({"windows": [
        {"project": "/code/shop", "projects": ["/code/shop"], "frontmost": true},
    ]});
    assert_eq!(
        dev_server_folders(&state, Some(Path::new("/code/shop/docs"))),
        Ok(folders(&["/code/shop/docs", "/code/shop"]))
    );
    assert_eq!(
        dev_server_folders(&state, Some(Path::new("/code/shop"))),
        Ok(folders(&["/code/shop"]))
    );
    assert_eq!(
        dev_server_folders(&state, None),
        Ok(folders(&["/code/shop"]))
    );
    assert!(dev_server_folders(&serde_json::json!({"windows": []}), None).is_err());
    assert_eq!(own_folder(Path::new("/")), None);
    if let Some(home) = std::env::var_os("HOME") {
        assert_eq!(own_folder(Path::new(&home)), None);
    }
    assert_eq!(
        own_folder(Path::new("/code/shop")),
        Some(PathBuf::from("/code/shop"))
    );
    // Its words are never databases.
    assert_eq!(q(&["browse"]), None);
    assert_eq!(q(&["intip", "5173"]), None);
}
