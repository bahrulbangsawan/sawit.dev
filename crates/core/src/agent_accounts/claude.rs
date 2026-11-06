//! Claude Code, the first agent with accounts: whether an account is signed in and as whom
//! (`claude auth status`, through a whitelist), its plan (the non-secret `oauthAccount` keys
//! Claude Code caches), and the environment it runs in.
//!
//! Nothing here reads a token: the sign-in is in Claude Code's Keychain item for the folder, and
//! only Claude Code's own commands touch it. A field whose name looks like a secret is dropped
//! even if it is on the list.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::Account;

/// The fields of `claude auth status --json` that are read. Everything else is dropped.
const WHITELIST: &[&str] = &[
    "loggedIn",
    "authMethod",
    "configDirectory",
    "email",
    "orgId",
    "orgName",
    "subscriptionType",
];
const FORBIDDEN: &[&str] = &["token", "key", "secret", "credential"];

/// Who an account is signed in as. Kept in memory only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AuthStatus {
    pub signed_in: bool,
    pub auth_method: Option<String>,
    pub config_directory: Option<String>,
    pub email: Option<String>,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    pub subscription_type: Option<String>,
}

impl AuthStatus {
    /// Usage windows exist only for claude.ai sign-ins.
    pub fn reports_usage(&self) -> bool {
        matches!(self.auth_method.as_deref(), None | Some("claude.ai"))
    }

    /// The same account and organisation as `other` (case-insensitive); `None` when unknown.
    pub fn same_account(&self, other: &AuthStatus) -> Option<bool> {
        let (a, b) = (self.email.as_ref()?, other.email.as_ref()?);
        if a.to_lowercase() != b.to_lowercase() {
            return Some(false);
        }
        match (&self.org_id, &other.org_id) {
            (Some(x), Some(y)) => Some(x == y),
            _ => Some(true),
        }
    }

    /// "person@example.com · Example Org".
    pub fn identity(&self) -> Option<String> {
        let parts: Vec<&str> = [self.email.as_deref(), self.org_name.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        (!parts.is_empty()).then(|| parts.join(" · "))
    }
}

fn allowed(name: &str) -> bool {
    let lower = name.to_lowercase();
    WHITELIST.contains(&name) && !FORBIDDEN.iter().any(|f| lower.contains(f))
}

/// The whitelisted fields of `claude auth status` output, as text.
pub fn allowed_fields(stdout: &str) -> BTreeMap<String, String> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(stdout) else {
        return BTreeMap::new();
    };
    map.into_iter()
        .filter(|(k, _)| allowed(k))
        .filter_map(|(k, v)| match v {
            Value::String(s) => Some((k, s.chars().take(256).collect())),
            Value::Bool(b) => Some((k, b.to_string())),
            _ => None,
        })
        .collect()
}

/// `claude auth status` by its exit code (0 signed in, 1 signed out; else `None`) and output.
pub fn parse_auth_status(stdout: &str, exit_code: i32) -> Option<AuthStatus> {
    let signed_in = match exit_code {
        0 => true,
        1 => false,
        _ => return None,
    };
    let mut f = allowed_fields(stdout);
    let mut take = |k: &str| f.remove(k).filter(|v| !v.is_empty());
    Some(AuthStatus {
        signed_in,
        auth_method: take("authMethod"),
        config_directory: take("configDirectory"),
        email: take("email"),
        org_id: take("orgId"),
        org_name: take("orgName"),
        subscription_type: take("subscriptionType"),
    })
}

/// The non-secret account facts Claude Code caches under `oauthAccount`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDetails {
    pub email_address: Option<String>,
    pub organization_name: Option<String>,
    pub organization_type: Option<String>,
    pub organization_rate_limit_tier: Option<String>,
    pub user_rate_limit_tier: Option<String>,
    pub seat_tier: Option<String>,
    pub plan_display_name: Option<String>,
}

impl AccountDetails {
    /// The user's tier wins over the organisation's, as Claude Code applies them.
    pub fn rate_limit_tier(&self) -> Option<&str> {
        self.user_rate_limit_tier
            .as_deref()
            .or(self.organization_rate_limit_tier.as_deref())
    }
}

