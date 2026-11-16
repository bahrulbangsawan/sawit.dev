//! Agent management: the registry (create, update, delete, checks, after a restart) and the
//! lifecycle (valid and invalid moves, racing requests), through the runtime too.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use super::store_tests::providers;
use super::support::{Scripted, agent, echo, runtime, temp};
use crate::orchestra::lifecycle::Lifecycle;
use crate::orchestra::model::{AgentState, Launch};
use crate::orchestra::registry;
use crate::orchestra::store::Store;
use crate::orchestra::{AgentId, Limits, NewAgent, OrchestraError, SubmitTask, TaskStatus};

#[test]
fn every_lifecycle_move_is_checked() {
    use AgentState::*;
    let all = [
        Registered, Starting, Ready, Busy, Paused, Stopping, Stopped, Failed,
    ];
    let allowed = [
        (Registered, Starting),
        (Stopped, Starting),
        (Failed, Starting),
        (Starting, Ready),
        (Starting, Failed),
        (Starting, Stopping),
        (Ready, Busy),
        (Ready, Paused),
        (Ready, Stopping),
        (Ready, Failed),
        (Busy, Ready),
        (Busy, Paused),
        (Busy, Stopping),
        (Busy, Failed),
        (Paused, Ready),
        (Paused, Busy),
        (Paused, Stopping),
        (Paused, Failed),
        (Stopping, Stopped),
        (Stopping, Failed),
    ];
    for from in all {
        for to in all {
            assert_eq!(
                from.can_go(to),
                allowed.contains(&(from, to)),
                "{from:?} -> {to:?}"
            );
        }
    }
    let l = Lifecycle::default();
    let id = AgentId::from("a");
    assert!(matches!(
        l.transition(&id, Ready, None),
        Err(OrchestraError::Conflict(_))
    ));
    assert_eq!(l.state(&id), Registered);
}

