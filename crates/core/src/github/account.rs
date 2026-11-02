//! The signed-in account, its picture, and its repositories.

use std::time::Duration;

use serde_json::Value;

use super::http::{api, api_reply};
use super::time::parse_time;
use crate::{CoreError, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    /// The picture GitHub shows for the account, on its avatars host.
    pub avatar_url: Option<String>,
    /// What the token may do, as GitHub says (`X-OAuth-Scopes`); `None` when it didn't say.
    pub scopes: Option<Vec<String>>,
}

pub fn parse_account(v: &Value, scopes: Option<Vec<String>>) -> Account {
    Account {
        login: v["login"].as_str().unwrap_or_default().to_string(),
        name: v["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .map(String::from),
        avatar_url: v["avatar_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .map(String::from),
        scopes,
    }
}

pub async fn account(token: &str) -> Result<Account> {
    let reply = api_reply("GET", "/user", token, None, "application/vnd.github+json").await?;
    let v: Value = serde_json::from_str(&reply.body).unwrap_or(Value::Null);
    Ok(parse_account(&v, reply.scopes))
}

/// The account's picture, small: a public address, so it's fetched without the token. Bytes of
/// a PNG or JPEG, which the caller sniffs.
pub async fn avatar(url: &str) -> Result<Vec<u8>> {
    let sized = if url.contains('?') {
        format!("{url}&s=64")
    } else {
        format!("{url}?s=64")
    };
    let out = tokio::time::timeout(
        Duration::from_secs(20),
        tokio::process::Command::new("/usr/bin/curl")
            .args(["-sSfL", "--max-time", "15", "--max-filesize", "1000000"])
            .arg(&sized)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| CoreError::GitHub("GitHub didn't send the avatar in time.".into()))??;
    if !out.status.success() || out.stdout.is_empty() {
        return Err(CoreError::GitHub("GitHub didn't send the avatar.".into()));
    }
    Ok(out.stdout)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub full_name: String,
    /// The account or organization it belongs to (`owner.login`).
    pub owner: String,
    pub description: Option<String>,
    pub private: bool,
    /// A fork of another repository.
    pub fork: bool,
    /// Archived on GitHub: read-only.
    pub archived: bool,
    /// The language GitHub found most of its code in.
    pub language: Option<String>,
    /// How many people starred it (`stargazers_count`).
    pub stars: u64,
    pub clone_url: String,
    /// Unix seconds of the last push.
    pub pushed_at: i64,
}

pub fn parse_repos(v: &Value) -> Vec<Repo> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let full_name = r["full_name"].as_str()?;
            Some(Repo {
                full_name: full_name.to_string(),
                // GitHub always says; else the name before the slash.
                owner: r["owner"]["login"]
                    .as_str()
                    .or_else(|| full_name.split_once('/').map(|(owner, _)| owner))
                    .unwrap_or_default()
                    .to_string(),
                description: r["description"]
                    .as_str()
                    .filter(|d| !d.is_empty())
                    .map(String::from),
                private: r["private"].as_bool().unwrap_or(false),
                fork: r["fork"].as_bool().unwrap_or(false),
                archived: r["archived"].as_bool().unwrap_or(false),
                language: r["language"]
                    .as_str()
                    .filter(|l| !l.is_empty())
                    .map(String::from),
                stars: r["stargazers_count"].as_u64().unwrap_or(0),
                clone_url: r["clone_url"].as_str()?.to_string(),
                pushed_at: r["pushed_at"].as_str().and_then(parse_time).unwrap_or(0),
            })
        })
        .collect()
}

/// The user's repositories (own, collaborator and organization), most
/// recently pushed first.
pub async fn repos(token: &str) -> Result<Vec<Repo>> {
    api("GET", "/user/repos?per_page=100&sort=pushed", token, None)
        .await
        .map(|v| parse_repos(&v))
}
