//! Timings of the terminal's hot paths, ignored by default. Run them optimised:
//! `cargo test --release -p sawit-core terminal::tests::timing -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use crate::terminal::{Command, GridSize, Palette, Session};

/// `rows` rows of `cols` columns of coloured words, as a busy program draws them: the 16
/// colours, bold, 256 colours, truecolour and backgrounds.
fn colourful_screen(cols: usize, rows: usize) -> Vec<u8> {
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

/// The median time of `f` over `n` runs, after `n / 10` runs to warm up.
fn median(n: usize, mut f: impl FnMut()) -> Duration {
    for _ in 0..n / 10 {
        f();
    }
    let mut times: Vec<Duration> = (0..n)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    times.sort();
    times[n / 2]
}

#[test]
#[ignore = "timing: run with --release -- --ignored --nocapture"]
fn snapshot_timing() {
    let (cols, rows) = (200, 50);
    let screen = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(screen.path(), colourful_screen(cols, rows - 1)).unwrap();
    let cmd = Command {
        program: "/bin/sh".into(),
        args: vec![
            "-c".into(),
            format!("cat '{}'; sleep 60", screen.path().display()),
        ],
        ..Command::default()
    };
    let size = GridSize {
        cols: cols as u16,
        rows: rows as u16,
        cell_width: 8,
        cell_height: 16,
    };
    let (session, _events) = Session::spawn(cmd, size).unwrap();
    let palette = Palette::default();
    let last = format!("word{:05}", rows - 2 + cols / 10 - 1);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !session.snapshot(&palette).text().contains(&last) {
        assert!(Instant::now() < deadline, "the screen was never drawn");
        std::thread::sleep(Duration::from_millis(20));
    }
    let time = median(2000, || {
        std::hint::black_box(session.snapshot(&palette));
    });
    println!("snapshot of a {cols}x{rows} screen: {time:?}");
}
