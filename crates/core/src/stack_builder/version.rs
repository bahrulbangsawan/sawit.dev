//! Which `create-better-t-stack` the New Project page runs: the one Sawit's data was ported from
//! ([`CLI_VERSION`]), or a newer one the user chose with Upgrade; and the newest one on npm,
//! asked at most once a day.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::cli_help::{self, OptionsDiff};
use super::{CLI_VERSION, Category, Pm, Stack};
use crate::instances::AppPaths;

/// npm's answer for the CLI's `latest` tag.
pub const LATEST_URL: &str = "https://registry.npmjs.org/create-better-t-stack/latest";

/// Where the CLI's release notes are.
pub const RELEASES_URL: &str = "https://github.com/AmanVarshney01/create-better-t-stack/releases";

/// Test hooks (the e2e specs): a version to take as npm's latest, and a file whose text to take
/// as a release's `create --help`, instead of asking npm and running the CLI.
pub const LATEST_ENV: &str = "SAWIT_STACK_CLI_LATEST";
pub const HELP_ENV: &str = "SAWIT_STACK_CLI_HELP";

/// How long a lookup of the latest version is kept.
const CHECK_EVERY: i64 = 24 * 60 * 60;

/// The version a `x.y.z` string is, pre-releases and build tags left out: `None` for one.
pub fn stable(version: &str) -> Option<(u64, u64, u64)> {
    let v = version.trim().trim_start_matches('v');
    if v.contains(['-', '+']) {
        return None;
    }
    let mut parts = v.split('.').map(|p| p.parse::<u64>().ok());
    let version = (parts.next()??, parts.next()??, parts.next()??);
    parts.next().is_none().then_some(version)
}

/// Whether `latest` is a stable release newer than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (stable(latest), stable(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// What the page keeps about the CLI's versions: the one the user chose (none: the pinned one),
/// and the last lookup of the latest.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliVersions {
    /// The version the user upgraded to; `None` runs [`CLI_VERSION`].
    #[serde(default)]
    pub chosen: Option<String>,
    /// The latest version npm gave, and when (Unix seconds).
    #[serde(default)]
    pub latest: Option<String>,
    #[serde(default)]
    pub checked_at: Option<i64>,
    /// What the chosen version takes that the page doesn't offer, and the other way round:
    /// the page hides what is gone.
    #[serde(default)]
    pub diff: Option<OptionsDiff>,
}

impl CliVersions {
    pub fn file(paths: &AppPaths) -> PathBuf {
        paths.root.join("new-project-cli.json")
    }

    /// What was kept, with a chosen version that the pinned one has caught up with dropped:
    /// a Sawit release that ports a newer CLI moves the default.
    pub fn load(paths: &AppPaths) -> Self {
        let mut v: Self = std::fs::read_to_string(Self::file(paths))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        if v.chosen
            .as_deref()
            .is_some_and(|c| !is_newer(c, CLI_VERSION))
        {
            v.chosen = None;
            v.diff = None;
        }
        v
    }

