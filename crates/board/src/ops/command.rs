//! The `command` and `commands` ops: an engine command run on the canvas, guarded so that it
//! leaves the canvas its one document, and the list of the commands an agent can run.

use figcraft_engine::{EngineError, Session};
use serde_json::{Value, json};
use thiserror::Error;

use super::files::canvas_files_by_bytes;
use super::refusal::{handled, refused};
use super::{BATCH, failure, success};
use crate::doc::{BoardDoc, BoardError};
use crate::policy;

/// The commands that open a document in the session. The `command` op never leaves one open
/// ([`Documents`] undoes it), so none of these can succeed through it.
pub(super) const OPENS_DOCUMENTS: &[&str] = &["file.recover", "version.duplicate", "branch.open"];

pub(super) fn commands() -> Value {
    let commands: Vec<Value> = figcraft_engine::cmd::all_commands()
        .iter()
        .filter(|command| listed(command.id))
        .map(|command| json!({"id": command.id, "label": command.label, "params": command.params}))
        .collect();
    success("commands", Value::Array(commands))
}

/// Whether the `commands` op offers command `id`: the canvas doesn't hide it, and the `command`
/// op runs it to a result. That one rule leaves out what Sawit does itself ([`HANDLED_BY_SAWIT`],
/// the save and the like) and what only opens a document ([`OPENS_DOCUMENTS`]), so every command
/// listed is one an agent can call.
fn listed(id: &str) -> bool {
    !policy::is_hidden(id) && handled(id).is_none() && !OPENS_DOCUMENTS.contains(&id)
}

pub(super) fn command(board: &mut BoardDoc, op: &Value) -> Value {
    let Some(id) = op.get("command").and_then(Value::as_str) else {
        return failure(
            r#"a command op needs a command id, like {"op": "command", "command": "view.zoomIn"}"#,
        );
    };
    let params = match op.get("params") {
        None | Some(Value::Null) => json!({}),
        Some(params) => params.clone(),
    };
    if let Some(refusal) = refused(id, &params) {
        return failure(refusal);
    }
    let params = match canvas_files_by_bytes(id, params) {
        Ok(params) => params,
        Err(error) => return failure(error),
    };
    let history = FileHistory::of(&board.session);
    let ran = if id == BATCH {
        batch(board, &params)
    } else {
        single(board, id, &params)
    };
    match ran {
        Ok(result) => success("result", result),
        Err(error) => {
            if let Some(history) = history {
                history.restore(&mut board.session);
            }
            failure(error)
        }
    }
}

/// The version history and branches of the canvas's file, as a command op found them.
///
/// The engine keeps them outside undo, so neither its rollback of a failed command or group nor
/// [`Documents::restore`] puts them back: a refused `branch.create` has already added its branch,
/// with two copies of the document, before the guard closes the branch's tab. A command op that
/// fails puts them back, so the canvas's next save writes nothing of what the op tried.
struct FileHistory {
    /// The uid of the canvas's document, which holds them.
    uid: u64,
    versions: Vec<figcraft_format::Version>,
    branches: Vec<figcraft_format::Branch>,
}

impl FileHistory {
    /// Those of the document on show, the canvas's. `None` without one.
    fn of(session: &Session) -> Option<Self> {
        let ds = session.ds().ok()?;
        Some(Self {
            uid: ds.uid,
            versions: ds.versions.clone(),
            branches: ds.branches.clone(),
        })
    }

    /// Puts them back on the document they were read from.
    fn restore(self, session: &mut Session) {
        if let Some(ds) = session.docs.iter_mut().find(|ds| ds.uid == self.uid) {
            ds.versions = self.versions;
            ds.branches = self.branches;
        }
    }
}

/// Why a command op has no result.
#[derive(Debug, Error)]
enum Failed {
    /// The engine refused the command, or it failed.
    #[error(transparent)]
    Engine(#[from] BoardError),
    /// The command would have left the canvas with another document open or on show; the canvas is
    /// as it was.
    #[error("{0} would open or switch documents; the canvas keeps its one document")]
    Switched(String),
}

/// The documents a session holds, as a command op found them: their uids in order, and the uid of
/// the one on show.
///
/// A uid names a document for good, because the engine gives every document it makes a new one. So
/// closing the canvas's document and opening another in its place, which leaves the count and the
/// index on show as they were, still reads as the change it is.
#[derive(Debug, PartialEq)]
pub(super) struct Documents {
    uids: Vec<u64>,
    shown: Option<u64>,
}

impl Documents {
    pub(super) fn of(session: &Session) -> Self {
        Self {
            uids: session.docs.iter().map(|doc| doc.uid).collect(),
            shown: session.ds().ok().map(|doc| doc.uid),
        }
    }

    /// Closes the documents opened since, and shows the one that was on show again.
    pub(super) fn restore(&self, session: &mut Session) {
        session.docs.retain(|doc| self.uids.contains(&doc.uid));
        session.active = self
            .shown
            .and_then(|uid| session.docs.iter().position(|doc| doc.uid == uid))
            .unwrap_or(0);
    }
}

/// Runs command `id`, other than a batch. If it left other documents open, or another on show,
/// those are closed again and the canvas's document is shown: nothing but `file.close` removes the
/// canvas's own document outside a batch, and Sawit refuses that one.
///
/// Not run in a group of its own: `edit.undo` and `edit.redo` aren't undo steps, and a group would
/// record them as new ones and drop what can be redone.
fn single(board: &mut BoardDoc, id: &str, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let result = board.execute(id, params);
    if Documents::of(&board.session) != before {
        before.restore(&mut board.session);
        return Err(Failed::Switched(id.into()));
    }
    Ok(result?)
}

/// Runs a `command.batch` in a group that fails when its steps left other documents open, or
/// another on show: the engine then puts back the documents, the one on show and the canvas's own
/// document, so nothing of the batch stays.
///
/// The group takes the label the batch's own group does (`label`, else "Batch"), so a batch that
/// stays is the one undo step it always was.
fn batch(board: &mut BoardDoc, params: &Value) -> Result<Value, Failed> {
    let before = Documents::of(&board.session);
    let label = figcraft_engine::params::string(params, "label").unwrap_or_else(|| "Batch".into());
    let mut switched = false;
    let result = crate::doc::grouped(&mut board.session, &label, |session| {
        let result = session.execute(BATCH, params)?;
        if Documents::of(session) != before {
            switched = true;
            return Err(EngineError::Disabled(
                BATCH.into(),
                "it would open or switch documents".into(),
            ));
        }
        Ok(result)
    });
    if switched {
        return Err(Failed::Switched(BATCH.into()));
    }
    Ok(result.map_err(BoardError::from)?)
}
