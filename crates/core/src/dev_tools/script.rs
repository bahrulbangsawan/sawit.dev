//! Running a tool's installer, upgrade or uninstaller: one per tool at a time, in the login
//! shell, its output streamed line by line.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

use crate::{CoreError, Result};

use super::Tool;
use super::programs::PRELUDE;

/// A login shell to run a script in: the user's own when it runs POSIX
/// scripts, so the profile's `PATH` and variables apply, else zsh
/// ([`crate::probe::script_shell`]). `login` is `$SHELL`.
pub(super) fn shell(login: Option<&str>) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(crate::probe::script_shell(login));
    cmd.arg("-l")
        .stdin(Stdio::null())
        .env("NONINTERACTIVE", "1")
        .kill_on_drop(true);
    // A new session has no controlling terminal, so an installer that
    // reads /dev/tty fails fast instead of waiting for an answer.
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd
}

/// Runs `tool`'s installer, sending each line it prints to `on_line`.
/// Fails with the last lines of output when it exits non-zero. Dropping
/// the future stops the installer and everything it started.
pub async fn install(tool: &Tool, on_line: impl Fn(String) + Send + Sync) -> Result<()> {
    run_locked(tool.id, tool.name, tool.install, on_line).await
}

/// Runs `command` (`tool`'s installer or upgrade) like [`install`].
pub async fn run(tool: &Tool, command: &str, on_line: impl Fn(String) + Send + Sync) -> Result<()> {
    run_locked(tool.id, tool.name, command, on_line).await
}

/// Runs an uninstall `command` for `tool`, like [`install`].
pub async fn uninstall(
    tool: &Tool,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_locked(tool.id, tool.name, command, on_line).await
}

/// Runs `command` for the tool `id` while holding that tool's lock: one
/// installer, upgrade or uninstaller per tool at a time, whether the Dev
/// Tools page, an agent (`install_dev_tools`, `upgrade_dev_tools`,
/// `uninstall_dev_tools`), `sawit pasang`, `sawit pupuk` or `sawit cabut`
/// runs it. Two at once would write the same folders (rustup, nvm's clone,
/// uv's).
pub(super) async fn run_locked(
    id: &str,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    let _lock = lock_tool(id, name)?;
    run_script(name, command, on_line).await
}

/// Runs `command` in the login shell as the Dev Tools page runs an installer
/// (its output streamed to `on_line`, stopped with everything it started when
/// dropped), under the lock `id`: what the Skills page runs to install, update
/// or remove a pack. `name` says what is busy when the lock is held.
pub async fn run_shell(
    id: &str,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_locked(id, name, command, on_line).await
}

/// Holds tool `id`'s lock until dropped; fails at once when it's held.
fn lock_tool(id: &str, name: &str) -> Result<std::fs::File> {
    use std::os::fd::AsRawFd as _;
    let dir = std::env::temp_dir().join("sawit-tool-locks");
    std::fs::create_dir_all(&dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{id}.lock")))?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(CoreError::Config(format!(
                "{name} is already being installed, upgraded or removed (from the Dev Tools page, an agent, `sawit pasang`, `sawit pupuk` or `sawit cabut`). Try again when that's done."
            )));
        }
        return Err(e.into());
    }
    Ok(file)
}

/// Ends the process group `shell()` started (curl, bash and whatever the
/// script spawned) when the run is dropped before it exits.
struct KillGroup(Option<i32>);

impl Drop for KillGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            unsafe {
                libc::killpg(pid, libc::SIGTERM);
            }
        }
    }
}

pub(super) async fn run_script(
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    run_script_as(
        crate::probe::login_shell().as_deref(),
        name,
        command,
        on_line,
    )
    .await
}

/// [`run_script`] with `login` as `$SHELL`.
pub(super) async fn run_script_as(
    login: Option<&str>,
    name: &str,
    command: &str,
    on_line: impl Fn(String) + Send + Sync,
) -> Result<()> {
    let script = format!("{PRELUDE}set -e\n{command}\n");
    let mut cmd = shell(login);
    cmd.arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    // `setsid` made the shell a group leader, so its pid is the group's.
    let mut group = KillGroup(child.id().map(|pid| pid as i32));
    let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
    let tail = std::sync::Mutex::new(Vec::<String>::new());
    let read = |pipe: Option<Box<dyn AsyncRead + Unpin + Send>>| {
        let (on_line, tail) = (&on_line, &tail);
        async move {
            let Some(pipe) = pipe else { return };
            let mut pipe = BufReader::new(pipe);
            let mut bytes = Vec::new();
            // Bytes, not `lines()`: a line that isn't UTF-8 would end the
            // reading there and close the pipe under the installer.
            loop {
                bytes.clear();
                match pipe.read_until(b'\n', &mut bytes).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let line = String::from_utf8_lossy(&bytes);
                let line = strip_ansi(line.trim_end_matches(['\n', '\r']));
                // Progress bars redraw with \r; keep what's left at the end.
                let line = line
                    .rsplit('\r')
                    .next()
                    .unwrap_or_default()
                    .trim_end()
                    .to_string();
                if line.trim().is_empty() {
                    continue;
                }
                {
                    let mut tail = tail.lock().unwrap();
                    tail.push(line.clone());
                    if tail.len() > 8 {
                        tail.remove(0);
                    }
                }
                on_line(line);
            }
        }
    };
    let wait = async {
        futures::join!(
            read(stdout.map(|p| Box::new(p) as Box<dyn AsyncRead + Unpin + Send>)),
            read(stderr.map(|p| Box::new(p) as Box<dyn AsyncRead + Unpin + Send>)),
        );
        child.wait().await
    };
    let status = tokio::time::timeout(Duration::from_secs(15 * 60), wait)
        .await
        .map_err(|_| CoreError::Command {
            program: name.into(),
            code: None,
            stderr: "timed out after 15 minutes".into(),
        })??;
    group.0 = None;
    if status.success() {
        Ok(())
    } else {
        Err(CoreError::Command {
            program: name.into(),
            code: status.code(),
            stderr: tail.into_inner().unwrap().join("\n"),
        })
    }
}

/// Drops terminal colour and cursor escapes (`ESC [ … letter`).
pub(super) fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}
