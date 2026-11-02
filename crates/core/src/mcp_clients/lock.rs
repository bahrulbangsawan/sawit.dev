//! One writer at a time per client config, across Sawit's processes.

use std::path::Path;

use super::cli::CLI_TIMEOUT;
use crate::{CoreError, Result};

/// An exclusive hold on one client config while it's read, changed and
/// written back, so two of Sawit's writers (the MCP page, an agent's
/// `install_mcp_server` in another process) never interleave and lose an
/// entry. The lock file is Sawit's own, in the temp folder, not one more
/// file beside the user's config. Dropping it lets go.
pub(super) struct ConfigLock(#[allow(dead_code)] std::fs::File);

fn lock_file(config: &Path) -> Result<std::fs::File> {
    use std::os::unix::ffi::OsStrExt as _;
    let key = std::fs::canonicalize(config)
        .or_else(|_| {
            let dir = std::fs::canonicalize(config.parent().unwrap_or(Path::new(".")))?;
            Ok::<_, std::io::Error>(dir.join(config.file_name().unwrap_or_default()))
        })
        .unwrap_or_else(|_| config.to_path_buf());
    // FNV-1a: stable across processes and builds.
    let hash = key
        .as_os_str()
        .as_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
    let dir = std::env::temp_dir().join("sawit-config-locks");
    std::fs::create_dir_all(&dir)?;
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join(format!("{hash:016x}.lock")))?)
}

/// Waits for the config's lock: for file edits, which take milliseconds.
pub(super) fn lock_config(config: &Path) -> Result<ConfigLock> {
    use std::os::fd::AsRawFd as _;
    let file = lock_file(config)?;
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(ConfigLock(file))
}

/// Waits for the config's lock without blocking a worker: a CLI update
/// holds it for as long as the client's CLI runs.
pub(super) async fn lock_config_async(config: &Path) -> Result<ConfigLock> {
    use std::os::fd::AsRawFd as _;
    let file = lock_file(config)?;
    let deadline = std::time::Instant::now() + 2 * CLI_TIMEOUT + std::time::Duration::from_secs(5);
    loop {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(ConfigLock(file));
        }
        let e = std::io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EWOULDBLOCK) {
            return Err(e.into());
        }
        if std::time::Instant::now() > deadline {
            return Err(CoreError::Config(format!(
                "Another change to {} is still running. Try again in a moment.",
                config.display()
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
