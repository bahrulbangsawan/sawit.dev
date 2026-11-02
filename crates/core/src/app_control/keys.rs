//! The keys `terminal_input` can press, by name, and what a terminal receives for each.

/// The keys [`key_bytes`] knows, as `terminal_input` and its errors list them.
pub const KEY_NAMES: &str = "enter, tab, escape, backspace, space, up, down, left, right, home, \
end, pageup, pagedown, delete or ctrl-<letter> (return, esc and ctrl+<letter> work too)";

/// What a terminal receives for a named key, one of [`KEY_NAMES`] (in any
/// case), or `ctrl-[` and `ctrl-\`.
pub fn key_bytes(name: &str) -> Option<Vec<u8>> {
    let name = name.trim().to_ascii_lowercase();
    let fixed: &[u8] = match name.as_str() {
        "enter" | "return" => b"\r",
        "tab" => b"\t",
        "escape" | "esc" => b"\x1b",
        "backspace" => b"\x7f",
        "space" => b" ",
        "up" => b"\x1b[A",
        "down" => b"\x1b[B",
        "right" => b"\x1b[C",
        "left" => b"\x1b[D",
        "home" => b"\x1b[H",
        "end" => b"\x1b[F",
        "pageup" => b"\x1b[5~",
        "pagedown" => b"\x1b[6~",
        "delete" => b"\x1b[3~",
        _ => {
            let letter = name
                .strip_prefix("ctrl-")
                .or_else(|| name.strip_prefix("ctrl+"))?;
            let [c] = letter.as_bytes() else { return None };
            return match c {
                b'a'..=b'z' => Some(vec![c & 0x1f]),
                b'[' => Some(vec![0x1b]),
                b'\\' => Some(vec![0x1c]),
                _ => None,
            };
        }
    };
    Some(fixed.to_vec())
}
