//! Running Claude Code itself for an account: `claude auth status`, and the usage check. Always
//! the unmodified `claude`, in an environment built from nothing ([`claude::environment`]).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::claude::{self, AuthStatus};
use super::{Account, tracking, usage};
use crate::{CoreError, Result};

/// The PATH children get: Homebrew's and the system's, plus where Claude Code installs itself.
pub fn base_path(home: &Path) -> String {
    [
        home.join(".local/bin").to_string_lossy().to_string(),
        "/opt/homebrew/bin".into(),
        "/usr/local/bin".into(),
        "/usr/bin".into(),
        "/bin".into(),
        "/usr/sbin".into(),
        "/sbin".into(),
    ]
    .join(":")
}

/// The `claude` to run: `SAWIT_CLAUDE` (tests point it at a stand-in), else the first on `PATH`,
/// else the usual install places.
pub fn find_claude(home: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("SAWIT_CLAUDE").map(PathBuf::from)
        && p.is_file()
    {
        return Some(p);
    }
    let path = std::env::var("PATH").unwrap_or_default();
    path.split(':')
        .map(PathBuf::from)
        .chain([
            home.join(".local/bin"),
            home.join(".claude/local"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ])
        .map(|d| d.join("claude"))
        .find(|p| p.is_file())
}

/// What a finished command printed.
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `program args` as `account`, in `cwd`, for at most `timeout`.
pub fn run_as(
    program: &Path,
    args: &[&str],
    account: &Account,
    home: &Path,
    cwd: &Path,
    timeout: Duration,
) -> Result<Output> {
    let user = std::env::var("USER").unwrap_or_default();
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .envs(claude::environment(account, home, &user, &base_path(home)))
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    let (mut out, mut err) = (child.stdout.take(), child.stderr.take());
    let read = |p: Option<std::process::ChildStdout>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut p) = p {
                use std::io::Read as _;
                let _ = p.read_to_string(&mut s);
            }
            s
        })
    };
    let stdout = read(out.take());
    let stderr = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(mut p) = err.take() {
            use std::io::Read as _;
            let _ = p.read_to_string(&mut s);
        }
        s
    });
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(CoreError::Config(format!(
                "{} didn't answer in {} seconds.",
                program.display(),
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        code: status.code().unwrap_or(-1),
        stdout: stdout.join().unwrap_or_default(),
        stderr: stderr.join().unwrap_or_default(),
    })
}

/// `claude auth status` for `account`.
pub fn auth_status(claude_bin: &Path, account: &Account, home: &Path) -> Result<AuthStatus> {
    let out = run_as(
        claude_bin,
        &["auth", "status"],
        account,
        home,
        home,
        Duration::from_secs(10),
    )?;
    claude::parse_auth_status(&out.stdout, out.code).ok_or_else(|| {
        CoreError::Config(format!(
            "Couldn't check the sign-in (claude auth status exited with code {}). Run it in a terminal to see why.",
            out.code
        ))
    })
}

/// "Check Usage Now": Claude Code sends one tiny request as `account` and reports the windows,
/// which are recorded as a new session's report. Runs in an empty folder, so no project settings
/// apply.
pub fn check_usage(
    claude_bin: &Path,
    account: &Account,
    home: &Path,
    root: &Path,
) -> Result<usage::Snapshot> {
    let dir = super::store::dir(root).join("usage-check");
    super::store::private_dir(&dir)?;
    let out = run_as(
        claude_bin,
        tracking::PROBE_ARGS,
        account,
        home,
        &dir,
        Duration::from_secs(45),
    )?;
    let Some(mut report) = tracking::parse_probe(&out.stdout) else {
        if out.code != 0 {
            let why = out.stderr.lines().last().unwrap_or("").trim().to_string();
            return Err(CoreError::Config(format!(
                "Couldn't check usage: {}",
                if why.is_empty() {
                    format!("claude exited with code {}.", out.code)
                } else {
                    why
                }
            )));
        }
        return Err(CoreError::Config(
            "Claude Code didn't report usage for this account. It does only for claude.ai Pro and Max sign-ins.".into(),
        ));
    };
    report.session_key = Some(format!("usage-check-{}", super::new_id()));
    usage::record(root, &account.id, &report, crate::github::now() as f64)
}
