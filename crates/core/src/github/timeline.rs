//! What happened on an issue or pull request, from GitHub's timeline, and the rows it is shown
//! as: a run of commits by one author becomes one push.

use serde_json::Value;

use super::http::api;
use super::time::parse_time;
use crate::Result;

/// One thing that happened on an issue or pull request, oldest first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A commit pushed to the pull request; [`group`] folds a run of them.
    Committed {
        author: String,
        time: i64,
        sha: String,
        message: String,
    },
    Commented {
        author: String,
        time: i64,
        body: String,
    },
    Reviewed {
        author: String,
        time: i64,
        state: ReviewState,
        body: String,
        /// GitHub's review id, to attach inline comments.
        id: u64,
        comments: Vec<ReviewComment>,
    },
    /// Merged, closed, renamed, labeled… one line, no body.
    Action {
        actor: String,
        time: i64,
        kind: ActionKind,
        /// What it was about: the new title, the label, the reviewer, a
        /// commit, a linked issue.
        detail: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    Merged,
    Closed,
    Reopened,
    Renamed,
    Labeled,
    Unlabeled,
    ReviewRequested,
    Assigned,
    ForcePushed,
    BranchDeleted,
    ReadyForReview,
    MarkedDraft,
    Referenced,
}

/// A comment on a line of a file, part of a review.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewComment {
    pub path: String,
    /// `None` when the line has since changed (outdated).
    pub line: Option<u64>,
    pub body: String,
}

impl Event {
    pub fn time(&self) -> i64 {
        match self {
            Event::Committed { time, .. }
            | Event::Commented { time, .. }
            | Event::Reviewed { time, .. }
            | Event::Action { time, .. } => *time,
        }
    }
}

fn login(v: &Value) -> String {
    v["login"].as_str().unwrap_or("ghost").to_string()
}

fn time_of(v: &Value, key: &str) -> i64 {
    v[key].as_str().and_then(parse_time).unwrap_or(0)
}

/// Reads the issue timeline API; events Sawit doesn't show are skipped.
pub fn parse_timeline(v: &Value) -> Vec<Event> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let event = e["event"].as_str()?;
            let action = |kind: ActionKind, detail: String| {
                Some(Event::Action {
                    actor: login(&e["actor"]),
                    time: time_of(e, "created_at"),
                    kind,
                    detail,
                })
            };
            match event {
                "committed" => Some(Event::Committed {
                    author: e["author"]["name"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string(),
                    time: e["author"]["date"]
                        .as_str()
                        .and_then(parse_time)
                        .unwrap_or(0),
                    sha: e["sha"].as_str()?.to_string(),
                    message: e["message"]
                        .as_str()
                        .unwrap_or_default()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                }),
                "commented" => Some(Event::Commented {
                    author: login(&e["user"]),
                    time: time_of(e, "created_at"),
                    body: e["body"].as_str().unwrap_or_default().to_string(),
                }),
                "reviewed" => Some(Event::Reviewed {
                    author: login(&e["user"]),
                    time: time_of(e, "submitted_at"),
                    state: match e["state"].as_str() {
                        Some("approved") => ReviewState::Approved,
                        Some("changes_requested") => ReviewState::ChangesRequested,
                        Some("dismissed") => ReviewState::Dismissed,
                        _ => ReviewState::Commented,
                    },
                    body: e["body"].as_str().unwrap_or_default().to_string(),
                    id: e["id"].as_u64().unwrap_or(0),
                    comments: Vec::new(),
                }),
                "merged" => action(
                    ActionKind::Merged,
                    e["commit_id"]
                        .as_str()
                        .map(|c| c[..c.len().min(7)].to_string())
                        .unwrap_or_default(),
                ),
                "closed" => action(ActionKind::Closed, String::new()),
                "reopened" => action(ActionKind::Reopened, String::new()),
                "renamed" => action(
                    ActionKind::Renamed,
                    e["rename"]["to"].as_str().unwrap_or_default().to_string(),
                ),
                "labeled" => action(
                    ActionKind::Labeled,
                    e["label"]["name"].as_str().unwrap_or_default().to_string(),
                ),
                "unlabeled" => action(
                    ActionKind::Unlabeled,
                    e["label"]["name"].as_str().unwrap_or_default().to_string(),
                ),
                "review_requested" => action(
                    ActionKind::ReviewRequested,
                    e["requested_reviewer"]["login"]
                        .as_str()
                        .or(e["requested_team"]["name"].as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
                "assigned" => action(ActionKind::Assigned, login(&e["assignee"])),
                "head_ref_force_pushed" => action(ActionKind::ForcePushed, String::new()),
                "head_ref_deleted" => action(ActionKind::BranchDeleted, String::new()),
                "ready_for_review" => action(ActionKind::ReadyForReview, String::new()),
                "convert_to_draft" => action(ActionKind::MarkedDraft, String::new()),
                "cross-referenced" => {
                    let issue = &e["source"]["issue"];
                    let number = issue["number"].as_u64()?;
                    action(
                        ActionKind::Referenced,
                        format!("#{number} {}", issue["title"].as_str().unwrap_or_default()),
                    )
                }
                _ => None,
            }
        })
        .collect()
}

