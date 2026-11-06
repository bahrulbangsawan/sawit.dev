//! Copying an error out of Sawit in one step (backlog item 1): a terminal's last output and an
//! instance's log, as plain text to paste into an issue or an agent.

/// How many of an instance's last log lines Copy Logs copies (and Copy Errors looks through).
pub const LOG_LINES: usize = 200;

/// How many lines around an error line Copy Errors keeps, before and after: the statement or
/// the stack frames that say what went wrong.
pub const CONTEXT: usize = 2;

/// Whether `line` reports an error: PostgreSQL's `ERROR:`, `FATAL:` and `PANIC:` levels, and what
/// compilers, test runners and runtimes print (`error:`, `error[E0308]`, `Error:`, `panicked at`,
/// `Traceback`, `Exception`, `FAILED`, `npm ERR!`).
pub fn is_error_line(line: &str) -> bool {
    const LEVELS: [&str; 3] = ["ERROR", "FATAL", "PANIC"];
    if LEVELS
        .iter()
        .any(|l| line.contains(&format!("{l}:")) || line.contains(&format!("{l} ")))
    {
        return true;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("error:")
        || lower.contains("error[")
        || lower.contains("panicked at")
        || lower.starts_with("traceback")
        || lower.contains("exception")
        || line.contains("FAILED")
        || lower.contains("npm err!")
        || lower.trim_start().starts_with("failed")
}

/// The error lines of `text` ([`is_error_line`]) with [`CONTEXT`] lines around each, in their
/// order; runs that are apart are separated by a `…` line. Empty when there is no error line.
pub fn errors_only(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut keep = vec![false; lines.len()];
    for (i, line) in lines.iter().enumerate() {
        if is_error_line(line) {
            let from = i.saturating_sub(CONTEXT);
            let to = (i + CONTEXT).min(lines.len().saturating_sub(1));
            keep[from..=to].iter_mut().for_each(|k| *k = true);
        }
    }
    let mut out: Vec<&str> = Vec::new();
    let mut gap = false;
    for (line, kept) in lines.iter().zip(&keep) {
        if *kept {
            if gap && !out.is_empty() {
                out.push("…");
            }
            out.push(line);
            gap = false;
        } else {
            gap = true;
        }
    }
    out.join("\n")
}

/// Removes ANSI escape codes (CSI sequences such as colours, OSC sequences such as titles and
/// links, and lone escapes) from `text`, for logs written with colours.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// "Copied 42 lines", for the toast.
pub fn copied_note(text: &str) -> String {
    match text.lines().count() {
        1 => "Copied 1 line".to_string(),
        n => format!("Copied {n} lines"),
    }
}

/// "Copied 5 lines, 1 error", for what Copy Errors copied ([`errors_only`]).
pub fn errors_note(text: &str) -> String {
    let errors = text.lines().filter(|l| is_error_line(l)).count();
    let what = if errors == 1 { "error" } else { "errors" };
    format!("{}, {errors} {what}", copied_note(text))
}

/// The last `n` lines of `text`, in order.
pub fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_their_context_in_order() {
        let log = "a\nb\nc\nLOG:  ok\nERROR:  relation \"x\" does not exist\nSTATEMENT:  select * from x\nd\ne\nf\ng\nFATAL:  out of memory\nh";
        assert_eq!(
            errors_only(log),
            "c\nLOG:  ok\nERROR:  relation \"x\" does not exist\nSTATEMENT:  select * from x\nd\n…\nf\ng\nFATAL:  out of memory\nh"
        );
        assert_eq!(errors_only("all good\nfine"), "");
    }

    #[test]
    fn errors_of_tools_are_found() {
        for line in [
            "error[E0308]: mismatched types",
            "thread 'main' panicked at src/main.rs:2:5:",
            "Traceback (most recent call last):",
            "npm ERR! code ELIFECYCLE",
            "TypeError: x is not a function",
            "2026-11-06 10:00:00.000 UTC [42] PANIC:  could not write",
        ] {
            assert!(is_error_line(line), "{line}");
        }
        assert!(!is_error_line("LOG:  database system is ready"));
    }

    #[test]
    fn ansi_codes_go() {
        assert_eq!(
            strip_ansi(
                "\u{1b}[1;31merror\u{1b}[0m: \u{1b}]8;;http://x\u{1b}\\link\u{1b}]8;;\u{7}!"
            ),
            "error: link!"
        );
    }

    #[test]
    fn notes_and_tails() {
        assert_eq!(copied_note("one"), "Copied 1 line");
        assert_eq!(copied_note("a\nb\nc"), "Copied 3 lines");
        assert_eq!(errors_note("a\nERROR:  x\nb"), "Copied 3 lines, 1 error");
        assert_eq!(last_lines("1\n2\n3\n4", 2), "3\n4");
        assert_eq!(last_lines("1", 5), "1");
    }
}
