//! Keyboard and mouse input encoded as terminal escape sequences (xterm).

/// A key press as the UI reports it. `key` uses GPUI-style names ("a",
/// "enter", "up", "f5"); `text` is the character the key produces, if any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyInput {
    pub key: String,
    pub text: Option<String>,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub cmd: bool,
}

/// Terminal modes that change how input is encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor: bool,
    pub bracketed_paste: bool,
    pub mouse_report: bool,
    pub mouse_drag: bool,
    pub mouse_motion: bool,
    pub sgr_mouse: bool,
    pub alt_screen: bool,
    pub alternate_scroll: bool,
}

/// xterm modifier parameter: 1 + shift + 2*alt + 4*ctrl.
fn modifier_param(k: &KeyInput) -> u8 {
    1 + k.shift as u8 + 2 * k.alt as u8 + 4 * k.ctrl as u8
}

/// Bytes to send to the PTY for `k`, or `None` if the terminal should not
/// receive it (e.g. Cmd shortcuts, which belong to the app).
pub fn key_to_bytes(k: &KeyInput, modes: Modes) -> Option<Vec<u8>> {
    if k.cmd {
        return None;
    }
    let m = modifier_param(k);
    let modified = m > 1;
    let csi = |final_byte: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{m}{final_byte}").into_bytes()
        } else if modes.app_cursor && "ABCDHF".contains(final_byte) {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{n};{m}~")
        } else {
            format!("\x1b[{n}~")
        }
        .into_bytes()
    };
    let ss3 = |final_byte: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{m}{final_byte}")
        } else {
            format!("\x1bO{final_byte}")
        }
        .into_bytes()
    };
    let alt_prefix = |mut bytes: Vec<u8>| {
        if k.alt {
            bytes.insert(0, 0x1b);
        }
        bytes
    };

    let bytes = match k.key.as_str() {
        "enter" => alt_prefix(b"\r".to_vec()),
        "backspace" => alt_prefix(if k.ctrl { vec![0x08] } else { vec![0x7f] }),
        "tab" if k.shift => b"\x1b[Z".to_vec(),
        "tab" => alt_prefix(b"\t".to_vec()),
        "escape" => alt_prefix(b"\x1b".to_vec()),
        "space" if k.ctrl => vec![0x00],
        "space" => alt_prefix(b" ".to_vec()),
        "up" => csi('A'),
        "down" => csi('B'),
        "right" => csi('C'),
        "left" => csi('D'),
        "home" => csi('H'),
        "end" => csi('F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3('P'),
        "f2" => ss3('Q'),
        "f3" => ss3('R'),
        "f4" => ss3('S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        key if k.ctrl && key.chars().count() == 1 => {
            let c = key.chars().next()?.to_ascii_lowercase();
            let code = match c {
                'a'..='z' => c as u8 - b'a' + 1,
                '@' | '2' => 0x00,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '?' | '8' => 0x7f,
                _ => return k.text.clone().map(|t| alt_prefix(t.into_bytes())),
            };
            alt_prefix(vec![code])
        }
        // Option composed a different character (å, or @ [ ] on many
        // layouts): send it as typed, like Terminal.app by default.
        key if k.alt
            && k.text
                .as_deref()
                .is_some_and(|t| !t.eq_ignore_ascii_case(key)) =>
        {
            k.text.clone()?.into_bytes()
        }
        _ => alt_prefix(k.text.clone()?.into_bytes()),
    };
    Some(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    Drag(MouseButton),
    Move,
    WheelUp,
    WheelDown,
}

/// Encodes a mouse event at grid `col`/`row` (0-based) if the program asked
/// for mouse reporting. Uses SGR encoding when enabled, else legacy X10.
pub fn mouse_report(
    action: MouseAction,
    col: usize,
    row: usize,
    shift: bool,
    modes: Modes,
) -> Option<Vec<u8>> {
    if !(modes.mouse_report || modes.mouse_drag || modes.mouse_motion) {
        return None;
    }
    let button_code = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (mut code, release) = match action {
        MouseAction::Press(b) => (button_code(b), false),
        MouseAction::Release(b) => (if modes.sgr_mouse { button_code(b) } else { 3 }, true),
        MouseAction::Drag(b) if modes.mouse_drag || modes.mouse_motion => {
            (32 + button_code(b), false)
        }
        MouseAction::Move if modes.mouse_motion => (35, false),
        MouseAction::WheelUp => (64, false),
        MouseAction::WheelDown => (65, false),
        _ => return None,
    };
    if shift {
        code += 4;
    }
    let (x, y) = (col + 1, row + 1);
    if modes.sgr_mouse {
        let suffix = if release { 'm' } else { 'M' };
        Some(format!("\x1b[<{code};{x};{y}{suffix}").into_bytes())
    } else if x < 223 && y < 223 {
        Some(vec![
            0x1b,
            b'[',
            b'M',
            32 + code,
            32 + x as u8,
            32 + y as u8,
        ])
    } else {
        None
    }
}

/// Wraps pasted text in bracketed-paste markers when the program enabled them.
pub fn paste_bytes(text: &str, modes: Modes) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if modes.bracketed_paste {
        format!("\x1b[200~{}\x1b[201~", text.replace('\x1b', "")).into_bytes()
    } else {
        text.into_bytes()
    }
}

/// Turns pixel scroll deltas into whole lines, carrying the remainder so slow
/// trackpad scrolling still moves.
#[derive(Debug, Clone, Copy, Default)]
pub struct ScrollAccumulator {
    remainder: f32,
}

impl ScrollAccumulator {
    pub fn add(&mut self, pixels: f32, line_height: f32) -> i32 {
        let total = self.remainder + pixels / line_height;
        let lines = total.trunc();
        self.remainder = total - lines;
        lines as i32
    }
}
