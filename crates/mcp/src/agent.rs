//! Who this `sawit-mcp` is serving: one agent session ([`sawit_core::agents`]). Claude Code,
//! Codex and Cursor each start their own stdio server per session, and a session's subagents
//! share it, so the process is the session. The client says its name in MCP `initialize`
//! (`clientInfo`); the id is made once per process; the project is the one the folder the
//! session works in belongs to. Every op sent to the app carries the identity, so the app can show
//! each agent's cursor in its own colour, with its name.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use rmcp::model::Implementation;
use sawit_core::agents::{AgentIdentity, client_name, session_id};

/// This process's agent session: its id from the start, the client's name once it has said it.
#[derive(Debug)]
pub struct Session {
    id: String,
    project: Option<String>,
    client: OnceLock<(String, Option<String>)>,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    /// The session of this process, working in the project of the folder it was started in
    /// ([`crate::agent_project`]).
    pub fn new() -> Self {
        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let project = crate::agent_project(None)
            .and_then(|dir| dir.file_name().map(|n| n.to_string_lossy().into_owned()));
        Self::with(session_id(std::process::id(), started), project)
    }

    fn with(id: String, project: Option<String>) -> Self {
        Self {
            id,
            project,
            client: OnceLock::new(),
        }
    }

    /// The client said who it is in `initialize`. Only the first `initialize` counts: a session
    /// has one client.
    pub fn met(&self, client: &Implementation) {
        let name = client_name(&client.name, client.title.as_deref());
        let version = Some(client.version.trim().to_string()).filter(|v| !v.is_empty());
        let _ = self.client.set((name, version));
    }

    /// The identity every op to the app carries. Before `initialize` (a test calling a tool
    /// directly), the client is "Agent".
    pub fn identity(&self) -> AgentIdentity {
        let (name, version) = self
            .client
            .get()
            .cloned()
            .unwrap_or_else(|| (AgentIdentity::anonymous().name, None));
        AgentIdentity {
            id: self.id.clone(),
            name,
            label: self.project.clone(),
            version,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The client's name and version from `initialize`, the process's id and the project's
    /// folder name make the identity; a second `initialize` changes nothing.
    #[test]
    fn the_identity_is_the_clients_and_the_processs() {
        let session = Session::with("mcp-1-2".into(), Some("shop".into()));
        assert_eq!(session.identity().name, "Agent");
        session.met(&Implementation::new("claude-code", "2.1.0"));
        session.met(&Implementation::new("codex-mcp-client", "0.40"));
        assert_eq!(
            session.identity(),
            AgentIdentity {
                id: "mcp-1-2".into(),
                name: "Claude Code".into(),
                label: Some("shop".into()),
                version: Some("2.1.0".into()),
            }
        );
    }

    #[test]
    fn each_process_has_its_own_id() {
        let session = Session::new();
        assert!(
            session
                .identity()
                .id
                .starts_with(&format!("mcp-{}-", std::process::id()))
        );
    }
}
