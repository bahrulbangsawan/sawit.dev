//! The usage collector: `sawit-statusline --account <id>`, which an account's `statusLine` runs
//! (the `sawit` binary under another name, as `sawit-mcp` is). Claude Code pipes it the status
//! line JSON; it keeps only the usage windows ([`usage::record`]), then prints the account's own
//! status line (the one it had before, run with the same input), or a short summary, or nothing.
//!
//! Rules from Claude Pool: never print on an error, always exit 0, no network, and read nothing
//! of the input but `rate_limits`, `version` and a hash of `session_id`.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::usage::{self, Report};
use super::{Accounts, AgentKind};

/// The name `sawit` runs as the collector under.
pub const PROGRAM: &str = "sawit-statusline";
/// The largest input read.
const MAX_INPUT: usize = 1 << 20;
/// How long the account's own status line may take.
const CHAIN_TIMEOUT: Duration = Duration::from_secs(2);

/// The link to the `sawit` executable an account's `statusLine` runs, in Sawit's data folder: it
/// stays put when the app moves or updates (the app refreshes it at launch).
pub fn link_path(root: &Path) -> PathBuf {
    super::store::dir(root).join("bin").join(PROGRAM)
}

/// Points the link at `exe` (this app's executable).
pub fn install_link(root: &Path, exe: &Path) -> crate::Result<()> {
    let link = link_path(root);
    super::store::private_dir(link.parent().expect("bin"))?;
    if std::fs::read_link(&link).ok().as_deref() == Some(exe) {
        return Ok(());
    }
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(exe, &link)?;
    Ok(())
}

/// The `statusLine` command for account `id`.
pub fn command(root: &Path, id: &str) -> String {
    format!(
        "{} --account {id}",
        crate::terminal::shell_quote(&link_path(root))
    )
}

/// Whether `status_line` (a `settings.json` value) is this collector's for some account.
pub fn is_ours(status_line: &Value) -> Option<String> {
    let cmd = status_line.get("command")?.as_str()?;
    if !cmd.contains(PROGRAM) {
        return None;
    }
    let (_, rest) = cmd.split_once("--account ")?;
    Some(rest.split_whitespace().next()?.to_string())
}

/// The account a report belongs to: the one whose folder is the session's `CLAUDE_CONFIG_DIR`
/// (unset: the default), since a session started in the home folder also runs the default
/// account's status line. Else the account named on the command line.
pub fn account_for(accounts: &Accounts, config_dir: Option<&str>, named: &str) -> String {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let found = match config_dir.filter(|d| !d.is_empty()) {
        None => accounts
            .of(AgentKind::ClaudeCode)
            .into_iter()
            .find(|a| a.is_default),
        Some(dir) => {
            let dir = canon(Path::new(dir));
            accounts
                .of(AgentKind::ClaudeCode)
                .into_iter()
                .find(|a| !a.is_default && canon(&a.dir) == dir)
        }
    };
    found.map(|a| a.id.clone()).unwrap_or_else(|| named.into())
}

/// `5h 23% · 7d 41%`.
pub fn summary(report: &Report) -> Option<String> {
    let parts: Vec<String> = [("5h", report.five_hour), ("7d", report.seven_day)]
        .into_iter()
        .filter_map(|(l, w)| w.map(|w| format!("{l} {}%", w.used_percentage.round())))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// What one collector run prints, for arguments `args` (after the program name), the status line
/// JSON `input`, the session's `CLAUDE_CONFIG_DIR`, Sawit's data folder `root` and the time.
pub fn run(
    args: &[String],
    input: &[u8],
    config_dir: Option<&str>,
    root: &Path,
    now: f64,
) -> Vec<u8> {
    let Some(named) = args
        .iter()
        .position(|a| a == "--account")
        .and_then(|i| args.get(i + 1))
    else {
        eprintln!("{PROGRAM}: usage: {PROGRAM} --account <id>");
        return Vec::new();
    };
    let record_only = args.iter().any(|a| a == "--record-only");
    let text = String::from_utf8_lossy(input);
    let Some(report) = Report::parse(&text) else {
        eprintln!("{PROGRAM}: the input isn't a JSON object");
        return Vec::new();
    };
    let accounts = Accounts::load(root).unwrap_or_default();
    let id = if record_only {
        named.clone()
    } else {
        account_for(&accounts, config_dir, named)
    };
    if let Err(e) = usage::record(root, &id, &report, now) {
        eprintln!("{PROGRAM}: {e}");
    }
    if record_only {
        return Vec::new();
    }
    let account = accounts.get(&id);
    if let Some(out) = super::statusline::render_managed(root, account, input, &report, now) {
        return out.into_bytes();
    }
    let chained = account
        .and_then(|a| a.original_status_line.as_ref())
        .and_then(|s| s.get("command"))
        .and_then(Value::as_str);
    if let Some(cmd) = chained {
        return chain(cmd, input).unwrap_or_default();
    }
    if accounts.settings.collector_output == "summary" {
        return summary(&report).unwrap_or_default().into_bytes();
    }
    Vec::new()
}

/// The account's own status line command, given the same input: its output when it exits 0
/// within 2 s, else nothing, as Claude Code treats a failing command.
pub fn chain(cmd: &str, input: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    let mut child = Command::new("/bin/sh")
        .args(["-c", cmd])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = input.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + CHAIN_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                eprintln!("{PROGRAM}: the account's own status line timed out");
                return None;
            }
        }
    }
}

/// The collector as a process: reads stdin, prints what [`run`] says, always exits 0.
pub fn main(args: &[String]) -> i32 {
    let mut input = Vec::new();
    let _ = std::io::stdin()
        .take(MAX_INPUT as u64 + 1)
        .read_to_end(&mut input);
    if input.len() > MAX_INPUT {
        eprintln!("{PROGRAM}: input larger than 1 MiB ignored");
        return 0;
    }
    let root = crate::instances::AppPaths::default_macos().root;
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let out = run(
        args,
        &input,
        config_dir.as_deref(),
        &root,
        crate::github::now() as f64,
    );
    use std::io::Write as _;
    let _ = std::io::stdout().write_all(&out);
    0
}

/// The status line JSON a usage check hands the collector: a made-up session and the windows.
pub fn probe_payload(reading: &Report, session: &str) -> String {
    let mut windows = serde_json::Map::new();
    for (key, w) in [
        ("five_hour", reading.five_hour),
        ("seven_day", reading.seven_day),
    ] {
        if let Some(w) = w {
            windows.insert(
                key.into(),
                json!({ "used_percentage": w.used_percentage, "resets_at": w.resets_at }),
            );
        }
    }
    json!({ "session_id": session, "rate_limits": windows }).to_string()
}