/// Inline review comments, by the review they belong to.
pub fn parse_review_comments(v: &Value) -> Vec<(u64, ReviewComment)> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some((
                c["pull_request_review_id"].as_u64()?,
                ReviewComment {
                    path: c["path"].as_str().unwrap_or_default().to_string(),
                    line: c["line"].as_u64(),
                    body: c["body"].as_str().unwrap_or_default().to_string(),
                },
            ))
        })
        .collect()
}

/// Puts inline comments under their reviews.
pub fn attach_review_comments(events: &mut [Event], comments: Vec<(u64, ReviewComment)>) {
    for (review, comment) in comments {
        if let Some(Event::Reviewed { comments, .. }) = events
            .iter_mut()
            .find(|e| matches!(e, Event::Reviewed { id, .. } if *id == review))
        {
            comments.push(comment);
        }
    }
}

/// A page (100 events) of the timeline, newest pages later. For a pull
/// request, its reviews carry their inline comments.
pub async fn timeline(
    token: &str,
    owner: &str,
    repo: &str,
    number: u64,
    pull: bool,
    page: u32,
) -> Result<Vec<Event>> {
    let v = api(
        "GET",
        &format!("/repos/{owner}/{repo}/issues/{number}/timeline?per_page=100&page={page}"),
        token,
        None,
    )
    .await?;
    let mut events = parse_timeline(&v);
    if pull && events.iter().any(|e| matches!(e, Event::Reviewed { .. })) {
        let v = api(
            "GET",
            &format!("/repos/{owner}/{repo}/pulls/{number}/comments?per_page=100"),
            token,
            None,
        )
        .await?;
        attach_review_comments(&mut events, parse_review_comments(&v));
    }
    Ok(events)
}

/// Events per timeline page; a full page means there may be more.
pub const TIMELINE_PAGE: usize = 100;

/// A timeline row: consecutive commits by one author become one push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Pushed {
        author: String,
        time: i64,
        /// Short hash and first line.
        commits: Vec<(String, String)>,
    },
    Event(Event),
}

pub fn group(events: &[Event]) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for event in events {
        if let Event::Committed {
            author,
            time,
            sha,
            message,
        } = event
        {
            let commit = (sha[..sha.len().min(7)].to_string(), message.clone());
            if let Some(Entry::Pushed {
                author: a,
                time: t,
                commits,
            }) = entries.last_mut()
                && a == author
            {
                commits.push(commit);
                *t = (*t).max(*time);
                continue;
            }
            entries.push(Entry::Pushed {
                author: author.clone(),
                time: *time,
                commits: vec![commit],
            });
        } else {
            entries.push(Entry::Event(event.clone()));
        }
    }
    entries
}
