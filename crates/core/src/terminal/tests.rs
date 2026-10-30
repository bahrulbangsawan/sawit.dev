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

#[test]
fn shell_langs_are_recognized() {
    for lang in [
        "bash", "sh", "zsh", "csh", "dash", "ksh", "tcsh", "shh", "Shell", " BASH ",
    ] {
        assert!(is_shell_lang(lang), "{lang}");
    }
    for lang in ["rust", "sql", "", "python"] {
        assert!(!is_shell_lang(lang), "{lang}");
    }
    assert_eq!(is_shell_lang("powershell"), cfg!(windows));
}

#[test]
fn shell_commands_strip_prompts_and_output() {
    assert_eq!(
        shell_commands("npm install\nnpm test\n"),
        "npm install\nnpm test"
    );
    assert_eq!(
        shell_commands("$ echo hi\nhi\n  $ ls -la\nfile\n"),
        "echo hi\nls -la"
    );
    assert_eq!(shell_commands("echo $HOME"), "echo $HOME");
}

#[test]
fn run_bytes_press_enter_after_each_line() {
    assert_eq!(run_bytes("ls"), b"ls\r");
    assert_eq!(run_bytes("cd x\nls"), b"cd x\rls\r");
    assert_eq!(run_bytes("a\r\nb"), b"a\rb\r");
    assert_eq!(run_bytes("a\x1b[2Jb"), b"a[2Jb\r");
}

#[test]
fn cd_bytes_quote_the_path() {
    let dir = std::path::Path::new("/tmp/it's here");
    assert_eq!(
        cd_bytes(dir, 0, 200),
        b"\x15 cd -- '/tmp/it'\\''s here' && printf '\\033[01A\\r\\033[J'\r".to_vec()
    );
    // 10 prompt columns + 54 command chars on a 40-column screen: two rows.
    assert!(
        String::from_utf8(cd_bytes(dir, 10, 40))
            .unwrap()
            .contains("[02A")
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
fn dropped_paths_are_quoted_and_spaced() {
    use std::path::PathBuf;
    let plain = Modes::default();
    let paths = [PathBuf::from("/a/b.txt"), PathBuf::from("/x/my file's.md")];
    assert_eq!(
        drop_bytes(&paths, plain),
        b"/a/b.txt '/x/my file'\\''s.md' ".to_vec()
    );
    let bracketed = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    assert_eq!(
        drop_bytes(&paths[..1], bracketed),
        b"\x1b[200~/a/b.txt \x1b[201~".to_vec()
    );
    assert_eq!(shell_quote(std::path::Path::new("")), "''");
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

#[test]
fn split_right_puts_the_new_pane_after_the_target() {
    let mut l = Layout::Pane(1);
    assert!(l.split(1, SplitDir::Right, 2));
    assert!(l.split(1, SplitDir::Down, 3));
    assert_eq!(l.panes(), vec![1, 3, 2]);
    assert!(!l.split(9, SplitDir::Right, 4));
}

#[test]
fn rects_divide_the_area_by_ratio() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let r = l.rects(Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    });
    assert_eq!(
        r[0],
        (
            1,
            Rect {
                x: 0.,
                y: 0.,
                w: 400.,
                h: 400.
            }
        )
    );
    assert_eq!(
        r[1],
        (
            2,
            Rect {
                x: 400.,
                y: 0.,
                w: 400.,
                h: 200.
            }
        )
    );
    assert_eq!(
        r[2],
        (
            3,
            Rect {
                x: 400.,
                y: 200.,
                w: 400.,
                h: 200.
            }
        )
    );
}

#[test]
fn rects_never_go_negative_in_a_tiny_area() {
    let mut l = Layout::Pane(1);
    for n in 2..8 {
        l.split(n - 1, SplitDir::Right, n);
    }
    for (_, r) in l.rects(Rect {
        x: 0.,
        y: 0.,
        w: 3.,
        h: 0.,
    }) {
        assert!(r.w >= 0. && r.h >= 0. && !r.w.is_nan() && !r.h.is_nan());
    }
}

#[test]
fn removing_a_pane_gives_its_space_to_the_sibling() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    assert_eq!(l.remove(1), Removed::Focus(2));
    assert_eq!(l.panes(), vec![2, 3]);
    assert_eq!(l.remove(3), Removed::Focus(2));
    assert_eq!(l, Layout::Pane(2));
    assert_eq!(l.remove(2), Removed::Last);
    assert_eq!(l.remove(7), Removed::NotFound);
}

