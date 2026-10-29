//! Cluster lifecycle via `initdb` / `pg_ctl`, with a port preflight check and
//! a live log tail.

use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc::UnboundedSender;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::instances::{AppPaths, Instance, ListenScope, validate_socket_dir};
use crate::pgctl::{PgCtlStatus, parse_status};
use crate::ports::PortVerdict;
use crate::probe::{PortReport, check_port};
use crate::{CoreError, Result, cmd};

const QUICK: Duration = Duration::from_secs(15);
const SLOW: Duration = Duration::from_secs(60);

fn failed(program: &str, out: &cmd::Output) -> CoreError {
    CoreError::Command {
        program: program.into(),
        code: out.code,
        stderr: out.combined().trim().to_string(),
    }
}

/// Creates a new cluster: superuser `postgres`, trust auth (local only), UTF8, C locale.
pub async fn init_cluster(inst: &Instance) -> Result<()> {
    if let Some(parent) = inst.data_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let data = inst.data_dir.as_os_str();
    let args = [
        "-D".as_ref(),
        data,
        "-U".as_ref(),
        "postgres".as_ref(),
        "-A".as_ref(),
        "trust".as_ref(),
        "-E".as_ref(),
        "UTF8".as_ref(),
        "--locale=C".as_ref(),
    ];
    let out = cmd::run(inst.bin_dir.join("initdb"), args, SLOW).await?;
    if out.code != Some(0) {
        return Err(failed("initdb", &out));
    }
    Ok(())
}

