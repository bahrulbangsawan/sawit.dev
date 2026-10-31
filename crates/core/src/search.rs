//! Text search across a folder: ripgrep when installed (it skips what
//! `.gitignore` ignores), else the system `grep`.

use std::path::{Path, PathBuf};
use std::time::Duration;

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
            [
                "--line-number",
                "--no-heading",
                "--null",
                "--color=never",
                "--fixed-strings",
                // A minified line is kept to its start, not read whole.
                "--max-columns=500",
                "--max-columns-preview",
            ]
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
                .chain(
                    crate::files::HIDDEN
                        .iter()
                        .map(|d| format!("--exclude-dir={d}")),
                )
                .chain(["-e", query, "--", &dir].map(String::from))
                .collect(),
        ),
    };
    search(&program, &args, limit, TIMEOUT).await
}

/// Runs `program` (ripgrep or grep) with `args`, and reads its first `limit` hits as they come.
/// With `limit` of them it stops the search: what it would print after them isn't read (a
/// one-letter query over a big tree prints most of the tree).
async fn search(
    program: &Path,
    args: &[String],
    limit: usize,
    timeout: Duration,
) -> Result<Vec<Hit>> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, BufReader};

    let failed = |code, stderr| CoreError::Command {
        program: program.to_string_lossy().into_owned(),
        code,
        stderr,
    };
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err(failed(None, "its output can't be read".into()));
    };
    // Read alongside, so that a search with much to say there never waits on it.
    let errors = tokio::spawn(async move {
        let mut text = Vec::new();
        let _ = stderr.read_to_end(&mut text).await;
        String::from_utf8_lossy(&text).into_owned()
    });
    let read = async {
        let mut hits = Vec::new();
        let mut out = BufReader::new(stdout);
        let mut line = Vec::new();
        while hits.len() < limit {
            line.clear();
            if out.read_until(b'\n', &mut line).await? == 0 {
                break;
            }
            hits.extend(parse(&String::from_utf8_lossy(&line), 1));
        }
        if hits.len() >= limit {
            let _ = child.start_kill();
        }
        Ok::<_, std::io::Error>((hits, child.wait().await?))
    };
    let (hits, status) = tokio::time::timeout(timeout, read)
        .await
        .map_err(|_| failed(None, format!("timed out after {timeout:?}")))??;
    match status.code() {
        // Stopped at the limit.
        _ if hits.len() >= limit => Ok(hits),
        // Both exit 1 when nothing matched.
        Some(0) | Some(1) => Ok(hits),
        code => Err(failed(code, errors.await.unwrap_or_default())),
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
        let raw = "/p/a:b.rs\x0012:    let x = 1;\n/p/c.rs\x003:fn main() {}\nnoise\n";
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

    /// A search stops reading once it has `limit` hits, and stops the search (FILES-11). It used
    /// to read everything the search printed before keeping the first `limit` hits: a one-letter
    /// query over a big tree held every matching line in memory, and could end "timed out"
    /// without showing any. Here the search would print hits for ever.
    #[tokio::test]
    async fn a_search_stops_at_its_limit() {
        let endless = "i=1; while :; do printf 'f\\000%s:hit\\n' $i; i=$((i+1)); done";
        let args = ["-c".to_string(), endless.to_string()];
        let started = std::time::Instant::now();
        let hits = search(Path::new("/bin/sh"), &args, 50, Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(hits.len(), 50);
        assert_eq!((hits[49].line, hits[49].text.as_str()), (50, "hit"));
        assert!(started.elapsed() < Duration::from_secs(5));

        // One that ends early gives what it found, and one that fails says why.
        let sh = |script: &str| ["-c".to_string(), script.to_string()];
        let few = sh("printf 'a\\0001:x\\nb\\0002:y\\n'; exit 1");
        let hits = search(Path::new("/bin/sh"), &few, 50, Duration::from_secs(10));
        assert_eq!(hits.await.unwrap().len(), 2);
        let broken = sh("echo 'bad pattern' >&2; exit 2");
        let failed = search(Path::new("/bin/sh"), &broken, 50, Duration::from_secs(10));
        let failed = failed.await.unwrap_err();
        assert!(failed.to_string().contains("bad pattern"), "{failed}");
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
