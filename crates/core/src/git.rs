//! Git through the `git` command line: status, staging, commits, branches,
//! merge, sync with the remote, history with a lane layout for the commit
//! graph, and diffs.
//!
//! Parsing is kept separate from running so it can be tested on fixed
//! output.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{CoreError, Result};

/// Local commands (status, log, diff, commit).
const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);
/// Commands that talk to a remote (fetch, pull, push, clone).
const REMOTE_TIMEOUT: Duration = Duration::from_secs(300);

/// What one `git` run printed.
pub struct GitOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl GitOutput {
    /// The useful part of what git said, for showing after an action.
    pub fn message(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.trim().to_string()
    }
}

/// Runs `git -C dir args…`. Never prompts in a terminal; with `token`, HTTPS
/// remotes on github.com fall back to it when no credential helper answers.
async fn git(dir: &Path, args: &[&str], token: Option<&str>, timeout: Duration) -> Result<GitOutput> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("-C").arg(dir);
    if token.is_some() {
        // Read from the environment so the token never shows up in `ps`.
        cmd.args([
            "-c",
            "credential.https://github.com.helper=!f() { test \"$1\" = get && echo username=x-access-token && echo \"password=$SAWIT_GITHUB_TOKEN\"; }; f",
        ]);
    }
    cmd.args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    if let Some(token) = token {
        cmd.env("SAWIT_GITHUB_TOKEN", token);
    }
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| CoreError::Command {
            program: format!("git {}", args.first().unwrap_or(&"")),
            code: None,
            stderr: format!("timed out after {timeout:?}"),
        })?
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => CoreError::Config(
                "Git isn't installed. Install the Xcode Command Line Tools with `xcode-select --install`, then try again.".into(),
            ),
            _ => CoreError::Io(e),
        })?;
    Ok(GitOutput {
        ok: out.status.success(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// Like [`git`], but a failure becomes an error carrying git's message.
async fn git_ok(dir: &Path, args: &[&str], token: Option<&str>, timeout: Duration) -> Result<String> {
    let out = git(dir, args, token, timeout).await?;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(CoreError::Git(out.message()))
    }
}

/// The top folder of the repository containing `dir`, if it is in one.
pub async fn discover(dir: &Path) -> Option<PathBuf> {
    let out = git(dir, &["rev-parse", "--show-toplevel"], None, LOCAL_TIMEOUT)
        .await
        .ok()?;
    out.ok
        .then(|| PathBuf::from(out.stdout.trim()))
        .filter(|p| !p.as_os_str().is_empty())
}

/// Makes `dir` a new repository with `main` as its first branch.
pub async fn init(dir: &Path) -> Result<()> {
    git_ok(dir, &["init", "-q", "-b", "main"], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

// ---------------------------------------------------------------- status

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// Current branch; `None` when HEAD is detached.
    pub branch: Option<String>,
    /// Short hash of HEAD; `None` before the first commit.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
    /// A merge is in progress (`MERGE_HEAD` exists).
    pub merging: bool,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.is_staged())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.is_unstaged())
    }

    pub fn conflicted(&self) -> impl Iterator<Item = &FileChange> {
        self.files.iter().filter(|f| f.conflicted)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    /// The old path of a rename or copy.
    pub from: Option<String>,
    /// Index (staged) state, porcelain letter: `M`, `A`, `D`, `R`, `C` or `.`.
    pub index: char,
    /// Work-tree (unstaged) state: `M`, `D`, `?` for untracked, or `.`.
    pub worktree: char,
    pub conflicted: bool,
}

impl FileChange {
    pub fn is_staged(&self) -> bool {
        !self.conflicted && self.index != '.' && self.index != '?'
    }

    pub fn is_unstaged(&self) -> bool {
        !self.conflicted && self.worktree != '.'
    }

    pub fn untracked(&self) -> bool {
        self.worktree == '?'
    }
}

/// Reads `git status --porcelain=v2 --branch -z`.
pub fn parse_status(raw: &str) -> Status {
    let mut status = Status::default();
    let mut fields = raw.split('\0');
    while let Some(entry) = fields.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => {
                    status.head = Some(value.chars().take(7).collect())
                }
                "branch.head" if value != "(detached)" => status.branch = Some(value.to_string()),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            status.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            status.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let mut chars = entry.chars();
        let kind = chars.next();
        let rest = chars.as_str().trim_start();
        match kind {
            // 1 XY sub mH mI mW hH hI path
            Some('1') => {
                if let Some((xy, path)) = split_entry(rest, 6) {
                    status.files.push(change(xy, path, None, false));
                }
            }
            // 2 XY sub mH mI mW hH hI Xscore path, then the old path
            Some('2') => {
                if let Some((xy, path)) = split_entry(rest, 7) {
                    let from = fields.next().map(String::from);
                    status.files.push(change(xy, path, from, false));
                }
            }
            // u XY sub m1 m2 m3 mW h1 h2 h3 path
            Some('u') => {
                if let Some((xy, path)) = split_entry(rest, 8) {
                    status.files.push(change(xy, path, None, true));
                }
            }
            Some('?') => status.files.push(FileChange {
                path: rest.to_string(),
                from: None,
                index: '.',
                worktree: '?',
                conflicted: false,
            }),
            _ => {}
        }
    }
    status
}

