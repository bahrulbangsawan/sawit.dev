//! Parsing of `pg_ctl status` output.

use crate::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PgCtlStatus {
    Running { pid: u32 },
    Stopped,
    NotACluster,
    MissingDir,
}

/// Interprets `pg_ctl status` from its exit code and combined stdout+stderr.
///
/// Exit codes: 0 running, 3 not running, 4 no accessible data directory.
pub fn parse_status(exit_code: i32, output: &str) -> Result<PgCtlStatus> {
    let unexpected = || {
        CoreError::Parse(format!(
            "unexpected pg_ctl status (exit {exit_code}): {output}"
        ))
    };
    match exit_code {
        0 => {
            let pid = output
                .split_once("(PID: ")
                .and_then(|(_, rest)| rest.split_once(')'))
                .and_then(|(pid, _)| pid.trim().parse().ok())
                .ok_or_else(unexpected)?;
            Ok(PgCtlStatus::Running { pid })
        }
        3 => Ok(PgCtlStatus::Stopped),
        4 if output.contains("is not a database cluster directory") => Ok(PgCtlStatus::NotACluster),
        4 if output.contains("does not exist") => Ok(PgCtlStatus::MissingDir),
        _ => Err(unexpected()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_with_pid() {
        let out = "pg_ctl: server is running (PID: 1234)\n/opt/x/bin/postgres \"-D\" \"/d\"\n";
        assert_eq!(
            parse_status(0, out).unwrap(),
            PgCtlStatus::Running { pid: 1234 }
        );
    }

    #[test]
    fn stopped() {
        assert_eq!(
            parse_status(3, "pg_ctl: no server running\n").unwrap(),
            PgCtlStatus::Stopped
        );
    }

    #[test]
    fn not_a_cluster() {
        let out = "pg_ctl: directory \"/x\" is not a database cluster directory\n";
        assert_eq!(parse_status(4, out).unwrap(), PgCtlStatus::NotACluster);
    }

    #[test]
    fn missing_dir() {
        let out = "pg_ctl: directory \"/x\" does not exist\n";
        assert_eq!(parse_status(4, out).unwrap(), PgCtlStatus::MissingDir);
    }

    #[test]
    fn garbage_is_error() {
        assert!(parse_status(0, "garbage").is_err());
    }
}
