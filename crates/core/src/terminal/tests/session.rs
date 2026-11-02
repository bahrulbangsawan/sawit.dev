//! Sessions running real programs in a PTY: what they draw, where they run, their selection,
//! and the palette their colours come from.

use std::time::{Duration, Instant};

use crate::terminal::{
    Command, GridSize, Palette, Rgb, SelectKind, Session, Snapshot, TermEvent, multiplexer_markers,
    run_bytes, wrap_unsetting,
};

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

fn wait_until(ok: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ok() {
        assert!(Instant::now() < deadline, "timed out waiting for condition");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn cwd_follows_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap();
    let cmd = Command {
        program: "/bin/sh".into(),
        cwd: Some(dir.clone()),
        ..Command::default()
    };
    let (s, _rx) = Session::spawn(
        cmd,
        GridSize {
            cols: 80,
            rows: 24,
            cell_width: 8,
            cell_height: 17,
        },
    )
    .unwrap();
    wait_until(|| s.cwd().as_deref() == Some(dir.as_path()));
    s.write(b"cd /\n".to_vec());
    wait_until(|| s.cwd().as_deref() == Some(std::path::Path::new("/")));
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
fn text_tail_keeps_scrollback_and_joins_wrapped_rows() {
    let size = GridSize {
        cols: 10,
        rows: 3,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "printf 'one\\ntwo\\nthree\\nfour\\n0123456789abcdef\\nend'; sleep 2".into(),
        ],
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    wait_for(&session, &Palette::default(), "end");
    assert_eq!(
        session.text_tail(100),
        "one\ntwo\nthree\nfour\n0123456789abcdef\nend"
    );
    assert_eq!(session.text_tail(2), "0123456789abcdef\nend");
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

/// The change count moves on with output, a resize, a scroll and the selection, and stays as
/// it is while none of them happens: a snapshot taken at a count is the screen until it moves.
#[test]
fn changes_count_what_a_snapshot_shows() {
    let size = GridSize {
        cols: 20,
        rows: 4,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "printf 'one\\ntwo\\nthree\\nfour\\nfive'; sleep 5".into(),
        ],
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    wait_for(&session, &Palette::default(), "five");
    let output = session.changes();
    assert!(output > 0, "the output wasn't counted");
    session.scroll_to_bottom();
    session.clear_selection();
    session.resize(size);
    assert_eq!(
        session.changes(),
        output,
        "the count moved on with nothing changed"
    );
    let mut last = output;
    let mut moves_on = |what: &str| {
        let now = session.changes();
        assert!(now > last, "{what} wasn't counted");
        last = now;
    };
    session.select_start(0, 0, false, SelectKind::Line);
    moves_on("a selection");
    session.clear_selection();
    moves_on("clearing the selection");
    session.scroll(1);
    moves_on("a scroll");
    session.scroll_to_bottom();
    moves_on("scrolling back down");
    session.resize(GridSize { cols: 30, ..size });
    moves_on("a resize");
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

#[cfg(unix)]
#[test]
fn shell_at_prompt_until_it_runs_a_command_then_cds() {
    let size = GridSize {
        cols: 300,
        rows: 6,
        cell_width: 8,
        cell_height: 16,
    };
    let mut env = std::collections::HashMap::new();
    env.insert("PS1".to_string(), "$ ".to_string());
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        env,
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    let palette = Palette::default();
    wait_for(&session, &palette, "$");
    assert!(session.at_prompt());

    session.write(run_bytes("sleep 2"));
    let deadline = Instant::now() + Duration::from_secs(2);
    while session.at_prompt() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !session.at_prompt(),
        "a running command is in the foreground"
    );

    let dir = tempfile::tempdir().unwrap();
    let dir = dir.path().canonicalize().unwrap().join("it's here");
    std::fs::create_dir(&dir).unwrap();
    session.write(b"\x03".to_vec());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.at_prompt() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    session.cd(&dir);
    session.write(run_bytes("pwd"));
    let snap = wait_for(&session, &palette, "it's here\n");
    assert!(
        snap.text().lines().any(|l| l == dir.to_string_lossy()),
        "{:?}",
        snap.text()
    );
    assert!(!snap.text().contains("cd --"), "{:?}", snap.text());
}

#[cfg(unix)]
#[test]
fn cd_echo_is_erased_even_when_it_wraps() {
    let size = GridSize {
        cols: 30,
        rows: 8,
        cell_width: 8,
        cell_height: 16,
    };
    let mut env = std::collections::HashMap::new();
    env.insert("PS1".to_string(), "prompt$ ".to_string());
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec!["-i".into()],
        env,
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    let palette = Palette::default();
    wait_for(&session, &palette, "prompt$");
    session.write(run_bytes("echo before"));
    wait_for(&session, &palette, "before\nprompt$");
    let dir = tempfile::tempdir().unwrap();
    session.cd(dir.path());
    session.write(run_bytes("echo after"));
    let snap = wait_for(&session, &palette, "after\nprompt$");
    let text = snap.text();
    assert!(
        !text.contains("cd --") && !text.contains("printf"),
        "{text:?}"
    );
    // The old prompt line is gone too: one prompt between the two echoes.
    assert!(
        text.contains("before\nprompt$ echo after\nafter"),
        "{text:?}"
    );
}

#[test]
fn selection_copies_cells_words_and_lines() {
    let size = GridSize {
        cols: 40,
        rows: 4,
        cell_width: 8,
        cell_height: 16,
    };
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            "printf 'hello big world\\nsecond'; sleep 2".into(),
        ],
        ..Command::default()
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    let palette = Palette::default();
    wait_for(&session, &palette, "second");
    assert_eq!(session.selection_text(), None);

    // Drag from the left of "big" to the right half of its "g".
    session.select_start(6, 0, false, SelectKind::Cell);
    session.select_extend(8, 0, true);
    assert_eq!(session.selection_text().as_deref(), Some("big"));
    let snap = session.snapshot(&palette);
    let selected: String = snap.lines[0]
        .iter()
        .filter(|c| c.selected)
        .map(|c| c.ch)
        .collect();
    assert_eq!(selected, "big");

    // A click without a drag selects nothing.
    session.select_start(6, 0, false, SelectKind::Cell);
    assert_eq!(session.selection_text(), None);

    session.select_start(12, 0, false, SelectKind::from_clicks(2));
    assert_eq!(session.selection_text().as_deref(), Some("world"));

    session.select_start(2, 1, false, SelectKind::from_clicks(3));
    assert_eq!(session.selection_text().as_deref(), Some("second\n"));

    session.clear_selection();
    assert_eq!(session.selection_text(), None);
}