    pub fn save(&self, paths: &AppPaths) -> crate::Result<()> {
        let file = Self::file(paths);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| crate::CoreError::Config(e.to_string()))?;
        crate::files::write_text_atomic(&file, &format!("{text}\n"))
    }

    /// The version Create Project runs.
    pub fn in_use(&self) -> &str {
        self.chosen.as_deref().unwrap_or(CLI_VERSION)
    }

    /// The newer release the page offers to upgrade to, if npm has one.
    pub fn upgrade(&self) -> Option<&str> {
        self.latest
            .as_deref()
            .filter(|latest| is_newer(latest, self.in_use()))
    }

    /// Whether the page hides option `id` of `category`: the chosen version doesn't take it.
    pub fn hides(&self, category: Category, id: &'static str) -> bool {
        self.chosen.is_some() && self.diff.as_ref().is_some_and(|d| d.hides(category, id))
    }

    /// Why `stack` can't be created with the version in use: an option it picks that the
    /// chosen version no longer takes.
    pub fn refuses(&self, stack: &Stack) -> Option<String> {
        let gone: Vec<String> = Category::ALL
            .iter()
            .flat_map(|&c| {
                stack
                    .picked(c)
                    .into_iter()
                    .filter(move |id| self.hides(c, id))
                    .map(move |id| {
                        format!("--{} {}", cli_help::flag(c), cli_help::cli_value(c, id))
                    })
            })
            .collect();
        (!gone.is_empty()).then(|| {
            format!(
                "create-better-t-stack {} doesn't take {}. Pick another option, or go back to {CLI_VERSION} on the New Project page.",
                self.in_use(),
                gone.join(", ")
            )
        })
    }

    /// Whether the kept lookup is older than a day (or there is none).
    pub fn stale(&self, now: i64) -> bool {
        self.checked_at
            .is_none_or(|at| now - at >= CHECK_EVERY || now < at)
    }
}

/// The `version` in npm's answer for a package's tag.
pub fn npm_version(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v["version"].as_str()?.trim();
    stable(version).map(|_| version.to_string())
}

/// The latest stable `create-better-t-stack` on npm, over HTTPS in Rust; `None` offline or on
/// any failure (the page then shows nothing).
pub async fn fetch_latest() -> Option<String> {
    if let Some(latest) = std::env::var(LATEST_ENV)
        .ok()
        .filter(|v| stable(v).is_some())
    {
        return Some(latest);
    }
    let body =
        tokio::task::spawn_blocking(|| crate::https::get(LATEST_URL, Duration::from_secs(8)))
            .await
            .ok()?
            .ok()?;
    npm_version(&body)
}

/// The flags and choices release `version` takes, from its `create --help`, run once through
/// `pm`'s package runner in the login shell (`npx -y`, `pnpm dlx`, `bunx`).
///
/// Not Rust: the CLI publishes its options only in its help (and a schema it prints itself),
/// so the user's own package runner runs the release they are about to use, once, on Upgrade.
pub async fn release_options(pm: Pm, version: &str) -> Result<cli_help::Flags, String> {
    if stable(version).is_none() {
        return Err(format!("{version} isn't a release version."));
    }
    if let Some(file) = std::env::var_os(HELP_ENV) {
        let text = std::fs::read_to_string(&file).map_err(|e| e.to_string())?;
        return Ok(cli_help::parse_help(&text));
    }
    let package = format!("create-better-t-stack@{version}");
    let line = pm
        .package_manager()
        .run_line(&[package.as_str(), "create", "--help"]);
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let ran = crate::dev_tools::run_shell(
        "new-project-help",
        "create-better-t-stack",
        &line,
        move |l| sink.lock().unwrap_or_else(|e| e.into_inner()).push(l),
    )
    .await;
    let text = lines.lock().unwrap_or_else(|e| e.into_inner()).join("\n");
    let flags = cli_help::parse_help(&text);
    match ran {
        Ok(()) if flags.len() > 5 => Ok(flags),
        Ok(()) => Err(format!(
            "create-better-t-stack {version} printed no options Sawit can read."
        )),
        Err(e) => Err(format!("create-better-t-stack {version} didn't run: {e}")),
    }
}

/// Upgrade: what `version` takes, compared with what the page offers, kept as the version in
/// use with that diff. Returns the kept versions.
pub async fn upgrade(paths: &AppPaths, pm: Pm, version: &str) -> Result<CliVersions, String> {
    let accepted = release_options(pm, version).await?;
    let diff = cli_help::diff(&cli_help::offered(), &cli_help::pinned(), &accepted);
    let mut v = CliVersions::load(paths);
    v.chosen = Some(version.to_string());
    v.diff = Some(diff);
    v.save(paths).map_err(|e| e.to_string())?;
    Ok(v)
}

