//! Starting a program in a new PTY, and the listener that turns the emulator's events into
//! [`TermEvent`]s.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::tty;
use futures::channel::mpsc;

use super::{GridSize, Session};
use crate::terminal::palette::Palette;
use crate::{CoreError, Result};

/// What to run in the terminal. `Default` is the user's login shell.
#[derive(Debug, Clone, Default)]
pub struct Command {
    /// Empty means `$SHELL` (falling back to `/bin/zsh`) as a login shell.
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    /// New output; redraw.
    Wakeup,
    Title(String),
    Bell,
    /// The program exited.
    Exit,
    /// The program asked to copy text (OSC 52).
    Copy(String),
}

impl Session {
    /// Starts `cmd` in a new PTY. Events arrive on the returned receiver.
    pub fn spawn(
        cmd: Command,
        size: GridSize,
    ) -> Result<(Session, mpsc::UnboundedReceiver<TermEvent>)> {
        let (tx, rx) = mpsc::unbounded();
        let pty_slot = Arc::new(OnceLock::new());
        let size_cell = Arc::new(FairMutex::new(size));
        let listener = Listener {
            events: tx,
            pty: pty_slot.clone(),
            size: size_cell.clone(),
        };

        let mut env = cmd.env.clone();
        env.entry("TERM".into())
            .or_insert_with(|| "xterm-256color".into());
        env.entry("COLORTERM".into())
            .or_insert_with(|| "truecolor".into());
        env.entry("TERM_PROGRAM".into())
            .or_insert_with(|| "sawit".into());
        let (program, args) = if cmd.program.is_empty() {
            default_shell()
        } else {
            (cmd.program.clone(), cmd.args.clone())
        };
        // `/usr/bin/env -u` is Unix-only; Windows has no multiplexers to hide.
        let (program, args) = if cfg!(unix) {
            let unset =
                multiplexer_markers(std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()));
            wrap_unsetting(&program, &args, &unset)
        } else {
            (program, args)
        };
        let shell = tty::Shell::new(program, args);
        let options = tty::Options {
            shell: Some(shell),
            working_directory: cmd.cwd.clone(),
            drain_on_exit: true,
            env,
        };
        let pty = tty::new(&options, size.into(), 0).map_err(CoreError::Io)?;
        #[cfg(unix)]
        let foreground = pty
            .file()
            .try_clone()
            .ok()
            .map(|file| (pty.child().id(), file));

        let config = Config {
            scrolling_history: 10_000,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let event_loop =
            EventLoop::new(term.clone(), listener, pty, true, false).map_err(CoreError::Io)?;
        let sender = event_loop.channel();
        let _ = pty_slot.set(sender.clone());
        event_loop.spawn();
        Ok((
            Session {
                term,
                pty: sender,
                size: size_cell,
                #[cfg(unix)]
                foreground,
            },
            rx,
        ))
    }
}

/// Variables that tell a program it runs inside another multiplexer session
/// (a program may refuse to run nested: herdr, tmux, Zellij). Terminals
/// opened by Sawit are new top-level sessions, so these are removed.
pub fn multiplexer_markers(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut found: Vec<String> = names
        .filter(|n| {
            n.starts_with("HERDR_") || n.starts_with("ZELLIJ") || n == "TMUX" || n == "TMUX_PANE"
        })
        .collect();
    found.sort();
    found
}

/// The user's shell: `$SHELL` as a login shell on macOS/Linux, PowerShell on
/// Windows.
pub fn default_shell() -> (String, Vec<String>) {
    if cfg!(windows) {
        ("powershell.exe".to_string(), vec!["-NoLogo".to_string()])
    } else {
        let fallback = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/sh"
        };
        (
            std::env::var("SHELL").unwrap_or_else(|_| fallback.into()),
            vec!["-l".to_string()],
        )
    }
}

/// Runs `program` through `/usr/bin/env -u NAME…` when there is something to unset.
pub fn wrap_unsetting(program: &str, args: &[String], unset: &[String]) -> (String, Vec<String>) {
    if unset.is_empty() {
        return (program.to_string(), args.to_vec());
    }
    let mut wrapped = Vec::with_capacity(unset.len() * 2 + 1 + args.len());
    for name in unset {
        wrapped.push("-u".to_string());
        wrapped.push(name.clone());
    }
    wrapped.push(program.to_string());
    wrapped.extend(args.iter().cloned());
    ("/usr/bin/env".to_string(), wrapped)
}

#[derive(Clone)]
pub(super) struct Listener {
    events: mpsc::UnboundedSender<TermEvent>,
    pty: Arc<OnceLock<EventLoopSender>>,
    size: Arc<FairMutex<GridSize>>,
}

impl Listener {
    fn reply(&self, text: String) {
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let forwarded = match event {
            Event::Wakeup => Some(TermEvent::Wakeup),
            Event::Title(t) => Some(TermEvent::Title(t)),
            Event::ResetTitle => Some(TermEvent::Title(String::new())),
            Event::Bell => Some(TermEvent::Bell),
            Event::Exit | Event::ChildExit(_) => Some(TermEvent::Exit),
            Event::ClipboardStore(_, text) => Some(TermEvent::Copy(text)),
            Event::PtyWrite(text) => {
                self.reply(text);
                None
            }
            Event::ColorRequest(ix, format) => {
                let p = Palette::default();
                let c = match ix {
                    0..=255 => p.indexed(ix as u8),
                    257 => p.background,
                    258 => p.cursor,
                    _ => p.foreground,
                };
                self.reply(format(alacritty_terminal::vte::ansi::Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                }));
                None
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *self.size.lock();
                self.reply(format(size.into()));
                None
            }
            _ => None,
        };
        if let Some(e) = forwarded {
            let _ = self.events.unbounded_send(e);
        }
    }
}
