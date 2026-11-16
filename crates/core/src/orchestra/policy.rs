//! The runtime's rules, enforced in Rust whatever an agent's prompt says: what a task's
//! permission mode lets a tool do, who may see, message and delegate to whom, and the limits on
//! delegation chains (depth, cycles, outstanding work, sizes).

use super::ids::AgentId;
use super::model::{AgentProfile, PermissionMode};
use super::{Limits, OrchestraError, Result};

/// What to do with a tool use a provider asks about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Reject,
    /// Ask the IDE's permission gate.
    Ask,
}

/// Tool kinds (ACP's `ToolKind`) that only look.
const LOOKING: &[&str] = &["read", "search", "fetch", "think"];

/// What `mode` says of a tool use of `kind`.
pub fn permission(mode: PermissionMode, kind: &str) -> Verdict {
    let looks = LOOKING.contains(&kind);
    match mode {
        PermissionMode::Deny => Verdict::Reject,
        PermissionMode::ReadOnly if looks => Verdict::Allow,
        PermissionMode::ReadOnly => Verdict::Reject,
        PermissionMode::Ask if looks => Verdict::Allow,
        PermissionMode::Ask => Verdict::Ask,
        PermissionMode::Allow => Verdict::Allow,
    }
}

/// The mode a delegated task runs in: never more than the task that asked for it had, nor more
/// than its own agent is given.
pub fn delegated_mode(parent: PermissionMode, recipient: &AgentProfile) -> PermissionMode {
    parent.min(recipient.permission)
}

/// Whether `caller` (`None`: the user) may see `target` in `list_agents`.
pub fn can_discover(caller: Option<&AgentProfile>, target: &AgentProfile) -> bool {
    match caller {
        None => true,
        Some(c) if c.id == target.id => false,
        Some(c) => {
            target.comms.discoverable
                && (c.workspace == target.workspace || target.comms.names(&c.id))
        }
    }
}

/// Whether `sender` may send `recipient` a message. The user may always.
pub fn check_contact(sender: Option<&AgentProfile>, recipient: &AgentProfile) -> Result<()> {
    if !recipient.enabled {
        return Err(OrchestraError::Denied(format!(
            "agent {} is disabled",
            recipient.id
        )));
    }
    let Some(sender) = sender else { return Ok(()) };
    if sender.id == recipient.id {
        return Err(OrchestraError::Denied(
            "an agent can't message itself".into(),
        ));
    }
    if !sender.comms.may_contact(&recipient.id) {
        return Err(OrchestraError::Denied(format!(
            "{} may not contact {}",
            sender.id, recipient.id
        )));
    }
    if !recipient.comms.accepts(&sender.id) {
        return Err(OrchestraError::Denied(format!(
            "{} doesn't take messages from {}",
            recipient.id, sender.id
        )));
    }
    if sender.workspace != recipient.workspace && !recipient.comms.names(&sender.id) {
        return Err(OrchestraError::Denied(format!(
            "{} works in another workspace and doesn't name {} on its accept list",
            recipient.id, sender.id
        )));
    }
    Ok(())
}

/// Whether `sender` may hand `recipient` a task.
pub fn check_delegate(sender: Option<&AgentProfile>, recipient: &AgentProfile) -> Result<()> {
    if let Some(s) = sender
        && !s.comms.may_delegate
    {
        return Err(OrchestraError::Denied(format!(
            "{} may not delegate tasks",
            s.id
        )));
    }
    check_contact(sender, recipient)
}

/// Checks a delegation from a task at `parent_depth`, whose chain of agents (the root task's
/// first, the caller's last) is `chain`, to `recipient`: depth, cycles and outstanding work.
pub fn check_chain(
    limits: &Limits,
    chain: &[AgentId],
    parent_depth: u32,
    open_under_root: u32,
    recipient: &AgentId,
) -> Result<()> {
    if parent_depth + 1 > limits.max_delegation_depth {
        return Err(OrchestraError::Limit(format!(
            "delegation depth {} is over the limit of {}",
            parent_depth + 1,
            limits.max_delegation_depth
        )));
    }
    if chain.contains(recipient) {
        return Err(OrchestraError::Denied(format!(
            "{recipient} is already working on this request higher up the chain: delegating back would make a cycle"
        )));
    }
    if open_under_root >= limits.max_outstanding_per_root {
        return Err(OrchestraError::Limit(format!(
            "this request already has {open_under_root} unfinished delegated tasks (limit {})",
            limits.max_outstanding_per_root
        )));
    }
    Ok(())
}

/// Checks a text against the size limit.
pub fn check_size(limits: &Limits, what: &str, text: &str) -> Result<()> {
    if text.len() > limits.max_message_bytes {
        return Err(OrchestraError::Limit(format!(
            "{what} is {} bytes; the limit is {}",
            text.len(),
            limits.max_message_bytes
        )));
    }
    if text.trim().is_empty() {
        return Err(OrchestraError::Invalid(format!("{what} is empty")));
    }
    Ok(())
}
