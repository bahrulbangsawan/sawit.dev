//! A PTY running a program, emulated by `alacritty_terminal`.

mod process;
mod snapshot;
mod spawn;
mod text;

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::event::WindowSize;
use alacritty_terminal::event_loop::{EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{Term, TermMode};

pub use snapshot::{CellView, CursorView, Snapshot};
use spawn::Listener;
pub use spawn::{Command, TermEvent, default_shell, multiplexer_markers, wrap_unsetting};
pub use text::SelectKind;

use super::input::Modes;

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

pub struct Session {
    term: Arc<FairMutex<Term<Listener>>>,
    pty: EventLoopSender,
    size: Arc<FairMutex<GridSize>>,
    /// What changed the screen, counted: output (by the listener), resizes, scrolls and
    /// selections ([`Session::changes`]).
    changes: Arc<AtomicU64>,
    /// The program's pid and a handle on the PTY master, to ask whether the
    /// program itself (not a job it started) is in the foreground.
    #[cfg(unix)]
    foreground: Option<(u32, std::fs::File)>,
}

impl Session {
    /// A count that moves on whenever what [`Session::snapshot`] shows may have changed: the
    /// program's output, a resize, a scroll or the selection. Read before a snapshot, it says
    /// whether that snapshot is still the screen: while the count stays, so does the screen.
    pub fn changes(&self) -> u64 {
        self.changes.load(Ordering::Acquire)
    }

    /// Counts a change to what a snapshot shows, made here rather than by output.
    fn changed(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    /// Types a `cd` into `dir` at the shell prompt, leaving no trace of it on
    /// screen. Only call when `at_prompt`.
    pub fn cd(&self, dir: &std::path::Path) {
        let (col, cols) = {
            let term = self.term.lock();
            (term.grid().cursor.point.column.0, term.columns())
        };
        self.write(super::input::cd_bytes(dir, col, cols));
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
        self.changed();
        let _ = self.pty.send(Msg::Resize(size.into()));
    }

    pub fn size(&self) -> GridSize {
        *self.size.lock()
    }

    /// Scrolls the view into history (`delta > 0` goes up).
    pub fn scroll(&self, delta: i32) {
        self.term.lock().scroll_display(Scroll::Delta(delta));
        self.changed();
    }

    pub fn scroll_to_bottom(&self) {
        let mut term = self.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
            self.changed();
        }
    }

    /// Input-relevant modes, without building a full snapshot.
    pub fn modes(&self) -> Modes {
        modes_of(*self.term.lock().mode())
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
