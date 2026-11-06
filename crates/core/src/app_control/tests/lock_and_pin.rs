//! A terminal's lock and pin, as they go over the socket.

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