/// The global config Claude Code keeps for `account`: `~/.claude.json` for the default,
/// `<dir>/.claude.json` otherwise.
pub fn global_config(account: &Account, home: &Path) -> PathBuf {
    if account.is_default {
        home.join(".claude.json")
    } else {
        account.dir.join(".claude.json")
    }
}

/// Only `oauthAccount`'s declared keys are decoded; nothing else in the file is read.
pub fn parse_details(text: &str) -> Option<AccountDetails> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct File {
        oauth_account: Option<AccountDetails>,
    }
    let mut d = serde_json::from_str::<File>(text).ok()?.oauth_account?;
    for field in [
        &mut d.email_address,
        &mut d.organization_name,
        &mut d.organization_type,
        &mut d.organization_rate_limit_tier,
        &mut d.user_rate_limit_tier,
        &mut d.seat_tier,
        &mut d.plan_display_name,
    ] {
        *field = field
            .take()
            .map(|v| v.trim().chars().take(128).collect::<String>())
            .filter(|v| !v.is_empty());
    }
    Some(d)
}

/// The plan, as specific as the data allows: a tier ("20x") only when Claude Code's account data
/// names it, never guessed. Details for another email than the signed-in one are ignored.
pub fn plan(
    subscription_type: Option<&str>,
    details: Option<&AccountDetails>,
    signed_in_email: Option<&str>,
) -> Option<String> {
    let details = details.filter(|d| match (signed_in_email, d.email_address.as_deref()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => true,
    });
    let org_type = details.and_then(|d| d.organization_type.as_deref());
    let family = family(subscription_type, org_type);
    if family.is_none() && subscription_type.is_none() {
        return None;
    }
    let seat = details
        .and_then(|d| d.seat_tier.as_deref())
        .map(|s| title_case(&s.replace('_', " ")));
    if let Some(name) = details.and_then(|d| d.plan_display_name.as_deref())
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || " .·-x+()".contains(c))
    {
        return Some(name.to_string());
    }
    let tier = details.and_then(|d| d.rate_limit_tier());
    Some(match family {
        Some("Max") => match multiplier(tier).filter(|_| tier.is_some_and(|t| t.contains("max"))) {
            Some(n) => format!("Max {n}x"),
            None => "Max · Unknown Tier".into(),
        },
        Some(f @ ("Team" | "Enterprise")) => match seat {
            Some(seat) => format!("{f} · {seat} seat"),
            None => f.into(),
        },
        Some(f) => f.into(),
        None => title_case(
            &subscription_type
                .unwrap_or("Unknown plan")
                .replace('_', " "),
        ),
    })
}

fn family(subscription_type: Option<&str>, org_type: Option<&str>) -> Option<&'static str> {
    let of = |s: &str| match s.to_lowercase().trim_start_matches("claude_") {
        "pro" => Some("Pro"),
        "max" => Some("Max"),
        "team" => Some("Team"),
        "enterprise" => Some("Enterprise"),
        _ => None,
    };
    subscription_type
        .and_then(of)
        .or_else(|| org_type.and_then(of))
}

/// `20` from `default_claude_max_20x`.
fn multiplier(tier: Option<&str>) -> Option<u32> {
    let tier = tier?;
    tier.split('_')
        .filter_map(|p| p.strip_suffix('x'))
        .find_map(|n| n.parse().ok())
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The environment `claude` runs with for `account`, built from nothing: HOME, USER, LOGNAME
/// (without them Claude Code can't find its Keychain sign-in and says "signed out"), PATH, and
/// `CLAUDE_CONFIG_DIR` only for an account that isn't the default.
pub fn environment(
    account: &Account,
    home: &Path,
    user: &str,
    path: &str,
) -> Vec<(String, String)> {
    let mut env = vec![
        ("HOME".into(), home.to_string_lossy().to_string()),
        ("USER".into(), user.into()),
        ("LOGNAME".into(), user.into()),
        ("PATH".into(), path.into()),
    ];
    if let Some(dir) = account.env_value() {
        env.push((
            account.kind.profile_var().into(),
            dir.to_string_lossy().to_string(),
        ));
    }
    env
}

/// Variables that would override every account's sign-in when exported globally. Their names
/// only are ever reported, never their values.
pub const OVERRIDING_VARS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CONFIG_DIR",
];
