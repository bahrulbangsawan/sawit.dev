//! What the collector asks of the system for a status line run: the folder's git branch and
//! status, and the terminal's width.

use super::render::GitStatus;

/// The branch and status of the folder, from `git status --porcelain=v2 --branch` (1 s at most).
pub fn git_probe(dir: &str) -> (Option<String>, Option<GitStatus>) {
    if !std::path::Path::new(dir).exists() {
        return (None, None);
    }
    let Ok(child) = std::process::Command::new("/usr/bin/git")
        .args([
            "-C",
            dir,
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .spawn()
    else {
        return (None, None);
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });
    let Ok(Ok(out)) = rx.recv_timeout(std::time::Duration::from_secs(1)) else {
        return (None, None);
    };
    if !out.status.success() {
        return (None, None);
    }
    parse_git(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_git(text: &str) -> (Option<String>, Option<GitStatus>) {
    let mut branch = None;
    let mut s = GitStatus::default();
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("# branch.head ") {
            branch = Some(if h == "(detached)" {
                "detached".into()
            } else {
                h.to_string()
            });
        } else if let Some(ab) = l.strip_prefix("# branch.ab ") {
            for p in ab.split(' ') {
                if let Some(n) = p.strip_prefix('+') {
                    s.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = p.strip_prefix('-') {
                    s.behind = n.parse().unwrap_or(0);
                }
            }
        } else if l.starts_with("1 ") || l.starts_with("2 ") {
            let xy: Vec<char> = l.chars().skip(2).take(2).collect();
            if xy.first() != Some(&'.') {
                s.staged += 1;
            }
            if xy.get(1) != Some(&'.') {
                s.modified += 1;
            }
        } else if l.starts_with("u ") {
            s.conflicts += 1;
        } else if l.starts_with("? ") {
            s.untracked += 1;
        }
    }
    (branch, Some(s))
}

/// The terminal's width: `COLUMNS`, else the controlling terminal's.
pub fn terminal_width() -> Option<usize> {
    if let Some(c) = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|c| *c > 20)
    {
        return Some(c);
    }
    let fd = unsafe {
        libc::open(
            c"/dev/tty".as_ptr(),
            libc::O_RDONLY | libc::O_NOCTTY | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return None;
    }
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == 0;
    unsafe { libc::close(fd) };
    (ok && ws.ws_col > 20).then_some(ws.ws_col as usize)
}
