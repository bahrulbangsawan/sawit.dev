//! Frame timings of a terminal, ignored by default. Run them optimised:
//! `cargo test --release -p sawit terminal::view::timing -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use gpui_kit::{
    AppContext as _, Bounds, Entity, TestAppContext, WindowBounds, WindowOptions, px, size,
};
use sawit_core::terminal::{Command, GridSize, SelectKind, Session};

use super::TerminalView;

/// `rows` rows of `cols` columns of coloured words, as a busy program draws them: the 16
/// colours, bold, 256 colours, truecolour and backgrounds.
fn colourful_lines(cols: usize, rows: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for row in 0..rows {
        for word in 0..cols / 10 {
            let n = row + word;
            let style = match n % 6 {
                0 => format!("\x1b[3{}m", n % 8),
                1 => format!("\x1b[1;9{}m", n % 8),
                2 => format!("\x1b[38;5;{}m", 16 + n % 216),
                3 => format!("\x1b[38;2;{};{};{}m", n % 256, n * 7 % 256, n * 13 % 256),
                4 => format!("\x1b[4{};30m", n % 8),
                _ => "\x1b[0m".to_string(),
            };
            out.extend_from_slice(format!("{style}word{n:05} ").as_bytes());
        }
        out.extend_from_slice(if row + 1 < rows {
            b"\x1b[0m\r\n"
        } else {
            b"\x1b[0m"
        });
    }
    out
}

/// The median time of a frame drawn after `change`, over `n` frames after `n / 10` to warm up.
fn median_frame(
    cx: &mut TestAppContext,
    view: &Entity<TerminalView>,
    n: usize,
    mut change: impl FnMut(&Session, usize),
) -> Duration {
    let mut frame = |cx: &mut TestAppContext, i: usize| {
        let start = Instant::now();
        cx.update(|cx| {
            view.update(cx, |v, cx| {
                change(&v.session, i);
                cx.notify();
            })
        });
        start.elapsed()
    };
    for i in 0..n / 10 {
        frame(cx, i);
    }
    let mut times: Vec<Duration> = (0..n).map(|i| frame(cx, i)).collect();
    times.sort();
    times[n / 2]
}

/// A window of one terminal, 200 columns by 50 rows of coloured text with a screenful of
/// history above, drawn again and again: unchanged (a pane redrawn for another's output),
/// with one line selected in turn (two rows change), and scrolled a line up and down (every
/// row changes).
#[gpui_kit::test]
#[ignore = "timing: run with --release -- --ignored --nocapture"]
fn frame_timing(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    cx.executor().allow_parking();
    let (cols, rows) = (200, 50);
    let screen = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(screen.path(), colourful_lines(cols, 2 * rows)).unwrap();
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("sleep 1; cat '{}'; sleep 600", screen.path().display()),
        ],
        ..Command::default()
    };
    let grid = GridSize {
        cols: cols as u16 + 3,
        rows: rows as u16,
        cell_width: 8,
        cell_height: 17,
    };
    let (session, events) = Session::spawn(cmd, grid).unwrap();
    let (_, view) = cx.update(|cx| {
        let bounds = Bounds::centered(None, size(px(1600.), px(rows as f32 * 17.)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |_, cx| {
            cx.new(|cx| TerminalView::new(session, events, "timing", cx))
        })
        .unwrap()
    });
    let last = format!("word{:05}", 2 * rows - 1 + cols / 10 - 1);
    let deadline = Instant::now() + Duration::from_secs(20);
    while !cx.update(|cx| view.read(cx).text_tail(1).contains(&last)) {
        assert!(Instant::now() < deadline, "the text was never drawn");
        std::thread::sleep(Duration::from_millis(20));
        cx.run_until_parked();
    }
    let size = cx.update(|cx| view.read(cx).session.size());
    let unchanged = median_frame(cx, &view, 500, |_, _| {});
    let selected = median_frame(cx, &view, 500, |s, i| {
        s.select_start(0, i % rows, false, SelectKind::Line)
    });
    let scrolled = median_frame(cx, &view, 500, |s, i| {
        s.scroll(if i % 2 == 0 { 1 } else { -1 })
    });
    println!(
        "frames of a {}x{} terminal: unchanged {unchanged:?}, a line selected {selected:?}, \
         scrolled {scrolled:?}",
        size.cols, size.rows
    );
}
