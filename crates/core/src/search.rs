//! Text search across a folder: ripgrep when installed (it skips what
//! `.gitignore` ignores), else the system `grep`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cmd;
use crate::probe::find_in_path;
use crate::{CoreError, Result};

const TIMEOUT: Duration = Duration::from_secs(30);
/// Longest line text kept for a hit.
const MAX_TEXT: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    pub text: String,
}

/// Lines under `dir` containing `query` (case-insensitive unless it has
/// capitals), at most `limit`.
pub async fn find(dir: &Path, query: &str, limit: usize) -> Result<Vec<Hit>> {
    let smart_case = if query.chars().any(char::is_uppercase) {
        None
    } else {
        Some("-i")
    };
    let dir = dir.to_string_lossy().into_owned();
    let (program, args): (PathBuf, Vec<String>) = match find_in_path("rg") {
        Some(rg) => (
            rg,
            ["--line-number", "--no-heading", "--null", "--color=never", "--fixed-strings"]
                .into_iter()
                .chain(smart_case)
                .chain(["-e", query, "--", &dir])
                .map(String::from)
                .collect(),
        ),
        None => (
            "/usr/bin/grep".into(),
            ["-rnIF", "--null"]
                .into_iter()
                .chain(smart_case)
                .map(String::from)
                .chain(crate::files::HIDDEN.iter().map(|d| format!("--exclude-dir={d}")))
                .chain(["-e", query, "--", &dir].map(String::from))
                .collect(),
        ),
    };
    let out = cmd::run(&program, &args, TIMEOUT).await?;
    // Both exit 1 when nothing matched.
    match out.code {
        Some(0) | Some(1) => Ok(parse(&out.stdout, limit)),
        code => Err(CoreError::Command {
            program: program.to_string_lossy().into_owned(),
            code,
            stderr: out.stderr,
        }),
    }
}

/// Reads `path\0line:text` lines.
fn parse(raw: &str, limit: usize) -> Vec<Hit> {
    raw.lines()
        .filter_map(|l| {
            let (path, rest) = l.split_once('\0')?;
            let (line, text) = rest.split_once(':')?;
            let text = text.trim();
            Some(Hit {
                path: PathBuf::from(path),
                line: line.parse().ok()?,
                text: match text.char_indices().nth(MAX_TEXT) {
                    Some((end, _)) => format!("{}…", &text[..end]),
                    None => text.to_string(),
                },
            })
        })
        .take(limit)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_null_separated_hits() {
        let raw = "/p/a:b.rs\012:    let x = 1;\n/p/c.rs\03:fn main() {}\nnoise\n";
        assert_eq!(
            parse(raw, 10),
            vec![
                Hit {
                    path: "/p/a:b.rs".into(),
                    line: 12,
                    text: "let x = 1;".into()
                },
                Hit {
                    path: "/p/c.rs".into(),
                    line: 3,
                    text: "fn main() {}".into()
                },
            ]
        );
        assert_eq!(parse(raw, 1).len(), 1);
    }

    #[tokio::test]
    async fn finds_text_in_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\nNeedle here\n").unwrap();
        let hits = find(d.path(), "needle", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 2);
        assert!(find(d.path(), "absent", 10).await.unwrap().is_empty());
    }
}
