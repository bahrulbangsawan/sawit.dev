//! The store: migrations, atomic claims (also across connections), FIFO lanes, idempotency and
//! recovery after a crash.

use std::collections::HashSet;

use super::support::{agent, temp};
use crate::orchestra::ids::now_ms;
use crate::orchestra::model::{DeliveryStatus, PermissionMode, TaskPayload, TaskStatus};
use crate::orchestra::registry;
use crate::orchestra::store::tasks::{self, NewTaskRow};
use crate::orchestra::store::{Store, agents, mail};
use crate::orchestra::{AgentId, TaskId, ThreadId};

pub fn providers() -> Vec<String> {
    vec!["mock".into(), "acp".into()]
}

pub fn row(agent: &AgentId, thread: &ThreadId, text: &str, mode: PermissionMode) -> NewTaskRow {
    NewTaskRow {
        id: TaskId::generate(),
        agent_id: agent.clone(),
        thread_id: thread.clone(),
        parent_task_id: None,
        root_task_id: None,
        depth: 0,
        requested_by: None,
        title: text.into(),
        payload: TaskPayload::Prompt { text: text.into() },
        permission: mode,
        max_attempts: 3,
        timeout_ms: None,
        idempotency_key: None,
        held: false,
    }
}

fn setup(store: &Store, dir: &std::path::Path) -> (AgentId, ThreadId) {
    let p = registry::create(store, agent("worker", dir), &providers()).unwrap();
    let main = store.with(|c| agents::main_thread(c, &p.id)).unwrap();
    (p.id, main.id)
}

#[test]
fn migrations_run_once_and_a_reopened_store_keeps_its_records() {
    let (_d, dir) = temp();
    let path = dir.join("o.sqlite");
    let store = Store::open(&path).unwrap();
    let (a, t) = setup(&store, &dir);
    store
        .with(|c| tasks::insert_task(c, &row(&a, &t, "one", PermissionMode::Ask)))
        .unwrap();
    drop(store);
    let again = Store::open(&path).unwrap();
    let version: i64 = again
        .with(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
        .unwrap();
    assert_eq!(version, 1);
    let listed = again
        .with(|c| tasks::list_tasks(c, &Default::default()))
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        again
            .with(|c| agents::get_agent(c, "worker"))
            .unwrap()
            .profile
            .id,
        a
    );
}

#[test]
fn a_newer_store_is_refused() {
    let (_d, dir) = temp();
    let path = dir.join("o.sqlite");
    drop(Store::open(&path).unwrap());
    let c = rusqlite::Connection::open(&path).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let err = Store::open(&path).unwrap_err().to_string();
    assert!(err.contains("newer"), "{err}");
}

#[test]
fn claims_are_fifo_within_a_thread_and_one_turn_runs_per_thread() {
    let store = Store::open_in_memory().unwrap();
    let (_d, dir) = temp();
    let (a, main) = setup(&store, &dir);
    let side = store.with(|c| agents::side_thread(c, &a, "side")).unwrap();
    let first = store
        .with(|c| tasks::insert_task(c, &row(&a, &main, "first", PermissionMode::Ask)))
        .unwrap()
        .0;
    let second = store
        .with(|c| tasks::insert_task(c, &row(&a, &main, "second", PermissionMode::Ask)))
        .unwrap()
        .0;
    let other = store
        .with(|c| tasks::insert_task(c, &row(&a, &side.id, "other lane", PermissionMode::Ask)))
        .unwrap()
        .0;
    let claim = || {
        store
            .tx(|c| tasks::claim_next(c, &a, "me", now_ms()))
            .unwrap()
    };
    assert_eq!(claim().unwrap().id, first.id);
    // The main lane is busy: the side lane's task goes next, not `second`.
    assert_eq!(claim().unwrap().id, other.id);
    assert!(claim().is_none());
    store
        .tx(|c| {
            tasks::transition(
                c,
                &first.id,
                &[TaskStatus::Starting],
                TaskStatus::Completed,
                None,
            )
        })
        .unwrap();
    assert_eq!(claim().unwrap().id, second.id);
}

#[test]
fn concurrent_schedulers_on_separate_connections_never_claim_a_task_twice() {
    let (_d, dir) = temp();
    let path = dir.join("o.sqlite");
    let store = Store::open(&path).unwrap();
    let (a, _) = setup(&store, &dir);
    for i in 0..40 {
        // Each in its own thread (lane), so all are claimable at once.
        let t = store.with(|c| agents::side_thread(c, &a, "lane")).unwrap();
        store
            .with(|c| tasks::insert_task(c, &row(&a, &t.id, &format!("t{i}"), PermissionMode::Ask)))
            .unwrap();
    }
    let workers: Vec<_> = (0..4)
        .map(|w| {
            let (path, a) = (path.clone(), a.clone());
            std::thread::spawn(move || {
                let store = Store::open(&path).unwrap();
                let mut mine = Vec::new();
                while let Some(t) = store
                    .tx(|c| tasks::claim_next(c, &a, &format!("w{w}"), now_ms()))
                    .unwrap()
                {
                    mine.push(t.id);
                }
                mine
            })
        })
        .collect();
    let all: Vec<TaskId> = workers
        .into_iter()
        .flat_map(|w| w.join().unwrap())
        .collect();
    let unique: HashSet<_> = all.iter().cloned().collect();
    assert_eq!(all.len(), 40);
    assert_eq!(unique.len(), 40);
}