#[test]
fn racing_starts_let_exactly_one_through() {
    let l = Arc::new(Lifecycle::default());
    let id = AgentId::from("a");
    let wins: usize = (0..16)
        .map(|_| {
            let (l, id) = (l.clone(), id.clone());
            std::thread::spawn(move || {
                l.transition(&id, AgentState::Starting, None)
                    .is_ok_and(|from| from != AgentState::Starting)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap() as usize)
        .sum();
    assert_eq!(wins, 1);
}

#[test]
fn the_registry_creates_updates_and_deletes_and_keeps_agents_across_restarts() {
    let (_d, dir) = temp();
    let path = dir.join("o.sqlite");
    let store = Store::open(&path).unwrap();
    let p = registry::create(&store, agent("Backend Agent", &dir), &providers()).unwrap();
    assert_eq!(p.id.as_str(), "backend-agent");
    let dup = registry::create(&store, agent("Backend Agent", &dir), &providers()).unwrap_err();
    assert!(matches!(dup, OrchestraError::Conflict(_)));
    let p2 = registry::update(&store, "backend-agent", &providers(), |p| {
        p.description = "builds APIs".into();
        p.id = AgentId::from("hijacked");
    })
    .unwrap();
    assert_eq!(p2.id, p.id, "an update never changes the id");
    assert_eq!(p2.description, "builds APIs");
    drop(store);
    let store = Store::open(&path).unwrap();
    let back = store
        .with(|c| crate::orchestra::store::agents::get_agent(c, "Backend Agent"))
        .unwrap();
    assert_eq!(back.profile.description, "builds APIs");
    registry::delete(&store, "backend-agent").unwrap();
    assert!(matches!(
        store.with(|c| crate::orchestra::store::agents::get_agent(c, "backend-agent")),
        Err(OrchestraError::NotFound(_))
    ));
    // A deleted agent's id isn't given to a new one (its history names it).
    assert!(registry::create(&store, agent("Backend Agent", &dir), &providers()).is_err());
}

#[test]
fn profiles_are_checked() {
    let (_d, dir) = temp();
    let store = Store::open_in_memory().unwrap();
    let bad = |n: NewAgent| {
        registry::create(&store, n, &providers())
            .unwrap_err()
            .to_string()
    };
    let mut relative = agent("a", &dir);
    relative.workdir = "relative/dir".into();
    assert!(bad(relative).contains("absolute"));
    let mut shell = agent("b", &dir);
    shell.provider = Some("acp".into());
    shell.launch = Some(Launch {
        command: "npx opencode acp".into(),
        ..Default::default()
    });
    assert!(bad(shell).contains("program name"));
    let mut none = agent("c", &dir);
    none.provider = Some("acp".into());
    assert!(bad(none).contains("program"));
    let mut unknown = agent("d", &dir);
    unknown.provider = Some("nope".into());
    assert!(bad(unknown).contains("no provider"));
    let mut preset = agent("e", &dir);
    preset.provider = Some("acp".into());
    preset.preset = Some("opencode".into());
    let p = registry::create(&store, preset, &providers()).unwrap();
    assert_eq!(p.launch.unwrap().args, vec!["acp".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn agents_start_lazily_and_stop_pause_and_resume() {
    let (_d, dir) = temp();
    let provider = Scripted::new(false, echo());
    let o = runtime(&dir, provider.clone(), Limits::default());
    let p = o.create_agent(agent("w", &dir)).await.unwrap();
    assert_eq!(o.agent("w").await.unwrap().state, AgentState::Registered);
    assert_eq!(
        provider.counters.starts.load(Ordering::SeqCst),
        0,
        "nothing starts before work"
    );
    let t = o
        .submit(SubmitTask {
            agent: "w".into(),
            text: "hello".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(o.wait_idle(Duration::from_secs(5)).await);
    assert_eq!(o.task(&t.id).await.unwrap().status, TaskStatus::Completed);
    assert_eq!(provider.counters.starts.load(Ordering::SeqCst), 1);
    assert_eq!(o.agent("w").await.unwrap().state, AgentState::Ready);
    o.pause_agent("w").await.unwrap();
    let held = o
        .submit(SubmitTask {
            agent: "w".into(),
            text: "later".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        o.task(&held.id).await.unwrap().status,
        TaskStatus::Queued,
        "a paused agent takes no work"
    );
    o.resume_agent("w").await.unwrap();
    assert!(o.wait_idle(Duration::from_secs(5)).await);
    o.stop_agent("w", false).await.unwrap();
    assert_eq!(o.agent("w").await.unwrap().state, AgentState::Stopped);
    assert!(o.live_agents().is_empty());
    o.start_agent("w").await.unwrap();
    assert_eq!(o.agent("w").await.unwrap().state, AgentState::Ready);
    o.set_enabled("w", false).await.unwrap();
    assert!(
        o.submit(SubmitTask {
            agent: "w".into(),
            text: "no".into(),
            ..Default::default()
        })
        .await
        .is_err()
    );
    o.delete_agent(p.id.as_str()).await.unwrap();
    assert!(o.agents().await.unwrap().is_empty());
    o.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_start_marks_the_agent_failed_and_the_task_is_retried() {
    let (_d, dir) = temp();
    let provider = Scripted::new(false, echo());
    provider.counters.fail_starts.store(1, Ordering::SeqCst);
    let o = runtime(&dir, provider.clone(), Limits::default());
    o.create_agent(agent("w", &dir)).await.unwrap();
    let t = o
        .submit(SubmitTask {
            agent: "w".into(),
            text: "x".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    super::support::until("the start failure", async || {
        o.agent("w").await.unwrap().state == AgentState::Failed
    })
    .await;
    let task = o.task(&t.id).await.unwrap();
    assert_eq!(task.status, TaskStatus::Queued);
    assert_eq!(task.failure.as_deref(), Some("provider_start"));
    // The cool-down is 10 s; a start asked for by hand goes through at once.
    o.start_agent("w").await.unwrap();
    assert!(o.wait_idle(Duration::from_secs(10)).await);
    assert_eq!(o.task(&t.id).await.unwrap().status, TaskStatus::Completed);
    o.shutdown().await;
}
