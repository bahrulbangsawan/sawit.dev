//! The tools that look at tasks and cancel them, with who may see and act on which task.

use super::ops::CancelReport;
use super::views::TaskView;
use super::{Caller, Comms};
use crate::orchestra::flow;
use crate::orchestra::ids::TaskId;
use crate::orchestra::model::{Task, TaskStatus};
use crate::orchestra::store::tasks::TaskFilter;
use crate::orchestra::store::{agents, tasks};
use crate::orchestra::{OrchestraError, Result};

impl Comms {
    /// Whether `caller` may see or act on `task`: the user always; an agent when the task is
    /// its own, it asked for it, or it is part of the request the agent is working on.
    fn may_see(c: &rusqlite::Connection, caller: &Caller, task: &Task) -> Result<bool> {
        let Some(me) = &caller.agent else {
            return Ok(true);
        };
        if &task.agent_id == me || task.requested_by.as_ref() == Some(me) {
            return Ok(true);
        }
        let mine = Self::current_task(c, caller, None)?;
        Ok(mine.is_some_and(|m| m.root_task_id == task.root_task_id && m.depth < task.depth))
    }

    pub fn get_task_status(&self, caller: &Caller, id: &TaskId) -> Result<TaskView> {
        self.store.with(|c| {
            let task = tasks::get_task(c, id)?;
            if !Self::may_see(c, caller, &task)? {
                return Err(OrchestraError::Denied(format!(
                    "task {id} isn't yours to see"
                )));
            }
            Ok(TaskView::new(&task, tasks::open_children(c, id)?))
        })
    }

    pub fn list_agent_tasks(
        &self,
        caller: &Caller,
        agent: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<TaskView>> {
        let statuses = match status {
            Some(s) => vec![
                TaskStatus::parse(s)
                    .ok_or_else(|| OrchestraError::Invalid(format!("no status `{s}`")))?,
            ],
            None => Vec::new(),
        };
        self.store.with(|c| {
            let agent = agent
                .map(|a| agents::get_agent(c, a).map(|r| r.profile.id))
                .transpose()?;
            let mut filter = TaskFilter {
                agent: agent.clone().or(caller.agent.clone()),
                statuses,
                limit: Some(100),
                ..Default::default()
            };
            if let Some(me) = &caller.agent
                && filter.agent.as_ref() != Some(me)
            {
                // Another agent's tasks: only those this agent asked for.
                filter.requested_by = Some(me.clone());
            }
            tasks::list_tasks(c, &filter)?
                .iter()
                .map(|t| Ok(TaskView::new(t, tasks::open_children(c, &t.id)?)))
                .collect()
        })
    }

    pub fn cancel_task(&self, caller: &Caller, id: &TaskId) -> Result<CancelReport> {
        let (report, events) = self.store.tx(|c| {
            let task = tasks::get_task(c, id)?;
            let allowed = match &caller.agent {
                None => true,
                Some(me) => {
                    task.requested_by.as_ref() == Some(me)
                        || Self::may_see(c, caller, &task)? && &task.agent_id != me
                }
            };
            if !allowed {
                return Err(OrchestraError::Denied(format!(
                    "task {id} wasn't asked for by you"
                )));
            }
            let done = flow::cancel_tree(c, id, &self.limits)?;
            Ok((
                CancelReport {
                    cancelled: done.cancelled,
                    stopping: done.signalled,
                },
                done.events,
            ))
        })?;
        self.after(events);
        Ok(report)
    }
}
