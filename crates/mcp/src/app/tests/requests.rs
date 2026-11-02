//! What the app is sent and what its answers become: names of panels, pages and actions, the
//! project and the agent's folder, `launch_app`'s path, and images sent as base64.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::handler::server::wrapper::Parameters;
use sawit_core::app_control::{InstanceAction, Page, Panel};
use serde_json::json;

use super::{fake_app_answering, native};
use crate::app::launch::{launch_args, launch_target, launched};
use crate::app::{
    INSTANCE_ACTIONS, PAGES, PANELS, TerminalReadArgs, base64_decode, project_path, variant,
};

#[test]
fn panel_and_page_names() {
    assert_eq!(
        variant::<Panel>("Shell_Config", PANELS),
        Ok(Panel::ShellConfig)
    );
    assert_eq!(variant::<Page>("git_graph", PAGES), Ok(Page::GitGraph));
    assert_eq!(variant::<Page>("keymap", PAGES), Ok(Page::Keymap));
    assert_eq!(variant::<Page>("Skills", PAGES), Ok(Page::Skills));
    assert_eq!(variant::<Page>("new_project", PAGES), Ok(Page::NewProject));
    assert_eq!(
        variant::<InstanceAction>("Restart", INSTANCE_ACTIONS),
        Ok(InstanceAction::Restart)
    );
    assert!(
        variant::<InstanceAction>("burn", INSTANCE_ACTIONS)
            .unwrap_err()
            .contains("restart")
    );
    assert!(
        variant::<Panel>("nope", PANELS)
            .unwrap_err()
            .contains("ports")
    );
}

/// MCP-3: the project an agent names goes to the app as the folder it is on disk: trimmed,
/// `~` expanded, a relative name taken from the agent's folder, and through the links on its
/// way, in the case the folder has and without `..`, as the app names the projects it opens.
/// It went as written, so `~/…`, `/tmp/…` or another case matched no open project, though the
/// same names work with the app closed.
#[test]
fn a_project_is_sent_as_the_folder_it_names() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("shop");
    std::fs::create_dir_all(real.join("web")).unwrap();
    let real = real.canonicalize().unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    for named in [
        format!(" {} ", link.display()),
        dir.path().join("SHOP").display().to_string(),
        real.join("web").join("..").display().to_string(),
    ] {
        assert_eq!(
            project_path(Some(named.clone())),
            Some(real.clone()),
            "{named}"
        );
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home).canonicalize().unwrap();
        assert_eq!(project_path(Some("~".into())), Some(home.clone()));
        assert_eq!(project_path(Some("~/".into())), Some(home));
    }
    let here = std::env::current_dir().unwrap().canonicalize().unwrap();
    assert_eq!(project_path(Some("src".into())), Some(here.join("src")));
    assert_eq!(project_path(Some("  ".into())), None);
    assert_eq!(project_path(None), None);
}

/// MCP-5: `launch_app` opens the folder or file its `path` names, and says what it opened.
/// The `sawit` CLI took the path as written: `~/proj` opened a new file under
/// `<the agent's folder>/~/proj` while the reply said "~/proj" was open, and `--version`
/// was read as an option. `~` is the home folder now, the path must be absolute (as for
/// `app_open`), and it comes after `--`, so nothing in it is an option or a subcommand.
#[test]
fn launch_app_opens_the_path_it_names_and_says_what_it_opened() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("shop");
    std::fs::create_dir_all(&real).unwrap();
    let real = real.canonicalize().unwrap();
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let target = launch_target(Some(&format!(" {} ", link.display()))).unwrap();
    assert_eq!(target.as_deref(), Some(real.as_path()));
    assert_eq!(
        launch_args(target.as_deref()),
        vec![std::ffi::OsString::from("--"), real.clone().into()]
    );
    assert_eq!(
        launched(target.as_deref()),
        format!("Sawit is running with {} open.", real.display())
    );
    if let Some(home) = std::env::var_os("HOME") {
        let home = Path::new(&home).canonicalize().unwrap();
        let target = launch_target(Some("~/no such folder")).unwrap();
        assert_eq!(target, Some(home.join("no such folder")));
        // Nothing is there: the app opens it as a new file, and the answer says so.
        let said = launched(target.as_deref());
        assert!(said.contains("new file"), "{said}");
    }
    for not_absolute in ["--version", "list", "proj", "~user/x"] {
        let refused = launch_target(Some(not_absolute)).unwrap_err();
        assert!(refused.contains("absolute"), "{not_absolute}: {refused}");
    }
    assert_eq!(launch_target(Some("  ")), Ok(None));
    assert_eq!(launch_target(None), Ok(None));
    assert!(launch_args(None).is_empty());
}

#[test]
fn decodes_base64() {
    assert_eq!(base64_decode("aGVsbG8=", "The canvas").unwrap(), b"hello");
    assert_eq!(
        base64_decode("iVBO\nRw==", "The canvas").unwrap(),
        [0x89, b'P', b'N', b'G']
    );
    assert_eq!(
        base64_decode("@@", "The browser"),
        Err("The browser sent invalid base64.".into())
    );
}

/// Without `project`, a terminal or app tool names the folder the agent works in, so the app
/// uses the window that has it open rather than whichever window is in front; with one, it
/// names only the project.
#[tokio::test]
async fn app_tools_name_the_agents_folder() {
    let dir = tempfile::tempdir().unwrap();
    let server = native(dir.path());
    let asked = fake_app_answering(
        &server.paths,
        vec![json!({"text": ""}), json!({"text": ""})],
    );
    let read = |project: Option<String>| TerminalReadArgs {
        tab: None,
        pane: None,
        lines: None,
        project,
    };
    server.terminal_read(Parameters(read(None))).await;
    let request = asked
        .recv_timeout(Duration::from_secs(5))
        .expect("the app was asked");
    assert_eq!(request.project, None);
    assert_eq!(request.agent_folder, crate::project_dir(None));
    assert!(request.agent_folder.is_some(), "the tests run in a folder");
    // Every op says which agent session sent it: this process's.
    let agent = request.agent.expect("the agent goes with the op");
    assert!(
        agent
            .id
            .starts_with(&format!("mcp-{}-", std::process::id()))
    );

    server
        .terminal_read(Parameters(read(Some("/code/shop".into()))))
        .await;
    let request = asked
        .recv_timeout(Duration::from_secs(5))
        .expect("the app was asked");
    assert_eq!(request.project, Some(PathBuf::from("/code/shop")));
    assert_eq!(request.agent_folder, None);
}