pub async fn status(inst: &Instance) -> Result<PgCtlStatus> {
    let out = cmd::run(
        inst.bin_dir.join("pg_ctl"),
        ["status".as_ref(), "-D".as_ref(), inst.data_dir.as_os_str()],
        QUICK,
    )
    .await?;
    parse_status(out.code.unwrap_or(-1), &out.combined())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preflight {
    Check,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    Started,
    AlreadyRunning {
        pid: u32,
    },
    /// The preflight found other listeners; nothing was started.
    Blocked(PortReport),
}

/// pg_ctl runs `exec postgres -D "<dir>" ... >> "<log>"` through `sh`, so
/// paths with shell metacharacters inside double quotes would be expanded.
pub(crate) fn check_shell_safe_path(path: &Path) -> Result<()> {
    let text = path
        .to_str()
        .ok_or_else(|| CoreError::Config("This path isn't valid UTF-8. Rename the folder and try again.".into()))?;
    if text.contains(['"', '$', '`', '\\']) {
        return Err(CoreError::Config(format!(
            "Path {text} contains \" $ ` or \\, which are unsafe for pg_ctl; move or rename the folder."
        )));
    }
    Ok(())
}

/// Options passed through `pg_ctl -o`, which runs them via the shell, so every
/// value is single-quoted (`*` would otherwise glob).
pub(crate) fn postgres_options(
    port: u16,
    listen: ListenScope,
    socket_dir: &Path,
) -> Result<String> {
    let dir = socket_dir
        .to_str()
        .ok_or_else(|| CoreError::Config("The socket folder path isn't valid UTF-8. Move Sawit's data to another folder.".into()))?;
    if dir.contains('\'') {
        return Err(CoreError::Config(format!(
            "The socket folder path can't contain a quote ('): {dir}. Rename the folder."
        )));
    }
    Ok(format!(
        "-p {port} -c listen_addresses='{}' -k '{dir}'",
        listen.listen_addresses()
    ))
}

/// Starts the cluster unless it is already running or (with `Preflight::Check`)
/// another process listens on the port. `managed` lists every Sawit instance
/// (name, data dir) so their listeners are named correctly.
pub async fn start(
    inst: &Instance,
    paths: &AppPaths,
    managed: Vec<(String, PathBuf)>,
    preflight: Preflight,
) -> Result<StartOutcome> {
    if let PgCtlStatus::Running { pid } = status(inst).await? {
        return Ok(StartOutcome::AlreadyRunning { pid });
    }
    let report = check_port(inst.port, &inst.listen.bind_addrs(), managed).await?;
    if preflight == Preflight::Check && report.verdict != PortVerdict::Free {
        return Ok(StartOutcome::Blocked(report));
    }

    let socket_dir = paths.socket_dir();
    validate_socket_dir(&socket_dir, inst.port)?;
    let log = paths.log_file(&inst.name);
    check_shell_safe_path(&inst.data_dir)?;
    check_shell_safe_path(&log)?;
    tokio::fs::create_dir_all(&socket_dir).await?;
    if let Some(dir) = log.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let opts = postgres_options(inst.port, inst.listen, &socket_dir)?;
    let args = [
        "start".as_ref(),
        "-w".as_ref(),
        "-t".as_ref(),
        "30".as_ref(),
        "-D".as_ref(),
        inst.data_dir.as_os_str(),
        "-l".as_ref(),
        log.as_os_str(),
        "-o".as_ref(),
        opts.as_ref(),
    ];
    let out = cmd::run(inst.bin_dir.join("pg_ctl"), args, SLOW).await?;
    if out.code != Some(0) {
        let tail = tokio::fs::read_to_string(&log).await.unwrap_or_default();
        let tail: Vec<&str> = tail.lines().rev().take(5).collect();
        let mut err = failed("pg_ctl start", &out);
        if let CoreError::Command { stderr, .. } = &mut err {
            for line in tail.into_iter().rev() {
                stderr.push('\n');
                stderr.push_str(line);
            }
        }
        return Err(err);
    }
    Ok(StartOutcome::Started)
}

/// Fast shutdown; a stopped cluster is fine.
pub async fn stop(inst: &Instance) -> Result<()> {
    if !matches!(status(inst).await?, PgCtlStatus::Running { .. }) {
        return Ok(());
    }
    let args = [
        "stop".as_ref(),
        "-w".as_ref(),
        "-m".as_ref(),
        "fast".as_ref(),
        "-D".as_ref(),
        inst.data_dir.as_os_str(),
    ];
    let out = cmd::run(inst.bin_dir.join("pg_ctl"), args, SLOW).await?;
    if out.code != Some(0) {
        return Err(failed("pg_ctl stop", &out));
    }
    Ok(())
}

pub async fn restart(
    inst: &Instance,
    paths: &AppPaths,
    managed: Vec<(String, PathBuf)>,
    preflight: Preflight,
) -> Result<StartOutcome> {
    stop(inst).await?;
    start(inst, paths, managed, preflight).await
}

/// Streams lines appended to `path` (starting with the last 64 KiB) until the
/// receiver is dropped. Tolerates a missing file and truncation/rotation.
pub async fn tail_log(path: PathBuf, tx: UnboundedSender<String>) {
    const BACKLOG: u64 = 64 * 1024;
    let mut pos: Option<u64> = None;
    let mut partial = String::new();
    let mut buf = Vec::new();
    while !tx.is_closed() {
        if let Ok(mut file) = tokio::fs::File::open(&path).await
            && let Ok(meta) = file.metadata().await
        {
            let len = meta.len();
            let start = match pos {
                None => len.saturating_sub(BACKLOG),
                Some(p) if p > len => {
                    partial.clear();
                    0
                }
                Some(p) => p,
            };
            if start < len && file.seek(SeekFrom::Start(start)).await.is_ok() {
                buf.clear();
                if file.read_to_end(&mut buf).await.is_ok() {
                    partial.push_str(&String::from_utf8_lossy(&buf));
                    while let Some(i) = partial.find('\n') {
                        let line: String = partial.drain(..=i).collect();
                        if tx
                            .unbounded_send(line.trim_end_matches(['\n', '\r']).to_string())
                            .is_err()
                        {
                            return;
                        }
                    }
                }
                pos = Some(start + buf.len() as u64);
            } else {
                pos = Some(start.min(len));
            }
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::io::Write;

    #[tokio::test]
    async fn tail_log_emits_appended_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pg.log");
        std::fs::write(&path, "old line\n").unwrap();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let task = tokio::spawn(tail_log(path.clone(), tx));
        assert_eq!(rx.next().await.unwrap(), "old line");
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(f, "first\nsecond").unwrap();
        assert_eq!(rx.next().await.unwrap(), "first");
        assert_eq!(rx.next().await.unwrap(), "second");
        drop(rx);
        tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn tail_log_waits_for_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("later.log");
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        tokio::spawn(tail_log(path.clone(), tx));
        tokio::time::sleep(Duration::from_millis(300)).await;
        std::fs::write(&path, "hello\n").unwrap();
        let line = tokio::time::timeout(Duration::from_secs(2), rx.next())
            .await
            .unwrap();
        assert_eq!(line.as_deref(), Some("hello"));
    }

    #[tokio::test]
    async fn tail_log_restarts_after_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pg.log");
        std::fs::write(&path, "a long first line\n").unwrap();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        tokio::spawn(tail_log(path.clone(), tx));
        assert_eq!(rx.next().await.unwrap(), "a long first line");
        std::fs::write(&path, "new\n").unwrap();
        let line = tokio::time::timeout(Duration::from_secs(2), rx.next())
            .await
            .unwrap();
        assert_eq!(line.as_deref(), Some("new"));
    }

    #[test]
    fn postgres_options_are_shell_quoted() {
        let opts = postgres_options(5440, ListenScope::All, Path::new("/a b/run")).unwrap();
        assert_eq!(opts, "-p 5440 -c listen_addresses='*' -k '/a b/run'");
    }

    #[test]
    fn shell_metacharacters_in_paths_rejected() {
        for bad in ["/a\"b", "/pg$data", "/x`id`", "/a\\b"] {
            assert!(check_shell_safe_path(Path::new(bad)).is_err(), "{bad}");
        }
        assert!(
            check_shell_safe_path(Path::new(
                "/Users/u/Library/Application Support/sawit/clusters/a b"
            ))
            .is_ok()
        );
    }

    #[test]
    fn single_quote_in_socket_dir_rejected() {
        assert!(postgres_options(5440, ListenScope::Localhost, Path::new("/it's/run")).is_err());
    }
}
