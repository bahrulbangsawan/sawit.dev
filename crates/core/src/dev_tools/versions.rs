//! Latest releases: each tool's newest version, comparing versions, and what an upgrade runs.

use std::collections::HashMap;
use std::time::Duration;

use super::detect::Found;
use super::{Latest, TOOLS, Tool};

/// The version Homebrew's API gives a cask
/// (`formulae.brew.sh/api/cask/<token>.json`): "3.2.0", the part before a
/// build number ("1.2.3,456" → "1.2.3"); `None` for `latest`, which has none.
pub(super) fn cask_version(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let version = v["version"].as_str()?.split(',').next()?.trim();
    version
        .starts_with(|c: char| c.is_ascii_digit())
        .then(|| version.to_string())
}

/// The version in a release tag ("v1.2.3", "bun-v1.2.3" → "1.2.3").
pub(super) fn tag_version(tag: &str) -> Option<String> {
    let start = tag.find(|c: char| c.is_ascii_digit())?;
    Some(tag[start..].trim().to_string())
}

/// `tool`'s latest released version, or `None` when it can't be read.
async fn latest_version(tool: &Tool) -> Option<String> {
    let (args, url): (&[&str], String) = match tool.latest {
        Latest::None => return None,
        Latest::Npm(pkg) => (
            &["-fsSL"],
            format!("https://registry.npmjs.org/{pkg}/latest"),
        ),
        // The release page redirects to its tag; no rate-limited API call.
        Latest::GitHub(repo) => (
            &["-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}"],
            format!("https://github.com/{repo}/releases/latest"),
        ),
        Latest::Cask(token) => (
            &["-fsSL"],
            format!("https://formulae.brew.sh/api/cask/{token}.json"),
        ),
    };
    let out = crate::cmd::run(
        "/usr/bin/curl",
        args.iter()
            .copied()
            .chain(["--max-time", "8", url.as_str()]),
        Duration::from_secs(10),
    )
    .await
    .ok()?;
    if out.code != Some(0) {
        return None;
    }
    match tool.latest {
        Latest::Npm(_) => {
            let v: serde_json::Value = serde_json::from_str(&out.stdout).ok()?;
            Some(v["version"].as_str()?.to_string())
        }
        Latest::Cask(_) => cask_version(&out.stdout),
        _ => tag_version(out.stdout.trim().rsplit_once("/tag/")?.1),
    }
}

/// The latest released version of every tool that publishes one, fetched
/// at once; tools that fail are left out.
pub async fn latest_versions() -> HashMap<&'static str, String> {
    futures::future::join_all(
        TOOLS
            .iter()
            .map(|t| async move { Some((t.id, latest_version(t).await?)) }),
    )
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// The newer release installed tool `id` can upgrade to. None for an app
/// whose copy its upgrade command doesn't upgrade ([`Found::outside`]): its
/// version shows, but nothing offers to upgrade it.
pub fn newer<'a>(
    id: &str,
    found: &HashMap<&'static str, Found>,
    latest: &'a HashMap<&'static str, String>,
) -> Option<&'a str> {
    let found = found.get(id)?;
    if found.outside.is_some() {
        return None;
    }
    let installed = found.version.as_ref()?;
    let latest = latest.get(id)?;
    is_newer(latest, installed).then_some(latest.as_str())
}

/// The tools upgrading `wanted` runs, in catalog order: every installed
/// tool with a newer release when `wanted` is empty, else each wanted tool
/// that is installed and not known to be up to date. Never an app whose
/// copy its upgrade command doesn't upgrade ([`Found::outside`]), named or
/// not: the page, `upgrade_dev_tools` and `sawit pupuk` all leave it alone.
pub fn upgrade_plan(
    wanted: &[&str],
    found: &HashMap<&'static str, Found>,
    latest: &HashMap<&'static str, String>,
) -> Vec<&'static Tool> {
    TOOLS
        .iter()
        .filter(|t| found.get(t.id).is_some_and(|f| f.outside.is_none()))
        .filter(|t| {
            if wanted.is_empty() {
                newer(t.id, found, latest).is_some()
            } else {
                wanted.contains(&t.id)
                    && (newer(t.id, found, latest).is_some() || !latest.contains_key(t.id))
            }
        })
        .collect()
}

/// `latest` is a higher dotted version than `installed`. Pre-release and
/// build suffixes ("-beta.1", "+abc") are ignored; missing parts count as 0.
pub fn is_newer(latest: &str, installed: &str) -> bool {
    fn parts(v: &str) -> Vec<u64> {
        let core = v.trim().trim_start_matches('v');
        let core = core.split(['-', '+', ' ']).next().unwrap_or_default();
        core.split('.')
            .map_while(|p| {
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().ok()
            })
            .collect()
    }
    let (a, b) = (parts(latest), parts(installed));
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let n = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..n)
        .map(|i| at(&a, i).cmp(&at(&b, i)))
        .find(|o| o.is_ne())
        .is_some_and(|o| o.is_gt())
}
