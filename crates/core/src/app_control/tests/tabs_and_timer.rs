//! The Pomodoro timer's action, a center tab's menu action, the Accounts list's reload and a
//! terminal read of the last command's output, as they go over the socket.

use super::*;

fn round_trips(command: Command, tag: &str) {
    let request = Request {
        project: None,
        agent_folder: None,
        agent: None,
        command,
    };
    let bytes = encode(&request);
    assert!(
        String::from_utf8_lossy(&bytes).contains(tag),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(decode(&bytes), Some(Ok(request)));
}

#[test]
fn the_new_commands_round_trip() {
    round_trips(
        Command::Pomodoro {
            action: "start".into(),
        },
        r#""cmd":"pomodoro""#,
    );
    round_trips(
        Command::TabAction {
            tab: "2".into(),
            action: "pin".into(),
        },
        r#""cmd":"tab_action""#,
    );
    round_trips(
        Command::Reload {
            store: Store::Accounts,
        },
        r#""store":"accounts""#,
    );
    round_trips(
        Command::TerminalRead {
            tab: Some(1),
            pane: None,
            lines: None,
            run: None,
            output: Some("errors".into()),
        },
        r#""output":"errors""#,
    );
}
