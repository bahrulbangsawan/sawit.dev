//! A PTY running a program, emulated by `alacritty_terminal`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use futures::channel::mpsc;

use super::input::Modes;
use super::palette::{Palette, Rgb};
use crate::{CoreError, Result};

/// Grid size plus cell size in pixels (programs may query the pixel size).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

impl From<GridSize> for WindowSize {
    fn from(s: GridSize) -> WindowSize {
        WindowSize {
            num_lines: s.rows,
            num_cols: s.cols,
            cell_width: s.cell_width,
            cell_height: s.cell_height,
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellView {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikeout: bool,
    /// Second half of a wide character; draw nothing.
    pub spacer: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorView {
    Block,
    Beam,
    Underline,
    Hidden,
}

/// Everything needed to draw the visible screen.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cols: usize,
    pub lines: Vec<Vec<CellView>>,
    /// (row, col) of the cursor when it is on screen.
    pub cursor: Option<(usize, usize)>,
    pub cursor_shape: CursorView,
    pub modes: Modes,
    /// Lines scrolled back into history (0 = live view).
    pub display_offset: usize,
}

impl Snapshot {
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|l| {
                l.iter()
                    .filter(|c| !c.spacer)
                    .map(|c| c.ch)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Variables that tell a program it runs inside another multiplexer session
/// (herdr refuses to nest when it sees its own). Terminals opened by Sawit
/// are new top-level sessions, so these are removed.
pub fn multiplexer_markers(names: impl Iterator<Item = String>) -> Vec<String> {
    let mut found: Vec<String> = names
        .filter(|n| {
            n.starts_with("HERDR_") || n.starts_with("ZELLIJ") || n == "TMUX" || n == "TMUX_PANE"
        })
        .collect();
    found.sort();
    found
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
struct Listener {
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

pub struct Session {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    size: Arc<FairMutex<GridSize>>,
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
            (
                std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()),
                vec!["-l".to_string()],
            )
        } else {
            (cmd.program.clone(), cmd.args.clone())
        };
        let unset =
            multiplexer_markers(std::env::vars_os().filter_map(|(k, _)| k.into_string().ok()));
        let (program, args) = wrap_unsetting(&program, &args, &unset);
        let shell = tty::Shell::new(program, args);
        let options = tty::Options {
            shell: Some(shell),
            working_directory: cmd.cwd.clone(),
            drain_on_exit: true,
            env,
        };
        let pty = tty::new(&options, size.into(), 0).map_err(CoreError::Io)?;

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
            },
            rx,
        ))
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    pub fn resize(&self, size: GridSize) {
        {
            let mut current = self.size.lock();
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term.lock().resize(size);
        let _ = self.pty.send(Msg::Resize(size.into()));
    }

    pub fn size(&self) -> GridSize {
        *self.size.lock()
    }

    /// Scrolls the view into history (`delta > 0` goes up).
    pub fn scroll(&self, delta: i32) {
        self.term.lock().scroll_display(Scroll::Delta(delta));
    }

    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Input-relevant modes, without building a full snapshot.
    pub fn modes(&self) -> Modes {
        modes_of(*self.term.lock().mode())
    }

    pub fn snapshot(&self, palette: &Palette) -> Snapshot {
        let term = self.term.lock();
        let content = term.renderable_content();
        let cols = term.columns();
        let rows = term.screen_lines();
        let colors = content.colors;
        let resolve = |color: Color, is_fg: bool| -> Rgb {
            let from_term = |ix: usize| {
                colors[ix].map(|c| Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                })
            };
            match color {
                Color::Spec(c) => Rgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                },
                Color::Indexed(ix) => from_term(ix as usize).unwrap_or_else(|| palette.indexed(ix)),
                Color::Named(named) => {
                    let ix = named as usize;
                    from_term(ix).unwrap_or_else(|| match named {
                        NamedColor::Foreground
                        | NamedColor::BrightForeground
                        | NamedColor::DimForeground => palette.foreground,
                        NamedColor::Background => palette.background,
                        NamedColor::Cursor => palette.cursor,
                        n if (n as usize) < 16 => palette.ansi[n as usize],
                        // Dim variants map onto the normal colors.
                        n => {
                            let base = (n as usize).saturating_sub(NamedColor::DimBlack as usize);
                            if base < 8 {
                                palette.ansi[base]
                            } else if is_fg {
                                palette.foreground
                            } else {
                                palette.background
                            }
                        }
                    })
                }
            }
        };

        let blank = CellView {
            ch: ' ',
            fg: palette.foreground,
            bg: palette.background,
            bold: false,
            italic: false,
            underline: false,
            strikeout: false,
            spacer: false,
        };
        let mut lines = vec![vec![blank; cols]; rows];
        let offset = content.display_offset as i32;
        for indexed in content.display_iter {
            let row = indexed.point.line.0 + offset;
            let col = indexed.point.column.0;
            if row < 0 || row as usize >= rows || col >= cols {
                continue;
            }
            let cell = indexed.cell;
            let flags = cell.flags;
            let mut fg = resolve(cell.fg, true);
            let mut bg = resolve(cell.bg, false);
            if flags.contains(Flags::BOLD)
                && let Color::Named(n) = cell.fg
                && (n as usize) < 8
            {
                fg = palette.ansi[n as usize + 8];
            }
            if flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if flags.contains(Flags::DIM) {
                fg = Rgb {
                    r: fg.r / 3 * 2,
                    g: fg.g / 3 * 2,
                    b: fg.b / 3 * 2,
                };
            }
            if flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            lines[row as usize][col] = CellView {
                ch: if cell.c == '\0' || cell.c == '\t' {
                    ' '
                } else {
                    cell.c
                },
                fg,
                bg,
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strikeout: flags.contains(Flags::STRIKEOUT),
                spacer: flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER),
            };
        }

        let mode = content.mode;
        let cursor_point: Point = content.cursor.point;
        let cursor_row = cursor_point.line.0 + offset;
        let cursor_visible = mode.contains(TermMode::SHOW_CURSOR)
            && content.cursor.shape != CursorShape::Hidden
            && cursor_row >= 0
            && (cursor_row as usize) < rows;
        let cursor = cursor_visible.then(|| {
            (
                cursor_row as usize,
                cursor_point.column.0.min(cols.saturating_sub(1)),
            )
        });
        let cursor_shape = match content.cursor.shape {
            CursorShape::Beam => CursorView::Beam,
            CursorShape::Underline => CursorView::Underline,
            CursorShape::Hidden => CursorView::Hidden,
            _ => CursorView::Block,
        };
        let modes = modes_of(mode);
        Snapshot {
            cols,
            lines,
            cursor,
            cursor_shape,
            modes,
            display_offset: content.display_offset,
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.pty.send(Msg::Shutdown);
    }
}

fn modes_of(mode: TermMode) -> Modes {
    Modes {
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        mouse_report: mode.contains(TermMode::MOUSE_REPORT_CLICK),
        mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
        mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
        sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
        alt_screen: mode.contains(TermMode::ALT_SCREEN),
        alternate_scroll: mode.contains(TermMode::ALTERNATE_SCROLL),
    }
}
