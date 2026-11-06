//! Running Claude Code sessions per account (Claude Code's own registry,
//! `<folder>/sessions/<pid>.json`, each checked against the live process), and **Continue On…**:
//! the conversation copied into another account and resumed there. A running session can never
//! change account (its sign-in is fixed when it starts), so a handoff starts a new one; the old
//! one keeps running until the user ends it.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::Account;
use crate::{CoreError, Result};

/// One running session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub account: String,
    pub pid: i32,
    pub session_id: String,
    pub cwd: String,
    pub name: Option<String>,
    /// Unix milliseconds.
    pub updated_at: Option<i64>,
    pub status: Option<String>,
    pub version: Option<String>,
}

impl Session {
    pub fn busy(&self) -> bool {
        self.status.as_deref() == Some("busy")
    }

    pub fn folder(&self) -> &str {
        self.cwd
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(&self.cwd)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    pid: i32,
    session_id: String,
    cwd: String,
    name: Option<String>,
    started_at: Option<f64>,
    updated_at: Option<f64>,
    status: Option<String>,
    version: Option<String>,
}

/// The sessions of `account` whose process still runs (and started when the entry says).
pub fn sessions(account: &Account, alive: &dyn Fn(i32, Option<f64>) -> bool) -> Vec<Session> {
    let dir = account.dir.join("sessions");
    let mut out: Vec<Session> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let pid: i32 = name.strip_suffix(".json")?.parse().ok()?;
            let text = std::fs::read_to_string(e.path())
                .ok()
                .filter(|t| t.len() < 256 * 1024)?;
            let entry: Entry = serde_json::from_str(&text).ok()?;
            (entry.pid == pid && alive(pid, entry.started_at)).then(|| Session {
                account: account.id.clone(),
                pid,
                session_id: entry.session_id.chars().take(64).collect(),
                cwd: entry.cwd,
                name: entry.name.map(|n| n.chars().take(80).collect()),
                updated_at: entry.updated_at.map(|t| t as i64),
                status: entry.status.map(|s| s.chars().take(24).collect()),
                version: entry.version.map(|s| s.chars().take(32).collect()),
            })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
    out
}

/// Whether process `pid` runs and started no later than the session did (a reused pid doesn't).
pub fn process_alive(pid: i32, started_ms: Option<f64>) -> bool {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as i32;
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    if n != size || info.pbi_status == libc::SZOMB {
        return false;
    }
    match started_ms {
        Some(ms) => (info.pbi_start_tvsec as f64) <= ms / 1000.0 + 2.0,
        None => true,
    }
}

/// Ends session `pid` as quitting it would (SIGTERM), after the user confirmed.
pub fn end(pid: i32) -> bool {
    pid > 0 && unsafe { libc::kill(pid, libc::SIGTERM) } == 0
}

/// What Continue On… copies, found before anything is written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub transcript: PathBuf,
    pub companion: Option<PathBuf>,
    pub destination: PathBuf,
    pub session_id: String,
}

fn valid_id(id: &str) -> bool {
    id.len() == 36
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && id.matches('-').count() == 4
}

/// Finds the session's conversation in `source` (the newest `projects/<key>/<id>.jsonl`) and where
/// it goes in `target`.
pub fn plan_handoff(session: &Session, source: &Account, target: &Account) -> Result<Handoff> {
    if source.id == target.id {
        return Err(CoreError::Config(
            "The session already runs on this account.".into(),
        ));
    }
    if !valid_id(&session.session_id) {
        return Err(CoreError::Config(
            "Claude Code's session id isn't a UUID, so it can't be resumed safely.".into(),
        ));
    }
    let projects = source.dir.join("projects");
    let file = format!("{}.jsonl", session.session_id);
    let transcript = std::fs::read_dir(&projects)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path().join(&file))
        .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(t, _)| *t)
        .map(|(_, p)| p)
        .ok_or_else(|| {
            CoreError::Config(format!(
                "Couldn't find this session's conversation under {}/projects: it may not have saved a message yet.",
                source.dir.display()
            ))
        })?;
    let project = transcript.parent().expect("projects/<key>").to_path_buf();
    let companion = Some(project.join(&session.session_id)).filter(|p| p.is_dir());
    let destination = target
        .dir
        .join("projects")
        .join(project.file_name().unwrap_or_default());
    Ok(Handoff {
        transcript,
        companion,
        destination,
        session_id: session.session_id.clone(),
    })
}

/// Copies the conversation (and its folder) into the target account, byte for byte. A copy
/// already there is renamed aside, never overwritten; a half-written last line is left out.
/// Returns the copies renamed aside.
pub fn copy_handoff(h: &Handoff, now: i64) -> Result<Vec<PathBuf>> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&h.destination)?;
    let stamp = crate::github::stamp(now).replace(':', "");
    let mut aside = Vec::new();
    let mut move_aside = |p: &Path| -> Result<()> {
        if p.symlink_metadata().is_ok() {
            let mut to = p.as_os_str().to_owned();
            to.push(format!(".sawit-bak-{stamp}"));
            std::fs::rename(p, &to)?;
            aside.push(PathBuf::from(to));
        }
        Ok(())
    };
    if let Some(c) = &h.companion {
        let dest = h.destination.join(&h.session_id);
        move_aside(&dest)?;
        copy_dir(c, &dest)?;
    }
    let mut data = std::fs::read(&h.transcript)?;
    if let Some(last) = data.iter().rposition(|b| *b == b'\n') {
        data.truncate(last + 1);
    }
    let dest = h.destination.join(format!("{}.jsonl", h.session_id));
    move_aside(&dest)?;
    crate::files::replace_file(&dest, &data, {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::Permissions::from_mode(0o600)
    })?;
    Ok(aside)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}
