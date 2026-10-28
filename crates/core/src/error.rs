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
    #[error("parse error: {0}")]
    Parse(String),
    #[error("configuration error: {0}")]
    Config(String),
    #[error(transparent)]
    Db(#[from] tokio_postgres::Error),
    #[error("download failed: {0}")]
    Download(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
