//! Small helper for running external programs asynchronously.

use std::ffi::OsStr;
use std::time::Duration;

use crate::{CoreError, Result};

pub(crate) struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn combined(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

/// Runs `program` with `args`, failing only if it cannot be spawned or exceeds `timeout`.
pub(crate) async fn run<I, S>(
    program: impl AsRef<OsStr>,
    args: I,
    timeout: Duration,
) -> Result<Output>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let program = program.as_ref();
    let child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = tokio::time::timeout(timeout, child)
        .await
        .map_err(|_| CoreError::Command {
            program: program.to_string_lossy().into_owned(),
            code: None,
            stderr: format!("timed out after {timeout:?}"),
        })??;
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}