#[test]
fn neighbor_follows_the_direction() {
    // 1 | 2
    //   | 3
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    l.split(2, SplitDir::Down, 3);
    let a = Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    };
    assert_eq!(l.neighbor(1, Direction::Right, a), Some(2));
    assert_eq!(l.neighbor(3, Direction::Left, a), Some(1));
    assert_eq!(l.neighbor(2, Direction::Down, a), Some(3));
    assert_eq!(l.neighbor(3, Direction::Up, a), Some(2));
    assert_eq!(l.neighbor(1, Direction::Left, a), None);
}

#[test]
fn set_ratio_keeps_both_sides_at_least_min() {
    let mut l = Layout::Pane(1);
    l.split(1, SplitDir::Right, 2);
    let root = SplitPath(vec![]);
    l.set_ratio(&root, 0.05, 800., 160.);
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.2).abs() < 1e-6));
    l.set_ratio(&root, 0.99, 800., 160.);
    assert!(matches!(l, Layout::Split { ratio, .. } if (ratio - 0.8).abs() < 1e-6));
    l.even(&root);
    assert!(matches!(l, Layout::Split { ratio, .. } if ratio == 0.5));
    assert_eq!(
        l.dividers(Rect {
            x: 0.,
            y: 0.,
            w: 800.,
            h: 400.
        })[0]
            .at,
        400.
    );
}

#[test]
fn pane_ids_are_unique() {
    let ids: std::collections::HashSet<_> = (0..1000).map(|_| new_pane_id()).collect();
    assert_eq!(ids.len(), 1000);
}

fn tab_area() -> Rect {
    Rect {
        x: 0.,
        y: 0.,
        w: 800.,
        h: 400.,
    }
}
const TAB_MIN: (f32, f32) = (160., 80.);

#[test]
fn split_focuses_the_new_pane_and_unzooms() {
    let mut t = TabState::new(1);
    assert!(t.split(SplitDir::Right, 2, tab_area(), TAB_MIN));
    assert_eq!(t.focused, 2);
    assert!(t.toggle_zoom());
    assert!(t.split(SplitDir::Down, 3, tab_area(), TAB_MIN));
    assert_eq!(t.zoomed, None);
    assert_eq!(t.panes.len(), 3);
}

#[test]
fn split_is_refused_when_the_pane_is_too_small() {
    let mut t = TabState::new(1);
    let small = Rect {
        x: 0.,
        y: 0.,
        w: 300.,
        h: 400.,
    };
    assert!(!t.split(SplitDir::Right, 2, small, TAB_MIN));
    assert_eq!(t.layout, Layout::Pane(1));
    assert_eq!(t.panes.len(), 1);
}

#[test]
fn closing_the_zoomed_pane_unzooms_and_focuses_the_sibling() {
    let mut t = TabState::new(1);
    t.split(SplitDir::Right, 2, tab_area(), TAB_MIN);
    t.toggle_zoom();
    assert_eq!(t.close(2), Removed::Focus(1));
    assert_eq!((t.focused, t.zoomed), (1, None));
    assert_eq!(t.close(1), Removed::Last);
}

#[test]
fn zoom_needs_two_panes_and_shows_only_the_zoomed_one() {
    let mut t = TabState::new(1);
    assert!(!t.toggle_zoom());
    t.split(SplitDir::Right, 2, tab_area(), TAB_MIN);
    t.toggle_zoom();
    assert_eq!(t.visible(tab_area()), vec![(2, tab_area())]);
    t.focus(1);
    assert_eq!(t.zoomed, None);
}

#[test]
fn rename_trims_and_clears() {
    let mut t = TabState::new(1);
    t.rename(1, "  api  ");
    assert_eq!(t.panes[&1].name.as_deref(), Some("api"));
    t.rename(1, "   ");
    assert_eq!(t.panes[&1].name, None);
    t.toggle_right_clicks(1);
    assert!(t.panes[&1].send_right_clicks);
}
