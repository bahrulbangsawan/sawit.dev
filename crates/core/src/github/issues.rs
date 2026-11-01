//! Issues and pull requests: listing them, their comments, opening and closing, a pull
//! request's merge state and diff, merging, and making new ones.

use serde_json::{Value, json};

use super::http::{api, api_raw};
use super::time::parse_time;
use crate::Result;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub author: String,
    pub open: bool,
    pub pull: bool,
    pub comments: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub url: String,
}

pub fn parse_items(v: &Value) -> Vec<Item> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            Some(Item {
                number: i["number"].as_u64()?,
                title: i["title"].as_str().unwrap_or_default().to_string(),
                body: i["body"].as_str().unwrap_or_default().to_string(),
                author: i["user"]["login"].as_str().unwrap_or("ghost").to_string(),
                open: i["state"].as_str() == Some("open"),
                pull: i.get("pull_request").is_some_and(|p| !p.is_null()),
                comments: i["comments"].as_u64().unwrap_or(0),
                created_at: i["created_at"].as_str().and_then(parse_time).unwrap_or(0),
                updated_at: i["updated_at"].as_str().and_then(parse_time).unwrap_or(0),
                url: i["html_url"].as_str().unwrap_or_default().to_string(),
            })
        })
        .collect()
}

/// Issues and pull requests (GitHub lists both as issues), recently updated
/// first. `open` false lists closed ones.
pub async fn items(token: &str, owner: &str, repo: &str, open: bool) -> Result<Vec<Item>> {
    let state = if open { "open" } else { "closed" };
    api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues?state={state}&per_page=100&sort=updated"),
        token,
        None,
    )
    .await
    .map(|v| parse_items(&v))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub created_at: i64,
}

pub fn parse_comments(v: &Value) -> Vec<Comment> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|c| Comment {
            author: c["user"]["login"].as_str().unwrap_or("ghost").to_string(),
            body: c["body"].as_str().unwrap_or_default().to_string(),
            created_at: c["created_at"].as_str().and_then(parse_time).unwrap_or(0),
        })
        .collect()
}

pub async fn comments(token: &str, owner: &str, repo: &str, number: u64) -> Result<Vec<Comment>> {
    api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues/{number}/comments?per_page=100"),
        token,
        None,
    )
    .await
    .map(|v| parse_comments(&v))
}

pub async fn add_comment(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    body: &str,
) -> Result<Comment> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/issues/{number}/comments"),
        token,
        Some(json!({ "body": body })),
    )
    .await?;
    Ok(parse_comments(&Value::Array(vec![v])).remove(0))
}

/// Opens or closes an issue or pull request.
pub async fn set_open(token: &str, owner: &str, repo: &str, number: u64, open: bool) -> Result<()> {
    api(
        "PATCH",
        &format!("/repos/{owner}/{repo}/issues/{number}"),
        token,
        Some(json!({ "state": if open { "open" } else { "closed" } })),
    )
    .await
    .map(drop)
}

/// A pull request's merge state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PullState {
    pub merged: bool,
    /// `None` while GitHub is still working it out.
    pub mergeable: Option<bool>,
    pub head: String,
    pub base: String,
    pub additions: u64,
    pub deletions: u64,
    pub changed_files: u64,
}

pub async fn pull_state(token: &str, owner: &str, repo: &str, number: u64) -> Result<PullState> {
    let v = api(
        "GET",
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        token,
        None,
    )
    .await?;
    Ok(PullState {
        merged: v["merged"].as_bool().unwrap_or(false),
        mergeable: v["mergeable"].as_bool(),
        head: v["head"]["ref"].as_str().unwrap_or_default().to_string(),
        base: v["base"]["ref"].as_str().unwrap_or_default().to_string(),
        additions: v["additions"].as_u64().unwrap_or(0),
        deletions: v["deletions"].as_u64().unwrap_or(0),
        changed_files: v["changed_files"].as_u64().unwrap_or(0),
    })
}

/// The pull request's whole change as a unified diff.
pub async fn pull_diff(token: &str, owner: &str, repo: &str, number: u64) -> Result<String> {
    api_raw(
        "GET",
        &format!("/repos/{owner}/{repo}/pulls/{number}"),
        token,
        None,
        "application/vnd.github.diff",
    )
    .await
}

pub async fn merge_pull(token: &str, owner: &str, repo: &str, number: u64) -> Result<()> {
    api(
        "PUT",
        &format!("/repos/{owner}/{repo}/pulls/{number}/merge"),
        token,
        Some(json!({ "merge_method": "merge" })),
    )
    .await
    .map(drop)
}

pub async fn create_pull(
    token: &str,
    owner: &str,
    repo: &str,
    head: &str,
    base: &str,
    title: &str,
    body: &str,
) -> Result<Item> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/pulls"),
        token,
        Some(json!({ "head": head, "base": base, "title": title, "body": body })),
    )
    .await?;
    let mut item = parse_items(&Value::Array(vec![v])).remove(0);
    item.pull = true;
    Ok(item)
}

pub async fn create_issue(
    token: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
) -> Result<Item> {
    let v = api(
        "POST",
        &format!("/repos/{owner}/{repo}/issues"),
        token,
        Some(json!({ "title": title, "body": body })),
    )
    .await?;
    Ok(parse_items(&Value::Array(vec![v])).remove(0))
}

/// The repository's default branch, for new pull requests.
pub async fn default_branch(token: &str, owner: &str, repo: &str) -> Result<String> {
    let v = api("GET", &format!("/repos/{owner}/{repo}"), token, None).await?;
    Ok(v["default_branch"].as_str().unwrap_or("main").to_string())
}