#[test]
fn an_idempotency_key_returns_the_task_made_before() {
    let store = Store::open_in_memory().unwrap();
    let (_d, dir) = temp();
    let (a, t) = setup(&store, &dir);
    let mut r = row(&a, &t, "once", PermissionMode::Ask);
    r.idempotency_key = Some("k1".into());
    let (first, made) = store.with(|c| tasks::insert_task(c, &r)).unwrap();
    assert!(made);
    r.id = TaskId::generate();
    let (again, made) = store.with(|c| tasks::insert_task(c, &r)).unwrap();
    assert!(!made);
    assert_eq!(first.id, again.id);
}

#[test]
fn recovery_requeues_unsent_and_safe_work_and_holds_the_rest_for_review() {
    let store = Store::open_in_memory().unwrap();
    let (_d, dir) = temp();
    let (a, _) = setup(&store, &dir);
    let lane = |n: &str| store.with(|c| agents::side_thread(c, &a, n)).unwrap().id;
    let (l1, l2, l3) = (lane("1"), lane("2"), lane("3"));
    let unsent = store
        .with(|c| tasks::insert_task(c, &row(&a, &l1, "unsent", PermissionMode::Allow)))
        .unwrap()
        .0;
    let safe = store
        .with(|c| tasks::insert_task(c, &row(&a, &l2, "read only", PermissionMode::ReadOnly)))
        .unwrap()
        .0;
    let risky = store
        .with(|c| tasks::insert_task(c, &row(&a, &l3, "writes", PermissionMode::Allow)))
        .unwrap()
        .0;
    for _ in 0..3 {
        store
            .tx(|c| tasks::claim_next(c, &a, "dead", now_ms()))
            .unwrap()
            .unwrap();
    }
    store.with(|c| tasks::mark_running(c, &safe.id)).unwrap();
    store.with(|c| tasks::mark_running(c, &risky.id)).unwrap();
    let r = store.tx(|c| tasks::recover(c)).unwrap();
    assert_eq!(r.requeued, vec![unsent.id.clone()]);
    assert_eq!(r.retried, vec![safe.id.clone()]);
    assert_eq!(r.interrupted, vec![risky.id.clone()]);
    let status = |id: &TaskId| store.with(|c| tasks::get_task(c, id)).unwrap().status;
    assert_eq!(status(&unsent.id), TaskStatus::Queued);
    assert_eq!(status(&safe.id), TaskStatus::Queued);
    assert_eq!(status(&risky.id), TaskStatus::Interrupted);
    let history = store.with(|c| tasks::transitions(c, &risky.id)).unwrap();
    assert_eq!(history.last().unwrap().1, "interrupted");
}

#[test]
fn deliveries_a_lost_turn_had_claimed_are_pending_again_after_recovery() {
    let store = Store::open_in_memory().unwrap();
    let (_d, dir) = temp();
    let (a, t) = setup(&store, &dir);
    let task = store
        .with(|c| tasks::insert_task(c, &row(&a, &t, "x", PermissionMode::Ask)))
        .unwrap()
        .0;
    let msg = crate::orchestra::model::AgentMessage {
        id: crate::orchestra::MessageId::generate(),
        sender: None,
        recipient: a.clone(),
        conversation_id: crate::orchestra::ConversationId::generate(),
        correlation_id: None,
        task_id: None,
        payload: crate::orchestra::MessagePayload::Text {
            content: "hi".into(),
        },
        created_at: now_ms(),
    };
    let d = crate::orchestra::model::Delivery {
        id: crate::orchestra::DeliveryId::generate(),
        message_id: msg.id.clone(),
        recipient: a.clone(),
        task_id: Some(task.id.clone()),
        status: DeliveryStatus::Pending,
        attempts: 0,
        last_error: None,
        updated_at: 0,
    };
    store
        .tx(|c| {
            mail::insert_message(c, &msg)?;
            mail::insert_delivery(c, &d)?;
            mail::set_deliveries(
                c,
                std::slice::from_ref(&d.id),
                &[DeliveryStatus::Pending],
                DeliveryStatus::Claimed,
                None,
            )
        })
        .unwrap();
    let r = store.tx(|c| tasks::recover(c)).unwrap();
    assert_eq!(r.deliveries, 1);
    let again = store.with(|c| mail::get_delivery(c, &d.id)).unwrap();
    assert_eq!(again.status, DeliveryStatus::Pending);
    assert_eq!(again.attempts, 1);
}

#[test]
fn the_scheduler_lease_has_one_holder_until_it_expires() {
    let store = Store::open_in_memory().unwrap();
    let take = |who: &str, now: i64| {
        store
            .with(|c| agents::take_lease(c, "s", who, now, 1000))
            .unwrap()
    };
    assert!(take("a", 0));
    assert!(!take("b", 500));
    assert!(take("a", 900));
    assert!(!take("b", 1500));
    assert!(take("b", 2000));
}
