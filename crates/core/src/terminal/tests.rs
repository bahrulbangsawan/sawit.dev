use super::*;
use std::time::{Duration, Instant};

fn key(name: &str) -> KeyInput {
    KeyInput {
        key: name.into(),
        ..KeyInput::default()
    }
}

fn ctrl(name: &str) -> KeyInput {
    KeyInput {
        key: name.into(),
        ctrl: true,
        ..KeyInput::default()
    }
}

#[test]
fn plain_text_uses_key_char() {
    let k = KeyInput {
        key: "a".into(),
        text: Some("a".into()),
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&k, Modes::default()), Some(b"a".to_vec()));
    let k = KeyInput {
        key: "a".into(),
        shift: true,
        text: Some("A".into()),
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&k, Modes::default()), Some(b"A".to_vec()));
}

#[test]
fn control_keys() {
    let m = Modes::default();
    assert_eq!(key_to_bytes(&ctrl("c"), m), Some(vec![0x03]));
    assert_eq!(key_to_bytes(&ctrl("a"), m), Some(vec![0x01]));
    assert_eq!(key_to_bytes(&ctrl("["), m), Some(vec![0x1b]));
    assert_eq!(key_to_bytes(&ctrl("space"), m), Some(vec![0x00]));
    assert_eq!(key_to_bytes(&key("enter"), m), Some(b"\r".to_vec()));
    assert_eq!(key_to_bytes(&key("backspace"), m), Some(vec![0x7f]));
    assert_eq!(key_to_bytes(&key("tab"), m), Some(b"\t".to_vec()));
    assert_eq!(key_to_bytes(&key("escape"), m), Some(b"\x1b".to_vec()));
    let shift_tab = KeyInput {
        key: "tab".into(),
        shift: true,
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&shift_tab, m), Some(b"\x1b[Z".to_vec()));
}

#[test]
fn arrows_follow_application_cursor_mode() {
    assert_eq!(
        key_to_bytes(&key("up"), Modes::default()),
        Some(b"\x1b[A".to_vec())
    );
    let app = Modes {
        app_cursor: true,
        ..Modes::default()
    };
    assert_eq!(key_to_bytes(&key("up"), app), Some(b"\x1bOA".to_vec()));
    assert_eq!(
        key_to_bytes(&ctrl("right"), app),
        Some(b"\x1b[1;5C".to_vec())
    );
}

#[test]
fn alt_prefixes_escape() {
    let k = KeyInput {
        key: "b".into(),
        alt: true,
        text: Some("b".into()),
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&k, Modes::default()), Some(b"\x1bb".to_vec()));
}

#[test]
fn navigation_and_function_keys() {
    let m = Modes::default();
    assert_eq!(key_to_bytes(&key("home"), m), Some(b"\x1b[H".to_vec()));
    assert_eq!(key_to_bytes(&key("end"), m), Some(b"\x1b[F".to_vec()));
    assert_eq!(key_to_bytes(&key("pageup"), m), Some(b"\x1b[5~".to_vec()));
    assert_eq!(key_to_bytes(&key("delete"), m), Some(b"\x1b[3~".to_vec()));
    assert_eq!(key_to_bytes(&key("f1"), m), Some(b"\x1bOP".to_vec()));
    assert_eq!(key_to_bytes(&key("f5"), m), Some(b"\x1b[15~".to_vec()));
    assert_eq!(key_to_bytes(&key("f12"), m), Some(b"\x1b[24~".to_vec()));
}

#[test]
fn cmd_shortcuts_are_not_sent() {
    let k = KeyInput {
        key: "c".into(),
        cmd: true,
        text: Some("c".into()),
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&k, Modes::default()), None);
}

#[test]
fn sgr_mouse_reports() {
    let m = Modes {
        mouse_report: true,
        sgr_mouse: true,
        ..Modes::default()
    };
    assert_eq!(
        mouse_report(MouseAction::Press(MouseButton::Left), 4, 2, false, m),
        Some(b"\x1b[<0;5;3M".to_vec())
    );
    assert_eq!(
        mouse_report(MouseAction::Release(MouseButton::Left), 4, 2, false, m),
        Some(b"\x1b[<0;5;3m".to_vec())
    );
    assert_eq!(
        mouse_report(MouseAction::WheelUp, 0, 0, false, m),
        Some(b"\x1b[<64;1;1M".to_vec())
    );
    assert_eq!(
        mouse_report(
            MouseAction::Press(MouseButton::Left),
            0,
            0,
            false,
            Modes::default()
        ),
        None
    );
}

#[test]
fn mouse_motion_only_when_requested() {
    let click_only = Modes {
        mouse_report: true,
        sgr_mouse: true,
        ..Modes::default()
    };
    assert_eq!(
        mouse_report(
            MouseAction::Drag(MouseButton::Left),
            1,
            1,
            false,
            click_only
        ),
        None
    );
    let drag = Modes {
        mouse_drag: true,
        ..click_only
    };
    assert_eq!(
        mouse_report(MouseAction::Drag(MouseButton::Left), 1, 1, false, drag),
        Some(b"\x1b[<32;2;2M".to_vec())
    );
}

