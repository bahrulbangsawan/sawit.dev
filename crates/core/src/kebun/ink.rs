//! The terminal side of the tools: colours ([`Ink`]), yes/no questions, clipped text and `~`
//! paths.

use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

/// Asks on the terminal; `None` when stdin isn't one.
pub(super) fn confirm(question: &str) -> Option<bool> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let ink = Ink::stdout();
    print!("  {} {question} [y/N] ", ink.yellow("?"));
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok()?;
    Some(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "ya" | "yes"
    ))
}

/// [`confirm`], asked on stderr: `sawit <db>` keeps stdout for its tables.
pub(super) fn confirm_on_stderr(question: &str) -> Option<bool> {
    if !std::io::stdin().is_terminal() {
        return None;
    }
    let ink = Ink::stderr();
    eprint!("  {} {question} [y/N] ", ink.yellow("?"));
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer).ok()?;
    Some(matches!(
        answer.trim().to_lowercase().as_str(),
        "y" | "ya" | "yes"
    ))
}

/// `text` cut to `max` characters, with `…` when shortened.
pub(super) fn clip(text: &str, max: usize) -> String {
    match text.char_indices().nth(max - 1) {
        Some((ix, _)) if text.chars().count() > max => format!("{}…", &text[..ix]),
        _ => text.into(),
    }
}

/// `/Users/me/x` → `~/x`.
pub(super) fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// ANSI colors, off when the stream isn't a terminal or `NO_COLOR` is set
/// (the same look as the `~/.zsh_functions` scripts).
pub(super) struct Ink {
    on: bool,
}

impl Ink {
    pub(super) fn stdout() -> Ink {
        Ink::new(std::io::stdout().is_terminal())
    }

    pub(super) fn stderr() -> Ink {
        Ink::new(std::io::stderr().is_terminal())
    }

    pub(super) fn new(terminal: bool) -> Ink {
        Ink {
            on: terminal && std::env::var_os("NO_COLOR").is_none(),
        }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        match self.on {
            true => format!("\x1b[{code}m{text}\x1b[0m"),
            false => text.to_string(),
        }
    }

    pub(super) fn red(&self, t: &str) -> String {
        self.paint("1;31", t)
    }

    pub(super) fn green(&self, t: &str) -> String {
        self.paint("1;32", t)
    }

    pub(super) fn yellow(&self, t: &str) -> String {
        self.paint("1;33", t)
    }

    pub(super) fn gray(&self, t: &str) -> String {
        self.paint("90", t)
    }

    fn rule(&self) {
        println!("{}", self.gray(&"─".repeat(60)));
    }

    pub(super) fn header(&self, title: &str, subtitle: &str) {
        println!();
        println!("{}", self.paint("1;34", title));
        println!("{}", self.gray(subtitle));
        self.rule();
    }

    pub(super) fn section(&self, title: &str) {
        println!();
        println!("{}", self.paint("1;33", &format!("▸ {title}")));
    }

    pub(super) fn empty(&self, text: &str) {
        println!("  {}", self.gray(&format!("({text})")));
    }

    pub(super) fn warn(&self, text: &str) {
        println!("  {} {text}", self.yellow("!"));
    }

    pub(super) fn kv(&self, key: &str, value: &str) {
        println!("  {} {value}", self.gray(&format!("{key:<20}")));
    }

    pub(super) fn footer(&self, text: &str) {
        self.rule();
        println!("{}", self.green(text));
        println!();
    }
}
