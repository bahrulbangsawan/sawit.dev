//! The whole flow: an orchestrator discovers a worker, delegates, the worker runs, its result
//! comes back to the orchestrator's own thread, the orchestrator finishes, and all of it is
//! still there after a restart. Also: a restart in the middle of a turn, and cancelling a
//! request while its delegated work runs.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use serde_json::json;

use super::support::{Script, Scripted, Turn, agent, done, runtime, temp, until};
use crate::orchestra::model::{DeliveryStatus, MessagePayload, PermissionMode, TaskPayload};
use crate::orchestra::provider::{StopReason, TurnOutcome};
use crate::orchestra::store::tasks::TaskFilter;
use crate::orchestra::store::{agents, mail, tasks};
use crate::orchestra::{Limits, RuntimeEvent, SubmitTask, TaskStatus};

/// The orchestrator lists agents, delegates to the first worker, and on its second turn
/// integrates the result; the worker answers with what it did.
fn team() -> Script {
    Arc::new(|t: Turn| {
        Box::pin(async move {
            match t.agent.as_str() {
                "orchestrator" if t.text.contains("Results of the work you delegated") => {
                    let line = t
                        .text
                        .lines()
                        .find(|l| l.contains("[completed]"))
                        .unwrap_or("none")
                        .to_string();
                    Ok(done(format!("integrated {line}")))
                }
                "orchestrator" => {
                    let listed = t
                        .ctx
                        .tools
                        .call("list_agents", json!({}))
                        .await
                        .map_err(crate::orchestra::provider::ProviderError::Protocol)?;
                    let worker = listed["agents"]
                        .as_array()
                        .and_then(|a| {
                            a.iter()
                                .find(|w| w["role"] == "worker role" && w["can_delegate"] == true)
                        })
                        .and_then(|w| w["id"].as_str())
                        .unwrap_or("none")
                        .to_string();
                    let d = t
                    .ctx
                    .tools
                    .call("delegate_task", json!({"recipient": worker, "title": "Implement auth API", "instructions": "Create login and refresh endpoints."}))
                    .await
                    .map_err(crate::orchestra::provider::ProviderError::Protocol)?;
                    Ok(done(format!(
                        "handed off {}",
                        d["task_id"].as_str().unwrap_or("?")
                    )))
                }
                _ if t.text.contains("block") || t.text.contains("Block") => {
                    t.ctx.cancel.cancelled().await;
                    Ok(TurnOutcome {
                        stop: StopReason::Cancelled,
                        text: String::new(),
                    })
                }
                _ => {
                    // Long enough that the orchestrator's turn has ended and it waits.
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    Ok(done("endpoints added in src/auth.rs"))
                }
            }
        })
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_orchestrator_delegates_and_gets_the_result_back_in_its_thread_across_a_restart() {
    let (_d, dir) = temp();
    let provider = Scripted::new(true, team());
    let o = runtime(&dir, provider.clone(), Limits::default());
    let mut events = o.subscribe();
    o.create_agent(agent("orchestrator", &dir)).await.unwrap();
    let mut w = agent("worker", &dir);
    w.role = "worker role".into();
    w.permission = Some(PermissionMode::Ask);
    o.create_agent(w).await.unwrap();
    let root = o
        .submit(SubmitTask {
            agent: "orchestrator".into(),
            text: "Implement authentication".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(o.wait_idle(Duration::from_secs(10)).await);

    let parent = o.task(&root.id).await.unwrap();
    assert_eq!(parent.status, TaskStatus::Completed);
    let text = parent.result.as_ref().unwrap()["text"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(text.starts_with("integrated - task "), "{text}");
    assert!(text.contains("endpoints added in src/auth.rs"), "{text}");
    let children = o
        .tasks(TaskFilter {
            parent: Some(root.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(children.len(), 1);
    let child = &children[0];
    assert_eq!(
        (child.agent_id.as_str(), child.depth, child.status),
        ("worker", 1, TaskStatus::Completed)
    );
    assert_eq!(
        child.requested_by.as_ref().map(|a| a.as_str()),
        Some("orchestrator")
    );
    assert_eq!(
        child.permission,
        PermissionMode::Ask,
        "the lower of the two modes"
    );
    assert!(matches!(child.payload, TaskPayload::Delegated { .. }));
    // The orchestrator ran twice, both times in its main thread (one lane, one conversation).
    let order = provider.counters.order.lock().unwrap().clone();
    let runs: Vec<_> = order
        .iter()
        .filter(|(a, _)| a.as_str() == "orchestrator")
        .collect();
    assert_eq!(runs.len(), 2);
    let main = o
        .store()
        .with(|c| agents::main_thread(c, &"orchestrator".into()))
        .unwrap();
    assert_eq!(parent.thread_id, main.id);
    let history: Vec<String> = o
        .store()
        .with(|c| tasks::transitions(c, &root.id))
        .unwrap()
        .into_iter()
        .map(|t| t.1)
        .collect();
    assert_eq!(
        history,
        [
            "queued",
            "starting",
            "running",
            "waiting",
            "queued",
            "starting",
            "running",
            "completed"
        ]
    );
    // The delegation and the result are messages with deliveries, correlated.
    let msgs = o.agent_messages("orchestrator", 10).await.unwrap();
    let result = msgs
        .iter()
        .find(|m| matches!(m.payload, MessagePayload::Result { .. }))
        .unwrap();
    let asked = msgs
        .iter()
        .find(|m| matches!(m.payload, MessagePayload::Delegation { .. }))
        .unwrap();
    assert_eq!(result.correlation_id.as_ref(), Some(&asked.id));
    assert_eq!(result.conversation_id, asked.conversation_id);
    for m in [result, asked] {
        let ds = o
            .store()
            .with(|c| mail::message_deliveries(c, &m.id))
            .unwrap();
        assert!(
            ds.iter().all(|d| d.status == DeliveryStatus::Delivered),
            "{ds:?}"
        );
    }
    let mut seen = Vec::new();
    while let Ok(e) = events.try_recv() {
        seen.push(e);
    }
    assert!(
        seen.iter()
            .any(|e| matches!(e, RuntimeEvent::TaskWaiting { task, .. } if task == &root.id))
    );
    assert!(
        seen.iter()
            .any(|e| matches!(e, RuntimeEvent::TaskCompleted { task, .. } if task == &child.id))
    );
    o.shutdown().await;
    drop(o);

    // After a restart the same state is there, and nothing runs again.
    let again = runtime(&dir, provider.clone(), Limits::default());
    assert!(again.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(again.task(&root.id).await.unwrap(), parent);
    assert_eq!(again.tasks(TaskFilter::default()).await.unwrap().len(), 2);
    assert_eq!(provider.counters.turns.load(Ordering::SeqCst), 3);
    assert_eq!(again.agents().await.unwrap().len(), 2);
    again.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_mid_turn_holds_the_task_for_review() {
    let (_d, dir) = temp();
    let provider = Scripted::new(true, team());
    let o = runtime(&dir, provider.clone(), Limits::default());
    o.create_agent(agent("worker", &dir)).await.unwrap();
    let t = o
        .submit(SubmitTask {
            agent: "worker".into(),
            text: "block until stopped".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    until("the turn to start", async || {
        provider.counters.active.load(Ordering::SeqCst) == 1
    })
    .await;
    o.shutdown().await;
    drop(o);
    let again = runtime(&dir, provider.clone(), Limits::default());
    tokio::time::sleep(Duration::from_millis(200)).await;
    let task = again.task(&t.id).await.unwrap();
    assert_eq!(
        task.status,
        TaskStatus::Interrupted,
        "it could have written files: a person decides"
    );
    assert_eq!(task.failure.as_deref(), Some("shutdown"));
    again.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancelling_a_request_cancels_the_work_it_delegated() {
    let (_d, dir) = temp();
    let script: Script = Arc::new(|t: Turn| {
        Box::pin(async move {
            if t.agent.as_str() == "orchestrator" {
                t.ctx
                    .tools
                    .call(
                        "delegate_task",
                        json!({"recipient": "worker", "title": "Block", "instructions": "block"}),
                    )
                    .await
                    .ok();
                return Ok(done("waiting"));
            }
            t.ctx.cancel.cancelled().await;
            Ok(TurnOutcome {
                stop: StopReason::Cancelled,
                text: String::new(),
            })
        })
    });
    let provider = Scripted::new(true, script);
    let o = runtime(&dir, provider.clone(), Limits::default());
    o.create_agent(agent("orchestrator", &dir)).await.unwrap();
    o.create_agent(agent("worker", &dir)).await.unwrap();
    let root = o
        .submit(SubmitTask {
            agent: "orchestrator".into(),
            text: "go".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    until("the delegated turn", async || {
        o.task(&root.id).await.unwrap().status == TaskStatus::Waiting
            && provider.counters.active.load(Ordering::SeqCst) == 1
    })
    .await;
    let report = o.cancel(&root.id).await.unwrap();
    assert_eq!(report.cancelled, vec![root.id.clone()]);
    assert_eq!(report.stopping.len(), 1);
    assert!(o.wait_idle(Duration::from_secs(5)).await);
    let child = &o
        .tasks(TaskFilter {
            parent: Some(root.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap()[0];
    assert_eq!(child.status, TaskStatus::Cancelled);
    assert_eq!(
        o.task(&root.id).await.unwrap().status,
        TaskStatus::Cancelled
    );
    o.shutdown().await;
}
