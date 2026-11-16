//! The runtime's API: agents (register, change, enable, start, stop, pause, delete, look up) and
//! tasks (submit, cancel, hold, release, requeue after review, look up). Store work runs on the
//! blocking pool; nothing here blocks an async worker.

use std::time::Duration;

use serde::Serialize;

use super::Orchestra;
use crate::orchestra::comms::{Caller, CancelReport, SubmitTask, TaskView};
use crate::orchestra::events::RuntimeEvent;
use crate::orchestra::ids::{AgentId, TaskId};
use crate::orchestra::model::{AgentMessage, AgentProfile, AgentState, Task, TaskStatus};
use crate::orchestra::provider::ProviderCapabilities;
use crate::orchestra::registry::{self, NewAgent};
use crate::orchestra::store::agents::ThreadRow;
use crate::orchestra::store::tasks::TaskFilter;
use crate::orchestra::store::{Store, agents, mail, tasks};
use crate::orchestra::{OrchestraError, Result};

/// An agent and the state of its runtime in this process.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AgentStatus {
    pub profile: AgentProfile,
    pub state: AgentState,
    pub detail: Option<String>,
    pub running_tasks: usize,
}

impl Orchestra {
    async fn store_run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&Store) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        self.inner.store.run(f).await
    }

    /// The provider ids agents may name here.
    pub fn provider_ids(&self) -> Vec<String> {
        self.inner.providers.ids()
    }

    pub async fn create_agent(&self, new: NewAgent) -> Result<AgentProfile> {
        let providers = self.provider_ids();
        let p = self
            .store_run(move |s| registry::create(s, new, &providers))
            .await?;
        self.inner.publish(vec![RuntimeEvent::AgentCreated {
            agent: p.id.clone(),
        }]);
        Ok(p)
    }

    /// Changes an agent's profile. A running provider is stopped when what it was started with
    /// changed; the next task starts it again.
    pub async fn update_agent(
        &self,
        key: &str,
        change: impl FnOnce(&mut AgentProfile) + Send + 'static,
    ) -> Result<AgentProfile> {
        let (providers, key) = (self.provider_ids(), key.to_string());
        let before = self.agent(&key).await?.profile;
        let after = self
            .store_run(move |s| registry::update(s, &key, &providers, change))
            .await?;
        let restart = before.launch != after.launch
            || before.workdir != after.workdir
            || before.provider != after.provider
            || before.isolation != after.isolation;
        if (restart || !after.enabled) && self.inner.running_for(&after.id) == 0 {
            self.inner
                .stop_runtime(&after.id, "its settings changed")
                .await;
        }
        self.inner.publish(vec![RuntimeEvent::AgentUpdated {
            agent: after.id.clone(),
        }]);
        self.wake();
        Ok(after)
    }

    pub async fn set_enabled(&self, key: &str, enabled: bool) -> Result<AgentProfile> {
        self.update_agent(key, move |p| p.enabled = enabled).await
    }

    /// Deletes an agent with no unfinished tasks. Its history stays.
    pub async fn delete_agent(&self, key: &str) -> Result<AgentId> {
        let id = self.agent(key).await?.profile.id;
        if self.inner.running_for(&id) > 0 {
            return Err(OrchestraError::Conflict(format!(
                "agent {id} is running a task"
            )));
        }
        let key = key.to_string();
        let id = self.store_run(move |s| registry::delete(s, &key)).await?;
        self.inner.stop_runtime(&id, "deleted").await;
        self.inner.lifecycle.forget(&id);
        self.inner
            .publish(vec![RuntimeEvent::AgentDeleted { agent: id.clone() }]);
        Ok(id)
    }

    fn status_of(&self, profile: AgentProfile) -> AgentStatus {
        let id = profile.id.clone();
        AgentStatus {
            state: self.inner.lifecycle.state(&id),
            detail: self.inner.lifecycle.detail(&id),
            running_tasks: self.inner.running_for(&id),
            profile,
        }
    }

    pub async fn agents(&self) -> Result<Vec<AgentStatus>> {
        let rows = self.store_run(|s| s.with(agents::list_agents)).await?;
        Ok(rows
            .into_iter()
            .map(|r| self.status_of(r.profile))
            .collect())
    }

    /// The agent `key` names (id or name).
    pub async fn agent(&self, key: &str) -> Result<AgentStatus> {
        let key = key.to_string();
        let row = self
            .store_run(move |s| s.with(|c| agents::get_agent(c, &key)))
            .await?;
        Ok(self.status_of(row.profile))
    }

    /// What the agent's provider can do: as negotiated when it runs, else the adapter's most.
    pub async fn agent_capabilities(&self, key: &str) -> Result<ProviderCapabilities> {
        let p = self.agent(key).await?.profile;
        if let Some(rt) = self.inner.lifecycle.live_runtime(&p.id) {
            return Ok(rt.capabilities());
        }
        self.inner
            .providers
            .get(&p.provider)
            .map(|pr| pr.capabilities())
            .ok_or_else(|| OrchestraError::Provider(format!("no provider `{}`", p.provider)))
    }

    pub async fn agent_threads(&self, key: &str) -> Result<Vec<ThreadRow>> {
        let id = self.agent(key).await?.profile.id;
        self.store_run(move |s| s.with(|c| agents::agent_threads(c, &id)))
            .await
    }

    pub async fn agent_messages(&self, key: &str, limit: u32) -> Result<Vec<AgentMessage>> {
        let id = self.agent(key).await?.profile.id;
        self.store_run(move |s| s.with(|c| mail::agent_messages(c, &id, limit)))
            .await
    }

    /// Starts the agent's provider now, rather than when it first gets work.
    pub async fn start_agent(&self, key: &str) -> Result<AgentStatus> {
        let p = self.agent(key).await?.profile;
        if !p.enabled {
            return Err(OrchestraError::Conflict(format!(
                "agent {} is disabled",
                p.id
            )));
        }
        self.inner.lifecycle.set_retry_after(&p.id, None);
        self.inner.ensure_started(&p).await?;
        self.agent(key).await
    }

    /// Stops the agent's provider. With turns running it refuses unless `force`: those turns
    /// then end as interrupted (requeued when that is safe, else held for review).
    pub async fn stop_agent(&self, key: &str, force: bool) -> Result<AgentStatus> {
        let id = self.agent(key).await?.profile.id;
        if !force && self.inner.running_for(&id) > 0 {
            return Err(OrchestraError::Conflict(format!(
                "agent {id} is running a task; stop it with force, or cancel the task"
            )));
        }
        self.inner.stop_runtime(&id, "stopped").await;
        self.agent(key).await
    }

    pub async fn restart_agent(&self, key: &str) -> Result<AgentStatus> {
        self.stop_agent(key, false).await?;
        self.start_agent(key).await
    }

    /// The agent keeps its provider but takes no new work.
    pub async fn pause_agent(&self, key: &str) -> Result<AgentStatus> {
        let id = self.agent(key).await?.profile.id;
        if !self.inner.lifecycle.state(&id).is_running() {
            return Err(OrchestraError::Conflict(format!(
                "agent {id} isn't running; disable it to keep it from taking work"
            )));
        }
        self.inner.transition(&id, AgentState::Paused, None)?;
        self.agent(key).await
    }

    pub async fn resume_agent(&self, key: &str) -> Result<AgentStatus> {
        let id = self.agent(key).await?.profile.id;
        let to = if self.inner.running_for(&id) > 0 {
            AgentState::Busy
        } else {
            AgentState::Ready
        };
        self.inner.transition(&id, to, None)?;
        self.wake();
        self.agent(key).await
    }

    /// Hands the user's work to an agent.
    pub async fn submit(&self, req: SubmitTask) -> Result<TaskView> {
        let comms = self.comms();
        tokio::task::spawn_blocking(move || comms.submit_task(&req))
            .await
            .map_err(|e| OrchestraError::Store(e.to_string()))?
    }

    /// Cancels a task and what it delegated: at once when it isn't running, by asking its
    /// provider to stop when it is.
    pub async fn cancel(&self, id: &TaskId) -> Result<CancelReport> {
        let (comms, id) = (self.comms(), id.clone());
        let report = tokio::task::spawn_blocking(move || comms.cancel_task(&Caller::user(), &id))
            .await
            .map_err(|e| OrchestraError::Store(e.to_string()))??;
        for t in &report.stopping {
            self.inner.signal(t);
        }
        Ok(report)
    }

    async fn move_task(
        &self,
        id: &TaskId,
        from: &'static [TaskStatus],
        to: TaskStatus,
        note: &'static str,
    ) -> Result<Task> {
        let id = id.clone();
        let task = self
            .store_run(move |s| {
                s.tx(|c| {
                    let t = tasks::transition(c, &id, from, to, Some(note))?;
                    if to == TaskStatus::Queued {
                        // A reviewed task gets one more attempt; its old error stays in history.
                        c.execute(
                            "UPDATE tasks SET not_before = NULL, prompt_sent_at = NULL,
                                max_attempts = max(max_attempts, attempts + 1) WHERE id = ?1",
                            [id.as_str()],
                        )?;
                    }
                    let _ = t;
                    tasks::get_task(c, &id)
                })
            })
            .await?;
        self.inner.publish(vec![RuntimeEvent::TaskStatusChanged {
            task: task.id.clone(),
            agent: task.agent_id.clone(),
            status: task.status,
        }]);
        self.wake();
        Ok(task)
    }

    /// Keeps a queued task from running until it is released.
    pub async fn hold(&self, id: &TaskId) -> Result<Task> {
        self.move_task(id, &[TaskStatus::Queued], TaskStatus::Held, "held")
            .await
    }

    pub async fn release(&self, id: &TaskId) -> Result<Task> {
        self.move_task(id, &[TaskStatus::Held], TaskStatus::Queued, "released")
            .await
    }

    /// Runs again a task held for review after an interruption.
    pub async fn requeue(&self, id: &TaskId) -> Result<Task> {
        self.move_task(
            id,
            &[TaskStatus::Interrupted],
            TaskStatus::Queued,
            "reviewed and requeued",
        )
        .await
    }

    pub async fn task(&self, id: &TaskId) -> Result<Task> {
        let id = id.clone();
        self.store_run(move |s| s.with(|c| tasks::get_task(c, &id)))
            .await
    }

    pub async fn tasks(&self, filter: TaskFilter) -> Result<Vec<Task>> {
        self.store_run(move |s| s.with(|c| tasks::list_tasks(c, &filter)))
            .await
    }

    /// Waits until no task is queued, running or waiting (or `timeout` passes); true when idle.
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let open = self
                .store_run(|s| {
                    s.with(|c| {
                        Ok(c.query_row(
                            "SELECT count(*) FROM tasks WHERE status IN ('queued', 'starting', 'running', 'waiting')",
                            [],
                            |r| r.get::<_, i64>(0),
                        )?)
                    })
                })
                .await
                .unwrap_or(1);
            if open == 0 && self.inner.running().is_empty() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            self.wake();
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    /// The agent ids with a live provider process.
    pub fn live_agents(&self) -> Vec<AgentId> {
        self.inner.lifecycle.with_runtime()
    }
}
