//! The scheduler: concurrency limits, providers that take one turn at a time, fairness, claims
//! across two runtimes, cancellation (queued and running), failures, retries, timeouts and
//! recovery after a crash.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::support::{Script, Scripted, Turn, agent, done, echo, runtime, temp};
use crate::orchestra::model::PermissionMode;
use crate::orchestra::provider::{ProviderError, StopReason, TurnOutcome};
use crate::orchestra::store::tasks::TaskFilter;
use crate::orchestra::{Limits, SubmitTask, TaskStatus};

fn sleepy(ms: u64) -> Script {
    Arc::new(move |_t: Turn| {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(ms)).await;
            Ok(done("ok"))
        })
    })
}

fn submit(agent: &str, text: &str) -> SubmitTask {
    SubmitTask {
        agent: agent.into(),
        text: text.into(),
        new_thread: true,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_in_a_shared_folder_take_turns_in_it() {
    let (_d, dir) = temp();
    let provider = Scripted::new(true, sleepy(40));
    let o = runtime(&dir, provider.clone(), Limits::default());
    let mut a = agent("w", &dir);
    a.max_concurrent_tasks = Some(3);
    o.create_agent(a).await.unwrap();
    for i in 0..4 {
        o.submit(submit("w", &format!("t{i}"))).await.unwrap();
    }
    assert!(o.wait_idle(Duration::from_secs(10)).await);
    assert_eq!(provider.counters.max_active.load(Ordering::SeqCst), 1);
    assert_eq!(provider.counters.turns.load(Ordering::SeqCst), 4);
    o.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrency_follows_the_agent_limit_and_the_provider() {
    for (concurrent, limit, expect) in [(true, 3, 3), (false, 3, 1)] {
        let (_d, dir) = temp();
        let provider = Scripted::new(concurrent, sleepy(80));
        let o = runtime(&dir, provider.clone(), Limits::default());
        let mut a = agent("w", &dir);
        a.max_concurrent_tasks = Some(limit);
        a.isolation = crate::orchestra::Isolation::Free;
        o.create_agent(a).await.unwrap();
        for i in 0..7 {
            o.submit(submit("w", &format!("t{i}"))).await.unwrap();
        }
        assert!(o.wait_idle(Duration::from_secs(10)).await);
        let seen = provider.counters.max_active.load(Ordering::SeqCst);
        assert_eq!(seen, expect, "concurrent={concurrent}: saw {seen} at once");
        o.shutdown().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_take_turns_fairly() {
    let (_d, dir) = temp();
    let provider = Scripted::new(true, sleepy(20));
    let limits = Limits {
        max_running: 1,
        ..Default::default()
    };
    let o = runtime(&dir, provider.clone(), limits);
    o.create_agent(agent("a", &dir)).await.unwrap();
    o.create_agent(agent("b", &dir)).await.unwrap();
    for i in 0..4 {
        o.submit(submit("a", &format!("a{i}"))).await.unwrap();
    }
    o.submit(submit("b", "b0")).await.unwrap();
    assert!(o.wait_idle(Duration::from_secs(10)).await);
    let order = provider.counters.order.lock().unwrap().clone();
    let b_at = order.iter().position(|(a, _)| a.as_str() == "b").unwrap();
    assert!(b_at <= 2, "b waited behind a's whole queue: {order:?}");
    o.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_runtimes_on_one_store_run_each_task_once() {
    let (_d, dir) = temp();
    let provider = Scripted::new(true, sleepy(5));
    let first = runtime(&dir, provider.clone(), Limits::default());
    let second = runtime(&dir, provider.clone(), Limits::default());
    for n in ["a", "b", "c"] {
        first.create_agent(agent(n, &dir)).await.unwrap();
    }
    for i in 0..15 {
        second
            .submit(submit(["a", "b", "c"][i % 3], &format!("t{i}")))
            .await
            .unwrap();
    }
    assert!(first.wait_idle(Duration::from_secs(15)).await);
    let per_task = provider.counters.per_task.lock().unwrap().clone();
    assert_eq!(per_task.len(), 15);
    assert!(per_task.values().all(|n| *n == 1), "{per_task:?}");
    first.shutdown().await;
    second.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_task_is_cancelled_before_it_runs_and_while_it_runs() {
    let (_d, dir) = temp();
    let waits: Script = Arc::new(|t: Turn| {
        Box::pin(async move {
            t.ctx.cancel.cancelled().await;
            Ok(TurnOutcome {
                stop: StopReason::Cancelled,
                text: "stopped".into(),
            })
        })
    });
    let provider = Scripted::new(true, waits);
    let o = runtime(&dir, provider.clone(), Limits::default());
    o.create_agent(agent("w", &dir)).await.unwrap();
    let held = o
        .submit(SubmitTask {
            held: true,
            ..submit("w", "held")
        })
        .await
        .unwrap();
    let report = o.cancel(&held.id).await.unwrap();
    assert_eq!(report.cancelled, vec![held.id.clone()]);
    assert_eq!(
        o.task(&held.id).await.unwrap().status,
        TaskStatus::Cancelled
    );
    assert_eq!(provider.counters.turns.load(Ordering::SeqCst), 0);
    let running = o.submit(submit("w", "long")).await.unwrap();
    super::support::until("the turn to start", async || {
        provider.counters.active.load(Ordering::SeqCst) == 1
    })
    .await;
    let report = o.cancel(&running.id).await.unwrap();
    assert_eq!(report.stopping, vec![running.id.clone()]);
    assert!(o.wait_idle(Duration::from_secs(5)).await);
    let t = o.task(&running.id).await.unwrap();
    assert_eq!(
        t.status,
        TaskStatus::Cancelled,
        "never completed after a cancellation"
    );
    o.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn failures_fail_retries_retry_and_timeouts_are_held_or_retried() {
    let (_d, dir) = temp();
    let script: Script = Arc::new(|t: Turn| {
        Box::pin(async move {
            if t.text.contains("broken") {
                return Err(ProviderError::Protocol("model error".into()));
            }
            if t.text.contains("hang") {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
            Ok(done("fine"))
        })
    });
    let provider = Scripted::new(true, script);
    let o = runtime(&dir, provider.clone(), Limits::default());
    o.create_agent(agent("w", &dir)).await.unwrap();
    let broken = o.submit(submit("w", "broken")).await.unwrap();
    let risky = o
        .submit(SubmitTask {
            timeout_ms: Some(100),
            ..submit("w", "hang with writes")
        })
        .await
        .unwrap();
    let safe = o
        .submit(SubmitTask {
            timeout_ms: Some(100),
            max_attempts: Some(2),
            permission: Some(PermissionMode::ReadOnly),
            ..submit("w", "hang read only")
        })
        .await
        .unwrap();
    super::support::until("the outcomes", async || {
        let s = |t: crate::orchestra::Task| t.status;
        s(o.task(&broken.id).await.unwrap()) == TaskStatus::Failed
            && s(o.task(&risky.id).await.unwrap()) == TaskStatus::Interrupted
            && s(o.task(&safe.id).await.unwrap()) == TaskStatus::Failed
    })
    .await;
    let b = o.task(&broken.id).await.unwrap();
    assert_eq!(
        (b.attempts, b.failure.as_deref()),
        (1, Some("provider_error"))
    );
    assert!(b.error.unwrap().contains("model error"));
    let s = o.task(&safe.id).await.unwrap();
    assert_eq!(
        (s.attempts, s.failure.as_deref()),
        (2, Some("timeout")),
        "a read-only task is retried, then fails"
    );
    let r = o.task(&risky.id).await.unwrap();
    assert_eq!(
        (r.attempts, r.failure.as_deref()),
        (1, Some("timeout")),
        "a task that may have written is held"
    );
    o.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn work_a_crash_left_behind_is_recovered_on_the_next_start() {
    let (_d, dir) = temp();
    let path = dir.join("orchestra.sqlite");
    let (writes, unsent) = {
        // A "previous run": tasks claimed and one sent, then the process died.
        let store = crate::orchestra::store::Store::open(&path).unwrap();
        let p = crate::orchestra::registry::create(
            &store,
            agent("w", &dir),
            &super::store_tests::providers(),
        )
        .unwrap();
        let lane = |n: &str| {
            store
                .with(|c| crate::orchestra::store::agents::side_thread(c, &p.id, n))
                .unwrap()
                .id
        };
        use crate::orchestra::store::tasks;
        let (l1, l2) = (lane("1"), lane("2"));
        let w = store
            .with(|c| {
                tasks::insert_task(
                    c,
                    &super::store_tests::row(&p.id, &l1, "writes", PermissionMode::Allow),
                )
            })
            .unwrap()
            .0;
        let u = store
            .with(|c| {
                tasks::insert_task(
                    c,
                    &super::store_tests::row(&p.id, &l2, "unsent", PermissionMode::Allow),
                )
            })
            .unwrap()
            .0;
        store
            .tx(|c| tasks::claim_next(c, &p.id, "dead", 0))
            .unwrap();
        store
            .tx(|c| tasks::claim_next(c, &p.id, "dead", 0))
            .unwrap();
        store.with(|c| tasks::mark_running(c, &w.id)).unwrap();
        (w.id, u.id)
    };
    let provider = Scripted::new(true, echo());
    let o = runtime(&dir, provider.clone(), Limits::default());
    super::support::until("recovery", async || {
        o.task(&unsent).await.unwrap().status == TaskStatus::Completed
    })
    .await;
    assert_eq!(
        o.task(&writes).await.unwrap().status,
        TaskStatus::Interrupted
    );
    o.requeue(&writes).await.unwrap();
    assert!(o.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(o.task(&writes).await.unwrap().status, TaskStatus::Completed);
    let all = o.tasks(TaskFilter::default()).await.unwrap();
    assert!(all.iter().all(|t| t.status == TaskStatus::Completed));
    o.shutdown().await;
}
