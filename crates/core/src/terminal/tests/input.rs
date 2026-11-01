//! Keys, mouse reports, pastes, drops and typed commands as the bytes a program reads.

use crate::terminal::{
    KeyInput, Modes, MouseAction, MouseButton, ScrollAccumulator, cd_bytes, drop_bytes,
    is_shell_lang, key_to_bytes, mouse_report, run_bytes, shell_commands, shell_quote,
};

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