/// Go back to the pinned version.
pub fn go_back(paths: &AppPaths) -> Result<CliVersions, String> {
    let mut v = CliVersions::load(paths);
    v.chosen = None;
    v.diff = None;
    v.save(paths).map_err(|e| e.to_string())?;
    Ok(v)
}

/// The kept versions, with the latest looked up again when the last lookup is a day old.
/// A failed lookup keeps what was there and is tried again next time.
pub async fn check(paths: &AppPaths, now: i64) -> CliVersions {
    let mut v = CliVersions::load(paths);
    if !v.stale(now) && std::env::var_os(LATEST_ENV).is_none() {
        return v;
    }
    if let Some(latest) = fetch_latest().await {
        v.latest = Some(latest);
        v.checked_at = Some(now);
        let _ = v.save(paths);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_and_pre_releases_are_ignored() {
        assert!(is_newer("3.50.0", "3.49.0"));
        assert!(is_newer("4.0.0", "3.49.9"));
        assert!(is_newer("3.49.10", "3.49.9"));
        assert!(!is_newer("3.49.0", "3.49.0"));
        assert!(!is_newer("3.48.2", "3.49.0"));
        assert!(!is_newer("3.50.0-beta.1", "3.49.0"));
        assert!(!is_newer("nonsense", "3.49.0"));
        assert_eq!(stable("v3.49.0"), Some((3, 49, 0)));
        assert_eq!(stable("3.49"), None);
        assert_eq!(
            npm_version(r#"{"name":"create-better-t-stack","version":"3.50.1"}"#).as_deref(),
            Some("3.50.1")
        );
        assert_eq!(npm_version(r#"{"version":"3.51.0-rc.1"}"#), None);
    }

    #[test]
    fn the_check_is_kept_a_day() {
        let v = CliVersions {
            latest: Some("3.50.0".into()),
            checked_at: Some(1_000),
            ..Default::default()
        };
        assert!(!v.stale(1_000 + CHECK_EVERY - 1));
        assert!(v.stale(1_000 + CHECK_EVERY));
        assert!(v.stale(999), "a clock set back looks again");
        assert!(CliVersions::default().stale(0));
    }

    #[test]
    fn the_chosen_version_is_used_until_the_pinned_one_catches_up() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            root: dir.path().to_path_buf(),
        };
        assert_eq!(CliVersions::load(&paths).in_use(), CLI_VERSION);
        let v = CliVersions {
            chosen: Some("99.0.0".into()),
            latest: Some("99.0.0".into()),
            checked_at: Some(1),
            diff: Some(OptionsDiff {
                new: vec![],
                gone: vec!["--orm mongoose".into()],
            }),
        };
        v.save(&paths).unwrap();
        let loaded = CliVersions::load(&paths);
        assert_eq!(loaded.in_use(), "99.0.0");
        assert_eq!(loaded.upgrade(), None, "nothing newer than the chosen one");
        assert!(loaded.hides(Category::Orm, "mongoose"));
        let mongo = Stack {
            orm: super::super::Orm::Mongoose,
            ..Stack::default()
        };
        let why = loaded.refuses(&mongo).expect("refused");
        assert!(why.contains("--orm mongoose"), "{why}");
        assert_eq!(loaded.refuses(&Stack::default()), None);
        let back = go_back(&paths).unwrap();
        assert_eq!(back.in_use(), CLI_VERSION);
        assert!(!back.hides(Category::Orm, "mongoose"));
        let v = CliVersions {
            chosen: Some("99.0.0".into()),
            ..Default::default()
        };
        // A chosen version the pinned one has reached is dropped: the default moved past it.
        let old = CliVersions {
            chosen: Some(CLI_VERSION.into()),
            ..v
        };
        old.save(&paths).unwrap();
        assert_eq!(CliVersions::load(&paths).chosen, None);
        let offer = CliVersions {
            latest: Some("99.1.0".into()),
            ..Default::default()
        };
        assert_eq!(offer.upgrade(), Some("99.1.0"));
    }
}
