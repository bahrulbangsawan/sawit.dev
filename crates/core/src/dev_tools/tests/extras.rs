//! The extras some rows show: the GitHub CLI's sign-in.

use crate::dev_tools::extras::{GhAuth, parse_gh_auth};

/// `gh auth status` exits 0 when signed in, with the login in its new and old wording, and 1
/// when not.
#[test]
fn the_github_clis_sign_in_is_read_from_gh_auth_status() {
    let new = "github.com\n  ✓ Logged in to github.com account octo-cat (keyring)\n  - Active account: true\n  - Token: gho_************************************\n";
    assert_eq!(
        parse_gh_auth(Some(0), new),
        GhAuth::SignedIn(Some("octo-cat".into()))
    );
    let old = "github.com\n  ✓ Logged in to github.com as hubot (oauth_token)\n";
    assert_eq!(
        parse_gh_auth(Some(0), old),
        GhAuth::SignedIn(Some("hubot".into()))
    );
    assert_eq!(
        parse_gh_auth(Some(0), "github.com\n"),
        GhAuth::SignedIn(None)
    );
    let out = "You are not logged into any GitHub hosts. To log in, run: gh auth login\n";
    assert_eq!(parse_gh_auth(Some(1), out), GhAuth::SignedOut);
    assert_eq!(parse_gh_auth(None, new), GhAuth::SignedOut);
}
