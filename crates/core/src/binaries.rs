//! Discovers installed PostgreSQL binaries (Homebrew, Postgres.app, PATH and
//! Sawit's own managed downloads) without hardcoding install paths.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cmd;
use crate::probe::brew_prefix;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PgVersion {
    pub major: u32,
    pub minor: u32,
}

impl std::fmt::Display for PgVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Parses `pg_ctl --version` / `postgres --version` output.
pub fn parse_version(output: &str) -> Option<PgVersion> {
    let rest = output.split_once("(PostgreSQL) ")?.1;
    let token = rest.split_whitespace().next()?;
    let mut parts = token.split('.');
    let digits = |s: &str| {
        s.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
    };
    let major = digits(parts.next()?).parse().ok()?;
    let minor = parts
        .next()
        .map(|m| digits(m).parse().unwrap_or(0))
        .unwrap_or(0);
    Some(PgVersion { major, minor })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinarySource {
    Managed,
    Homebrew,
    PostgresApp,
    Path,
}

impl std::fmt::Display for BinarySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BinarySource::Managed => "Sawit",
            BinarySource::Homebrew => "Homebrew",
            BinarySource::PostgresApp => "Postgres.app",
            BinarySource::Path => "PATH",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgBinaries {
    pub bin_dir: PathBuf,
    pub version: PgVersion,
    pub source: BinarySource,
}

impl PgBinaries {
    pub fn tool(&self, name: &str) -> PathBuf {
        self.bin_dir.join(name)
    }
}

#[derive(Debug, Clone)]
pub struct SearchRoots {
    pub brew_prefix: Option<PathBuf>,
    pub postgres_app: PathBuf,
    pub path_dirs: Vec<PathBuf>,
    pub managed: PathBuf,
}

impl SearchRoots {
    pub async fn detect(managed: PathBuf) -> SearchRoots {
        SearchRoots {
            brew_prefix: brew_prefix().await,
            postgres_app: "/Applications/Postgres.app".into(),
            path_dirs: std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default(),
            managed,
        }
    }
}

/// Subdirectories of `dir` matching `filter`, sorted by name.
fn subdirs(dir: &Path, filter: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return vec![];
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_str().is_some_and(&filter))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    dirs
}

/// Directories that contain both `pg_ctl` and `initdb`, deduplicated by
/// the canonical path of their `pg_ctl`. Earlier sources win: managed, Homebrew, Postgres.app, PATH.
pub fn candidate_bin_dirs(roots: &SearchRoots) -> Vec<(PathBuf, BinarySource)> {
    let mut candidates = Vec::new();
    for d in subdirs(&roots.managed, |n| !n.starts_with('.')) {
        candidates.push((d.join("bin"), BinarySource::Managed));
    }
    if let Some(brew) = &roots.brew_prefix {
        for d in subdirs(&brew.join("opt"), |n| n.starts_with("postgresql")) {
            candidates.push((d.join("bin"), BinarySource::Homebrew));
        }
    }
    for d in subdirs(&roots.postgres_app.join("Contents/Versions"), |n| {
        n != "latest"
    }) {
        candidates.push((d.join("bin"), BinarySource::PostgresApp));
    }
    candidates.extend(
        roots
            .path_dirs
            .iter()
            .map(|d| (d.clone(), BinarySource::Path)),
    );

    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|(dir, _)| dir.join("pg_ctl").is_file() && dir.join("initdb").is_file())
        // Key on the real pg_ctl file: PATH dirs like /opt/homebrew/bin hold
        // per-file symlinks into the Cellar, so the directories differ.
        .filter(|(dir, _)| {
            let pg_ctl = dir.join("pg_ctl");
            seen.insert(pg_ctl.canonicalize().unwrap_or(pg_ctl))
        })
        .collect()
}

