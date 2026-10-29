use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("`{program}` failed (exit {code:?}): {stderr}")]
    Command {
        program: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("{0}")]
    Parse(String),
    #[error("{0}")]
    Config(String),
    #[error(transparent)]
    Db(#[from] tokio_postgres::Error),
    /// A server-side SQL error, already formatted with detail/hint/position.
    #[error("{0}")]
    Sql(String),
    #[error("Unable to download. {0}")]
    Download(String),
    /// A formatter is missing or rejected the input; already user-facing.
    #[error("{0}")]
    Format(String),
    /// Git refused; its own message, already user-facing.
    #[error("{0}")]
    Git(String),
    /// GitHub refused or couldn't be reached; already user-facing.
    #[error("{0}")]
    GitHub(String),
    /// A network check failed; already user-facing.
    #[error("{0}")]
    Network(String),
    /// Opening a database connection failed, sorted by what to do about it.
    #[error("{message}")]
    Connect {
        kind: crate::db::FailureKind,
        message: String,
    },
}

pub type Result<T> = std::result::Result<T, CoreError>;
