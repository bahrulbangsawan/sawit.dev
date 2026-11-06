//! What some tools' rows say and do besides install, upgrade and uninstall: whether the GitHub
//! CLI is signed in.

use std::path::Path;
use std::time::Duration;

/// The GitHub CLI's sign-in, from `gh auth status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhAuth {
    /// Signed in to github.com, as this login when gh says.
    SignedIn(Option<String>),
    SignedOut,
}

/// Reads `gh auth status` for github.com with the `gh` at `gh`. It prints the token masked
/// (`gho_****`), and nothing of it is kept. `None` when gh couldn't be asked.
pub async fn gh_auth(gh: &Path) -> Option<GhAuth> {
    let out = crate::cmd::run(
        gh,
        ["auth", "status", "--hostname", "github.com"],
        Duration::from_secs(8),
    )
    .await
    .ok()?;
    Some(parse_gh_auth(
        out.code,
        &format!("{}\n{}", out.stdout, out.stderr),
    ))
}

/// What `gh auth status` said: exit 0 is signed in, and the login is in "Logged in to
/// github.com account octocat (keyring)" (gh 2.40 and later) or "Logged in to github.com as
/// octocat" (older).
pub fn parse_gh_auth(code: Option<i32>, text: &str) -> GhAuth {
    if code != Some(0) {
        return GhAuth::SignedOut;
    }
    let login = text.lines().find_map(|line| {
        let rest = line.split_once("Logged in to github.com")?.1.trim_start();
        let rest = rest
            .strip_prefix("account ")
            .or_else(|| rest.strip_prefix("as "))?;
        let login: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        (!login.is_empty()).then_some(login)
    });
    GhAuth::SignedIn(login)
}