#[test]
fn palette_resolves_named_indexed_and_truecolor() {
    let p = Palette::default();
    assert_eq!(p.indexed(1), p.ansi[1]);
    assert_eq!(p.indexed(16), Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(p.indexed(196), Rgb { r: 255, g: 0, b: 0 });
    assert_eq!(p.indexed(232), Rgb { r: 8, g: 8, b: 8 });
    assert_eq!(
        p.indexed(255),
        Rgb {
            r: 238,
            g: 238,
            b: 238
        }
    );
}

fn wait_for(session: &Session, palette: &Palette, needle: &str) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let snap = session.snapshot(palette);
        if snap.text().contains(needle) || Instant::now() > deadline {
            return snap;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn session_renders_program_output() {
    let size = GridSize {
        cols: 40,
        rows: 5,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec!["-c".into(), "printf 'hello\\033[31m red'; sleep 2".into()],
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    let palette = Palette::default();
    let snap = wait_for(&session, &palette, "hello red");
    assert!(snap.text().contains("hello red"), "{:?}", snap.text());
    assert_eq!(snap.cols, 40);
    assert_eq!(snap.lines.len(), 5);
    let red = &snap.lines[0][6];
    assert_eq!(red.ch, 'r');
    assert_eq!(red.fg, palette.ansi[1]);
}

#[test]
fn session_echoes_input_and_resizes() {
    let size = GridSize {
        cols: 20,
        rows: 4,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/bin/cat".into(),
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    session.write(b"abc\r".to_vec());
    let palette = Palette::default();
    let snap = wait_for(&session, &palette, "abc");
    assert!(snap.text().contains("abc"), "{:?}", snap.text());
    assert_eq!(session.modes(), snap.modes);
    session.resize(GridSize {
        cols: 30,
        rows: 6,
        cell_width: 8,
        cell_height: 16,
    });
    let snap = session.snapshot(&palette);
    assert_eq!((snap.cols, snap.lines.len()), (30, 6));
}

#[test]
fn session_reports_exit() {
    let size = GridSize {
        cols: 20,
        rows: 4,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/usr/bin/true".into(),
        ..Command::default()
    };
    let (_session, mut events) = Session::spawn(cmd, size).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match events.try_recv() {
            Ok(TermEvent::Exit) => break,
            Ok(_) => {}
            Err(_) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => panic!("no exit event"),
        }
    }
}

#[test]
fn parent_multiplexer_markers_are_detected() {
    let vars = [
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "3"),
        ("TMUX", "/tmp/x"),
        ("ZELLIJ_SESSION_NAME", "a"),
        ("PATH", "/bin"),
        ("TERM_PROGRAM", "herdr"),
    ];
    let names = multiplexer_markers(vars.iter().map(|(k, _)| k.to_string()));
    assert_eq!(
        names,
        vec!["HERDR_ENV", "HERDR_PANE_ID", "TMUX", "ZELLIJ_SESSION_NAME"]
    );
}

#[test]
fn unset_wrapper_uses_env_dash_u() {
    let (program, args) = wrap_unsetting("herdr", &[], &["HERDR_ENV".into(), "TMUX".into()]);
    assert_eq!(program, "/usr/bin/env");
    assert_eq!(args, vec!["-u", "HERDR_ENV", "-u", "TMUX", "herdr"]);
    let (program, args) = wrap_unsetting("/bin/zsh", &["-l".into()], &[]);
    assert_eq!(
        (program.as_str(), args),
        ("/bin/zsh", vec!["-l".to_string()])
    );
}

#[test]
fn option_composed_characters_are_sent_as_typed() {
    // German layout: Option-L types '@'; Option-a composes 'å'.
    let at = KeyInput {
        key: "l".into(),
        alt: true,
        text: Some("@".into()),
        ..KeyInput::default()
    };
    assert_eq!(key_to_bytes(&at, Modes::default()), Some(b"@".to_vec()));
    let a = KeyInput {
        key: "a".into(),
        alt: true,
        text: Some("\u{e5}".into()),
        ..KeyInput::default()
    };
    assert_eq!(
        key_to_bytes(&a, Modes::default()),
        Some("\u{e5}".as_bytes().to_vec())
    );
}

#[test]
fn scroll_accumulator_keeps_fractions() {
    let mut acc = ScrollAccumulator::default();
    let lines: Vec<i32> = (0..10).map(|_| acc.add(5.0, 17.0)).collect();
    assert_eq!(lines.iter().sum::<i32>(), 2); // 50px / 17px
    assert_eq!(acc.add(-40.0, 17.0), -1);
}
