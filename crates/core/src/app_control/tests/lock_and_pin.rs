//! A terminal's close, lock and pin, as they go over the socket.

use super::*;

/// Lock and pin say what they set, or flip it.
#[test]
fn lock_and_pin_round_trip() {
    for command in [
        Command::TerminalLock {
            tab: 2,
            locked: Some(true),
        },
        Command::TerminalPin {
            tab: 0,
            pinned: None,
        },
    ] {
        let request = Request {
            project: None,
            agent_folder: None,
            agent: None,
            command,
        };
        assert_eq!(decode(&encode(&request)), Some(Ok(request)));
    }
    let flip = decode(
        br#"sawit-control/1
{"cmd":"terminal_lock","tab":1}"#,
    );
    assert!(matches!(
        flip,
        Some(Ok(Request {
            command: Command::TerminalLock {
                tab: 1,
                locked: None
            },
            ..
        }))
    ));
}

/// A close from a `sawit-mcp` that predates `confirm` reads as unconfirmed, so the app still
/// asks before ending what runs there.
#[test]
fn a_terminal_close_is_unconfirmed_unless_it_says_so() {
    let old = decode(
        br#"sawit-control/1
{"cmd":"terminal_close","tab":1}"#,
    );
    assert!(matches!(
        old,
        Some(Ok(Request {
            command: Command::TerminalClose {
                tab: 1,
                pane: None,
                confirm: false,
                scope: None
            },
            ..
        }))
    ));
    let request = Request {
        project: None,
        agent_folder: None,
        agent: None,
        command: Command::TerminalClose {
            tab: 1,
            pane: Some(0),
            confirm: true,
            scope: None,
        },
    };
    let bytes = encode(&request);
    assert_eq!(
        &bytes[MAGIC.len()..],
        br#"{"cmd":"terminal_close","tab":1,"pane":0,"confirm":true}"#
    );
    assert_eq!(decode(&bytes), Some(Ok(request)));
}