/// The `XY` field and the path after the `skip` fields that follow it.
fn split_entry(rest: &str, skip: usize) -> Option<(&str, &str)> {
    let mut parts = rest.splitn(skip + 2, ' ');
    let xy = parts.next()?;
    let path = parts.nth(skip)?;
    Some((xy, path))
}

fn change(xy: &str, path: &str, from: Option<String>, conflicted: bool) -> FileChange {
    let mut c = xy.chars();
    FileChange {
        path: path.to_string(),
        from,
        index: c.next().unwrap_or('.'),
        worktree: c.next().unwrap_or('.'),
        conflicted,
    }
}

pub async fn status(repo: &Path) -> Result<Status> {
    let raw = git_ok(
        repo,
        &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"],
        None,
        LOCAL_TIMEOUT,
    )
    .await?;
    let mut status = parse_status(&raw);
    status.merging = git(repo, &["rev-parse", "-q", "--verify", "MERGE_HEAD"], None, LOCAL_TIMEOUT)
        .await
        .is_ok_and(|o| o.ok);
    Ok(status)
}

// ---------------------------------------------------------------- staging

pub async fn stage(repo: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["add", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn stage_all(repo: &Path) -> Result<()> {
    git_ok(repo, &["add", "--all"], None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn unstage(repo: &Path, paths: &[String]) -> Result<()> {
    let mut args = vec!["restore", "--staged", "--"];
    args.extend(paths.iter().map(String::as_str));
    // Before the first commit there is nothing to restore from.
    let out = git(repo, &args, None, LOCAL_TIMEOUT).await?;
    if out.ok {
        return Ok(());
    }
    let mut args = vec!["rm", "--cached", "-r", "-q", "--"];
    args.extend(paths.iter().map(String::as_str));
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn unstage_all(repo: &Path) -> Result<()> {
    let out = git(repo, &["reset", "-q"], None, LOCAL_TIMEOUT).await?;
    if out.ok {
        return Ok(());
    }
    git_ok(repo, &["rm", "--cached", "-r", "-q", "."], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

/// Throws away unstaged changes to `file` (deletes it when untracked).
pub async fn discard(repo: &Path, file: &FileChange) -> Result<()> {
    if file.untracked() {
        git_ok(repo, &["clean", "-f", "-q", "--", &file.path], None, LOCAL_TIMEOUT)
            .await
            .map(drop)
    } else {
        git_ok(repo, &["restore", "--worktree", "--", &file.path], None, LOCAL_TIMEOUT)
            .await
            .map(drop)
    }
}

pub async fn commit(repo: &Path, message: &str, amend: bool) -> Result<String> {
    let mut args = vec!["commit", "-m", message];
    if amend {
        args.push("--amend");
    }
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await
}

/// The last commit's message, to edit when amending.
pub async fn last_message(repo: &Path) -> Result<String> {
    git_ok(repo, &["log", "-1", "--format=%B"], None, LOCAL_TIMEOUT)
        .await
        .map(|s| s.trim_end().to_string())
}

// ---------------------------------------------------------------- branches

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main`, or `origin/main` for a remote branch.
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
}

/// Reads `for-each-ref --format=%(refname)%00%(upstream:short)%00%(HEAD)`.
pub fn parse_branches(raw: &str) -> Vec<Branch> {
    raw.lines()
        .filter_map(|line| {
            let mut f = line.split('\0');
            let refname = f.next()?;
            let upstream = f.next().filter(|s| !s.is_empty()).map(String::from);
            let current = f.next() == Some("*");
            let (name, remote) = if let Some(n) = refname.strip_prefix("refs/heads/") {
                (n, false)
            } else {
                (refname.strip_prefix("refs/remotes/")?, true)
            };
            // `origin/HEAD` points at another remote branch.
            (!(remote && name.ends_with("/HEAD"))).then(|| Branch {
                name: name.to_string(),
                remote,
                current,
                upstream,
            })
        })
        .collect()
}

pub async fn branches(repo: &Path) -> Result<Vec<Branch>> {
    git_ok(
        repo,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)%00%(upstream:short)%00%(HEAD)",
            "refs/heads",
            "refs/remotes",
        ],
        None,
        LOCAL_TIMEOUT,
    )
    .await
    .map(|raw| parse_branches(&raw))
}

/// Switches to `name`; a remote branch (`origin/x`) gets a local tracking
/// branch `x`.
pub async fn switch(repo: &Path, branch: &Branch) -> Result<()> {
    let args: Vec<&str> = if branch.remote {
        let local = branch.name.split_once('/').map_or(branch.name.as_str(), |(_, b)| b);
        vec!["switch", "--track", "-c", local, &branch.name]
    } else {
        vec!["switch", &branch.name]
    };
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await.map(drop)
}

pub async fn create_branch(repo: &Path, name: &str) -> Result<()> {
    git_ok(repo, &["switch", "-c", name], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

/// Merges `branch` into the current branch. Conflicts are not an error: the
/// returned output says what happened and [`Status::merging`] is set.
pub async fn merge(repo: &Path, branch: &str) -> Result<GitOutput> {
    git(repo, &["merge", "--no-edit", branch], None, LOCAL_TIMEOUT).await
}

pub async fn merge_abort(repo: &Path) -> Result<()> {
    git_ok(repo, &["merge", "--abort"], None, LOCAL_TIMEOUT)
        .await
        .map(drop)
}

// ---------------------------------------------------------------- remote

pub async fn fetch(repo: &Path, token: Option<&str>) -> Result<String> {
    git_ok(repo, &["fetch", "--all", "--prune"], token, REMOTE_TIMEOUT).await
}

/// Fast-forwards only: a diverged branch is left for an explicit merge.
pub async fn pull(repo: &Path, token: Option<&str>) -> Result<String> {
    git_ok(repo, &["pull", "--ff-only"], token, REMOTE_TIMEOUT).await
}

/// Pushes the current branch, setting its upstream on `origin` the first time.
pub async fn push(repo: &Path, has_upstream: bool, token: Option<&str>) -> Result<String> {
    let args: &[&str] = if has_upstream {
        &["push"]
    } else {
        &["push", "-u", "origin", "HEAD"]
    };
    let out = git(repo, args, token, REMOTE_TIMEOUT).await?;
    if out.ok {
        Ok(out.message())
    } else {
        Err(CoreError::Git(out.message()))
    }
}

pub async fn clone(url: &str, into: &Path, token: Option<&str>) -> Result<()> {
    let parent = into.parent().unwrap_or(Path::new("/"));
    let target = into.to_string_lossy();
    git_ok(parent, &["clone", "--", url, &target], token, REMOTE_TIMEOUT)
        .await
        .map(drop)
}

pub async fn remote_url(repo: &Path) -> Option<String> {
    let out = git(repo, &["remote", "get-url", "origin"], None, LOCAL_TIMEOUT)
        .await
        .ok()?;
    out.ok.then(|| out.stdout.trim().to_string())
}

/// `owner/name` of a github.com remote, from its HTTPS or SSH URL.
pub fn github_slug(url: &str) -> Option<(String, String)> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))
        .or_else(|| {
            // https://user@github.com/…
            let rest = url.strip_prefix("https://")?;
            rest.split_once("@github.com/").map(|(_, p)| p)
        })?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| (owner.to_string(), name.to_string()))
}

// ---------------------------------------------------------------- history

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
    /// Branch and tag names pointing here (`HEAD -> main`, `tag: v1`, …).
    pub refs: Vec<String>,
    pub subject: String,
}

impl Commit {
    pub fn short(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }
}

const LOG_FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%at%x00%D%x00%s%x1e";

/// Reads `git log` printed with [`LOG_FORMAT`].
pub fn parse_log(raw: &str) -> Vec<Commit> {
    raw.split('\x1e')
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            if record.is_empty() {
                return None;
            }
            let mut f = record.splitn(7, '\0');
            let hash = f.next()?.to_string();
            let parents = f.next()?.split_whitespace().map(String::from).collect();
            let author = f.next()?.to_string();
            let email = f.next()?.to_string();
            let time = f.next()?.parse().unwrap_or(0);
            let refs = f
                .next()?
                .split(", ")
                .filter(|r| !r.is_empty())
                .map(String::from)
                .collect();
            let subject = f.next().unwrap_or_default().trim_end().to_string();
            Some(Commit {
                hash,
                parents,
                author,
                email,
                time,
                refs,
                subject,
            })
        })
        .collect()
}

/// The newest `limit` commits on every branch, newest first. `path` limits
/// it to the current branch's commits touching that file or folder, with
/// parents rewritten to those commits so the graph stays connected.
pub async fn log(repo: &Path, limit: usize, path: Option<&str>) -> Result<Vec<Commit>> {
    let limit = format!("-n{limit}");
    let mut args = vec!["log", "--date-order", LOG_FORMAT, &limit];
    match path {
        Some(path) => args.extend(["--parents", "--", path]),
        None => args.extend(["--all"]),
    }
    let out = git(repo, &args, None, LOCAL_TIMEOUT).await?;
    if out.ok {
        Ok(parse_log(&out.stdout))
    } else if out.stderr.contains("does not have any commits") {
        Ok(Vec::new())
    } else {
        Err(CoreError::Git(out.message()))
    }
}

/// One line of a drawn edge in a graph row; lanes are columns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Straight through the row in this lane.
    Pass(usize),
    /// From the top of this lane into the row's commit.
    In(usize),
    /// From the row's commit to the bottom of this lane.
    Out(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphRow {
    /// The lane the commit's dot is in.
    pub lane: usize,
    pub edges: Vec<Edge>,
}

/// Assigns each commit (newest first, parents after children) a lane and the
/// edges to draw in its row. Lanes keep their column until they end, so a
/// line never jumps sideways except where it meets its commit.
pub fn graph(commits: &[Commit]) -> Vec<GraphRow> {
    // The commit each lane is waiting for.
    let mut lanes: Vec<Option<&str>> = Vec::new();
    commits
        .iter()
        .map(|commit| {
            let mut edges = Vec::new();
            let mut lane = None;
            for (i, waiting) in lanes.iter_mut().enumerate() {
                match *waiting {
                    Some(h) if h == commit.hash => {
                        edges.push(Edge::In(i));
                        lane.get_or_insert(i);
                        *waiting = None;
                    }
                    Some(_) => edges.push(Edge::Pass(i)),
                    None => {}
                }
            }
            let lane = lane.unwrap_or_else(|| free_lane(&mut lanes));
            for (n, parent) in commit.parents.iter().enumerate() {
                let target = if n == 0 {
                    lanes[lane] = Some(parent.as_str());
                    lane
                } else if let Some(i) = lanes.iter().position(|l| *l == Some(parent.as_str())) {
                    i
                } else {
                    let i = free_lane(&mut lanes);
                    lanes[i] = Some(parent.as_str());
                    i
                };
                edges.push(Edge::Out(target));
            }
            while lanes.last() == Some(&None) {
                lanes.pop();
            }
            GraphRow { lane, edges }
        })
        .collect()
}

fn free_lane(lanes: &mut Vec<Option<&str>>) -> usize {
    lanes.iter().position(Option::is_none).unwrap_or_else(|| {
        lanes.push(None);
        lanes.len() - 1
    })
}

/// Lanes a set of rows needs side by side.
pub fn lane_count(rows: &[GraphRow]) -> usize {
    rows.iter()
        .flat_map(|r| {
            std::iter::once(r.lane).chain(r.edges.iter().map(|e| match *e {
                Edge::Pass(l) | Edge::In(l) | Edge::Out(l) => l,
            }))
        })
        .max()
        .map_or(0, |m| m + 1)
}

#[derive(Clone, Debug, Default)]
pub struct CommitDetail {
    pub body: String,
    pub committer: String,
    /// The patch against the first parent.
    pub patch: String,
}

pub async fn show(repo: &Path, hash: &str) -> Result<CommitDetail> {
    let raw = git_ok(
        repo,
        &["show", "--first-parent", "-m", "--format=%cn%x00%B%x00", "--patch", "--no-color", hash],
        None,
        LOCAL_TIMEOUT,
    )
    .await?;
    let mut parts = raw.splitn(3, '\0');
    Ok(CommitDetail {
        committer: parts.next().unwrap_or_default().to_string(),
        body: parts.next().unwrap_or_default().trim().to_string(),
        patch: parts.next().unwrap_or_default().trim_start().to_string(),
    })
}

/// The patch for one changed file: staged (index vs HEAD) or not (work tree
/// vs index; an untracked file shows as all new).
pub async fn diff(repo: &Path, file: &FileChange, staged: bool) -> Result<String> {
    if !staged && file.untracked() {
        let out = git(
            repo,
            &["diff", "--no-color", "--no-index", "--", "/dev/null", &file.path],
            None,
            LOCAL_TIMEOUT,
        )
        .await?;
        // Exit 1 means "there are differences".
        return Ok(out.stdout);
    }
    let mut args = vec!["diff", "--no-color"];
    if staged {
        args.push("--cached");
    }
    args.push("--");
    if let Some(from) = file.from.as_deref().filter(|_| staged) {
        args.push(from);
    }
    args.push(&file.path);
    git_ok(repo, &args, None, LOCAL_TIMEOUT).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_branch_and_every_entry_kind() {
        let raw = "# branch.oid 1234567890abcdef\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/a file.rs\0\
1 A. N... 000000 100644 100644 000 ccc new.rs\0\
2 R. N... 100644 100644 100644 ddd ddd R100 b.rs\0old b.rs\0\
u UU N... 100644 100644 100644 100644 e f g conflict.rs\0\
? notes.txt\0";
        let s = parse_status(raw);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(s.head.as_deref(), Some("1234567"));
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        let paths: Vec<_> = s.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["src/a file.rs", "new.rs", "b.rs", "conflict.rs", "notes.txt"]);
        assert_eq!(s.files[2].from.as_deref(), Some("old b.rs"));
        assert_eq!(s.staged().count(), 2);
        assert_eq!(s.unstaged().count(), 2);
        assert_eq!(s.conflicted().count(), 1);
        assert!(s.files[4].untracked());
    }

    #[test]
    fn status_before_first_commit_and_detached() {
        let s = parse_status("# branch.oid (initial)\0# branch.head (detached)\0");
        assert_eq!(s.head, None);
        assert_eq!(s.branch, None);
        assert!(s.files.is_empty());
    }

    #[test]
    fn branches_skip_remote_head_and_mark_current() {
        let raw = "refs/heads/main\0origin/main\0*\nrefs/heads/feat\0\0 \nrefs/remotes/origin/HEAD\0\0 \nrefs/remotes/origin/main\0\0 \n";
        let b = parse_branches(raw);
        assert_eq!(b.len(), 3);
        assert!(b[0].current && !b[0].remote);
        assert_eq!(b[0].upstream.as_deref(), Some("origin/main"));
        assert_eq!(b[1].upstream, None);
        assert!(b[2].remote && b[2].name == "origin/main");
    }

    #[test]
    fn github_slug_from_remote_urls() {
        let want = Some(("acme".to_string(), "app".to_string()));
        assert_eq!(github_slug("https://github.com/acme/app.git"), want);
        assert_eq!(github_slug("https://github.com/acme/app"), want);
        assert_eq!(github_slug("git@github.com:acme/app.git"), want);
        assert_eq!(github_slug("ssh://git@github.com/acme/app.git"), want);
        assert_eq!(github_slug("https://me@github.com/acme/app.git"), want);
        assert_eq!(github_slug("https://gitlab.com/acme/app.git"), None);
        assert_eq!(github_slug("https://github.com/acme"), None);
    }

    fn c(hash: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            email: String::new(),
            time: 0,
            refs: Vec::new(),
            subject: String::new(),
        }
    }

    #[test]
    fn log_parses_records() {
        let raw = "aaa\0bbb ccc\0Ann\0ann@x\01700000000\0HEAD -> main, origin/main\0Merge feat\x1e\nbbb\0\0Bo\0bo@x\01600000000\0\0First\x1e\n";
        let log = parse_log(raw);
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].parents, ["bbb", "ccc"]);
        assert_eq!(log[0].refs, ["HEAD -> main", "origin/main"]);
        assert_eq!(log[0].subject, "Merge feat");
        assert!(log[1].parents.is_empty() && log[1].refs.is_empty());
        assert_eq!(log[1].time, 1600000000);
    }

    #[test]
    fn graph_linear_history_stays_in_one_lane() {
        let rows = graph(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert!(rows.iter().all(|r| r.lane == 0));
        assert_eq!(rows[0].edges, [Edge::Out(0)]);
        assert_eq!(rows[1].edges, [Edge::In(0), Edge::Out(0)]);
        assert_eq!(rows[2].edges, [Edge::In(0)]);
        assert_eq!(lane_count(&rows), 1);
    }

    #[test]
    fn graph_merge_branches_out_and_joins_back() {
        // m merges f into b; f and b both come from a.
        let rows = graph(&[
            c("m", &["b", "f"]),
            c("f", &["a"]),
            c("b", &["a"]),
            c("a", &[]),
        ]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[0].edges, [Edge::Out(0), Edge::Out(1)]);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].edges, [Edge::Pass(0), Edge::In(1), Edge::Out(1)]);
        assert_eq!(rows[2].lane, 0);
        // Both lanes wait for `a`; b's lane takes it, f's lane closes into it.
        assert_eq!(rows[2].edges, [Edge::In(0), Edge::Pass(1), Edge::Out(0)]);
        assert_eq!(rows[3].lane, 0);
        assert_eq!(rows[3].edges, [Edge::In(0), Edge::In(1)]);
        assert_eq!(lane_count(&rows), 2);
    }

    #[test]
    fn graph_unrelated_tips_get_their_own_lanes() {
        let rows = graph(&[c("x", &["a"]), c("y", &["a"]), c("a", &[])]);
        assert_eq!(rows[0].lane, 0);
        assert_eq!(rows[1].lane, 1);
        assert_eq!(rows[1].edges, [Edge::Pass(0), Edge::Out(1)]);
        assert_eq!(rows[2].edges, [Edge::In(0), Edge::In(1)]);
    }

    /// Runs real git in a scratch repository.
    #[test]
    fn end_to_end_status_stage_commit_branch_merge() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            git_ok(repo, &["init", "-q", "-b", "main"], None, LOCAL_TIMEOUT).await.unwrap();
            for (k, v) in [("user.name", "Test"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
                git_ok(repo, &["config", k, v], None, LOCAL_TIMEOUT).await.unwrap();
            }

            assert_eq!(discover(&repo.join(".")).await.map(|p| p.canonicalize().unwrap()), Some(repo.canonicalize().unwrap()));
            std::fs::write(repo.join("a.txt"), "one\n").unwrap();
            let s = status(repo).await.unwrap();
            assert_eq!(s.branch.as_deref(), Some("main"));
            assert!(s.files[0].untracked());
            assert_eq!(diff(repo, &s.files[0], false).await.unwrap().lines().filter(|l| l.starts_with('+')).count(), 2);

            stage_all(repo).await.unwrap();
            assert_eq!(status(repo).await.unwrap().staged().count(), 1);
            unstage_all(repo).await.unwrap();
            assert_eq!(status(repo).await.unwrap().staged().count(), 0);
            stage(repo, &["a.txt".into()]).await.unwrap();
            commit(repo, "first", false).await.unwrap();
            assert_eq!(last_message(repo).await.unwrap(), "first");

            create_branch(repo, "feat").await.unwrap();
            std::fs::write(repo.join("b.txt"), "two\n").unwrap();
            stage_all(repo).await.unwrap();
            commit(repo, "second", false).await.unwrap();
            let main = branches(repo).await.unwrap().into_iter().find(|b| b.name == "main").unwrap();
            switch(repo, &main).await.unwrap();
            assert!(merge(repo, "feat").await.unwrap().ok);

            let log = log(repo, 10, None).await.unwrap();
            assert_eq!(log.len(), 2);
            assert_eq!(log[0].subject, "second");
            let detail = show(repo, &log[0].hash).await.unwrap();
            assert_eq!(detail.body, "second");
            assert!(detail.patch.contains("+two"));

            std::fs::write(repo.join("a.txt"), "changed\n").unwrap();
            let s = status(repo).await.unwrap();
            discard(repo, &s.files[0]).await.unwrap();
            assert!(status(repo).await.unwrap().files.is_empty());
        });
    }
}
