//! A terminal's rename, and a create that leaves the project unopened, as they go over the socket.

use super::*;

fn request(command: Command) -> Request {
    Request {
        project: None,
        agent_folder: None,
        agent: None,
        command,
    }
}

/// A rename names the tab, or with a pane that pane, and its name goes as given: a blank one
/// (which clears the name) too.
#[test]
fn a_terminal_rename_round_trips() {
    let tab = request(Command::TerminalRename {
        tab: 2,
        pane: None,
        name: "dev server".into(),
    });
    let bytes = encode(&tab);
    assert_eq!(
        &bytes[MAGIC.len()..],
        br#"{"cmd":"terminal_rename","tab":2,"pane":null,"name":"dev server"}"#
    );
    assert_eq!(decode(&bytes), Some(Ok(tab)));
    let pane = request(Command::TerminalRename {
        tab: 0,
        pane: Some(1),
        name: String::new(),
    });
    let bytes = encode(&pane);
    assert_eq!(decode(&bytes), Some(Ok(pane)));
    // Without a pane it names the tab.
    let short = decode(
        br#"sawit-control/1
{"cmd":"terminal_rename","tab":3,"name":"tests"}"#,
    );
    assert!(matches!(
        short,
        Some(Ok(Request {
            command: Command::TerminalRename { tab: 3, pane: None, ref name },
            ..
        })) if name == "tests"
    ));
    // A rename needs the name: without one it doesn't read.
    assert!(matches!(
        decode(
            br#"sawit-control/1
{"cmd":"terminal_rename","tab":3}"#
        ),
        Some(Err(_))
    ));
}

/// Create Project Only goes as `open: false`; an op without `open`, as an older `sawit-mcp`
/// sends it, opens the project as Create Project does.
#[test]
fn a_create_says_whether_it_opens_the_project() {
    let only = request(Command::NewProject {
        location: "/Users/me/Developer".into(),
        stack: crate::stack_builder::Stack::default(),
        open: false,
    });
    let bytes = encode(&only);
    let wire = String::from_utf8_lossy(&bytes[MAGIC.len()..]).into_owned();
    assert!(wire.ends_with(r#","open":false}"#), "{wire}");
    assert_eq!(decode(&bytes), Some(Ok(only)));
    let older = decode(
        br#"sawit-control/1
{"cmd":"new_project","location":"/tmp","stack":{}}"#,
    );
    assert!(
        matches!(
            older,
            Some(Ok(Request {
                command: Command::NewProject { open: true, .. },
                ..
            }))
        ),
        "{older:?}"
    );
}