/// Runs `pg_ctl --version` in each candidate; newest version first.
pub async fn discover(roots: &SearchRoots) -> Vec<PgBinaries> {
    let mut found = Vec::new();
    for (bin_dir, source) in candidate_bin_dirs(roots) {
        let Ok(out) = cmd::run(
            bin_dir.join("pg_ctl"),
            ["--version"],
            Duration::from_secs(5),
        )
        .await
        else {
            continue;
        };
        if let Some(version) = parse_version(&out.stdout) {
            found.push(PgBinaries {
                bin_dir,
                version,
                source,
            });
        }
    }
    found.sort_by_key(|b| std::cmp::Reverse(b.version));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_homebrew_version() {
        assert_eq!(
            parse_version("pg_ctl (PostgreSQL) 16.14 (Homebrew)\n"),
            Some(PgVersion {
                major: 16,
                minor: 14
            })
        );
    }

    #[test]
    fn parses_plain_version() {
        assert_eq!(
            parse_version("pg_ctl (PostgreSQL) 18.0"),
            Some(PgVersion {
                major: 18,
                minor: 0
            })
        );
    }

    #[test]
    fn parses_beta_version() {
        assert_eq!(
            parse_version("pg_ctl (PostgreSQL) 19beta1"),
            Some(PgVersion {
                major: 19,
                minor: 0
            })
        );
    }

    #[test]
    fn garbage_version_is_none() {
        assert_eq!(parse_version("command not found"), None);
    }

    fn fake_bin(dir: &Path, with_initdb: bool) {
        fs::create_dir_all(dir).unwrap();
        fs::write(dir.join("pg_ctl"), "").unwrap();
        if with_initdb {
            fs::write(dir.join("initdb"), "").unwrap();
        }
    }

    #[test]
    fn candidate_dirs_cover_all_sources_without_duplicates() {
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        fake_bin(&r.join("brew/opt/postgresql@16/bin"), true);
        fake_bin(&r.join("brew/opt/postgresql@17/bin"), true);
        fake_bin(&r.join("brew/opt/postgresql@15/bin"), false); // no initdb: ignored
        fake_bin(&r.join("Postgres.app/Contents/Versions/18/bin"), true);
        fake_bin(&r.join("managed/16.4.0/bin"), true);
        fs::create_dir_all(r.join("pathdir")).unwrap();
        std::os::unix::fs::symlink(r.join("brew/opt/postgresql@16/bin"), r.join("pathdir/pg16"))
            .unwrap();

        let roots = SearchRoots {
            brew_prefix: Some(r.join("brew")),
            postgres_app: r.join("Postgres.app"),
            path_dirs: vec![r.join("pathdir/pg16"), r.join("nonexistent")],
            managed: r.join("managed"),
        };
        let found = candidate_bin_dirs(&roots);
        let sources: Vec<_> = found.iter().map(|(_, s)| *s).collect();
        assert_eq!(found.len(), 4, "{found:?}");
        assert_eq!(
            sources,
            vec![
                BinarySource::Managed,
                BinarySource::Homebrew,
                BinarySource::Homebrew,
                BinarySource::PostgresApp
            ]
        );
    }

    #[test]
    fn path_dir_with_symlinked_tools_is_deduplicated() {
        // Homebrew's /opt/homebrew/bin holds symlinks to the Cellar binaries.
        let root = tempfile::tempdir().unwrap();
        let r = root.path();
        let real = r.join("brew/Cellar/postgresql@16/16.14/bin");
        fake_bin(&real, true);
        fs::create_dir_all(r.join("brew/opt/postgresql@16")).unwrap();
        std::os::unix::fs::symlink(&real, r.join("brew/opt/postgresql@16/bin")).unwrap();
        fs::create_dir_all(r.join("brew/bin")).unwrap();
        for tool in ["pg_ctl", "initdb"] {
            std::os::unix::fs::symlink(real.join(tool), r.join("brew/bin").join(tool)).unwrap();
        }
        let roots = SearchRoots {
            brew_prefix: Some(r.join("brew")),
            postgres_app: r.join("none"),
            path_dirs: vec![r.join("brew/bin")],
            managed: r.join("none"),
        };
        let found = candidate_bin_dirs(&roots);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].1, BinarySource::Homebrew);
    }

    #[tokio::test]
    #[ignore = "needs a real PostgreSQL install (Homebrew on the dev machine)"]
    async fn discover_finds_real_homebrew() {
        let roots = SearchRoots::detect(std::env::temp_dir().join("sawit-none")).await;
        let found = discover(&roots).await;
        assert!(
            found
                .iter()
                .any(|b| b.source == BinarySource::Homebrew && b.version.major == 16),
            "{found:?}"
        );
    }
}
