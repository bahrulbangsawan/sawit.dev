//! The drain scheduler. One process schedules at a time: the one holding the store's lease
//! (renewed while it lives, so a second app or a crashed one never runs work twice). When it
//! takes the lease it first recovers what a previous run left. Then, on every wake (new work
//! stored here, a turn ending) and every poll (work stored by `sawit-mcp`), it drains: it walks
//! the enabled agents round-robin, one claim per agent per pass so no agent starves another,
//! and claims a task for each agent that has room (its own limit, its provider's concurrency,
//! its folder free, a global permit), until a pass claims nothing. Each claim is an atomic
//! store transaction; each claimed task runs on its own Tokio task holding a permit of the
//! bounded semaphore, so running work never exceeds [`super::Limits::max_running`].

use std::sync::Arc;
use std::time::Duration;

use super::events::RuntimeEvent;
use super::exec;
use super::ids::now_ms;
use super::model::{AgentState, Isolation};
use super::runtime::Inner;
use super::store::{agents, tasks};

/// The lease's name in the store, and how long it lasts unless renewed.
const LEASE: &str = "scheduler";
const LEASE_TTL_MS: i64 = 20_000;

pub(crate) async fn run(inner: Arc<Inner>) {
    let mut holding = false;
    loop {
        let owner = inner.config.owner.clone();
        let held = inner
            .store
            .run(move |s| s.with(|c| agents::take_lease(c, LEASE, &owner, now_ms(), LEASE_TTL_MS)))
            .await
            .unwrap_or_else(|e| {
                log::warn!(target: "orchestra", "the scheduler lease could not be taken: {e}");
                false
            });
        if held && !holding {
            recover(&inner).await;
        }
        holding = held;
        if held {
            signal_cancellations(&inner).await;
            drain(&inner).await;
            reap_idle(&inner).await;
        }
        tokio::select! {
            _ = inner.wake.notified() => {}
            _ = tokio::time::sleep(inner.config.poll_interval) => {}
            _ = inner.shutdown.cancelled() => break,
        }
    }
    let owner = inner.config.owner.clone();
    let _ = inner
        .store
        .run(move |s| s.with(|c| agents::release_lease(c, LEASE, &owner)))
        .await;
}

async fn recover(inner: &Arc<Inner>) {
    match inner.store.run(|s| s.tx(|c| tasks::recover(c))).await {
        Ok(r) => {
            if !(r.requeued.is_empty() && r.retried.is_empty() && r.interrupted.is_empty()) {
                log::info!(
                    target: "orchestra",
                    "recovered: {} requeued, {} retried, {} held for review, {} deliveries pending again",
                    r.requeued.len(), r.retried.len(), r.interrupted.len(), r.deliveries
                );
            }
            inner.publish(vec![RuntimeEvent::Recovered {
                requeued: r.requeued.len(),
                retried: r.retried.len(),
                interrupted: r.interrupted.len(),
            }]);
        }
        Err(e) => log::warn!(target: "orchestra", "recovery failed: {e}"),
    }
}

/// Cancellations asked for in the store (by `sawit-mcp`, or before a restart) reach the running
/// turns' tokens.
async fn signal_cancellations(inner: &Arc<Inner>) {
    let Ok(ids) = inner.store.run(|s| s.with(tasks::cancel_requested)).await else {
        return;
    };
    for id in ids {
        inner.signal(&id);
    }
}

/// Claims and starts work until a whole pass over the agents claims nothing.
pub(crate) async fn drain(inner: &Arc<Inner>) {
    loop {
        if inner.shutdown.is_cancelled() {
            return;
        }
        let Ok(rows) = inner.store.run(|s| s.with(agents::list_agents)).await else {
            return;
        };
        let mut rows: Vec<_> = rows.into_iter().filter(|r| r.profile.enabled).collect();
        if rows.is_empty() {
            return;
        }
        // Start after the agent served last, so every agent with work gets its turn.
        let last = inner
            .last_served
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(at) = last.and_then(|l| rows.iter().position(|r| r.profile.id == l)) {
            rows.rotate_left(at + 1);
        }
        let mut claimed = false;
        for row in rows {
            let p = row.profile;
            let state = inner.lifecycle.state(&p.id);
            if matches!(state, AgentState::Paused | AgentState::Stopping)
                || !inner.lifecycle.may_start(&p.id)
            {
                continue;
            }
            let Some(provider) = inner.providers.get(&p.provider) else {
                continue;
            };
            let concurrent = inner.lifecycle.live_runtime(&p.id).map_or_else(
                || provider.capabilities().concurrent_turns,
                |r| r.capabilities().concurrent_turns,
            );
            let room = if concurrent {
                p.max_concurrent_tasks as usize
            } else {
                1
            };
            if inner.running_for(&p.id) >= room {
                continue;
            }
            if inner.lifecycle.live_runtime(&p.id).is_none()
                && inner.lifecycle.live_count() >= inner.config.limits.max_live_agents
            {
                // Room for another provider process only once an idle one is stopped.
                reap_lru(inner).await;
                if inner.lifecycle.live_count() >= inner.config.limits.max_live_agents {
                    continue;
                }
            }
            let folder = match p.isolation {
                Isolation::Shared => match inner.folders.try_take(&p.workdir) {
                    Some(g) => Some(g),
                    None => continue,
                },
                Isolation::Worktree | Isolation::Free => None,
            };
            let Ok(permit) = inner.permits.clone().try_acquire_owned() else {
                return;
            };
            let (agent, owner) = (p.id.clone(), inner.config.owner.clone());
            let claim = inner
                .store
                .run(move |s| s.tx(|c| tasks::claim_next(c, &agent, &owner, now_ms())))
                .await;
            let task = match claim {
                Ok(Some(t)) => t,
                Ok(None) => continue,
                Err(e) => {
                    log::warn!(target: "orchestra", "claiming work for {} failed: {e}", p.id);
                    continue;
                }
            };
            claimed = true;
            *inner.last_served.lock().unwrap_or_else(|p| p.into_inner()) = Some(p.id.clone());
            let cancel = inner.track(&task);
            let inner2 = inner.clone();
            tokio::spawn(async move {
                let id = task.id.clone();
                exec::run(inner2.clone(), task, cancel).await;
                // Capacity first, then the wake that uses it.
                drop(folder);
                drop(permit);
                inner2.finished(&id);
            });
        }
        if !claimed {
            return;
        }
    }
}

/// Stops provider processes idle for longer than the limit.
async fn reap_idle(inner: &Arc<Inner>) {
    for id in inner.lifecycle.idle(inner.config.limits.idle_timeout) {
        inner.stop_runtime(&id, "idle").await;
    }
}

/// Stops the least recently used idle provider process, to make room.
async fn reap_lru(inner: &Arc<Inner>) {
    if let Some(id) = inner.lifecycle.idle(Duration::ZERO).into_iter().next() {
        inner
            .stop_runtime(&id, "making room for another agent")
            .await;
    }
}
