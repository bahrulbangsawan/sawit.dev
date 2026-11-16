//! Agent-to-agent communication: discovery, messages and their deliveries, replies landing in
//! the right thread, authorization, and the limits on delegation (depth, cycles, outstanding
//! work, sizes).

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use super::store_tests::providers;
use super::support::{Script, Scripted, Turn, agent, done, runtime, temp};
use crate::orchestra::model::{DeliveryStatus, TaskStatus};
use crate::orchestra::store::{Store, agents, mail, tasks};
use crate::orchestra::{Caller, Comms, Limits, OrchestraError, SubmitTask, registry};

fn setup(names: &[&str]) -> (tempfile::TempDir, Comms) {
    let (d, dir) = temp();
    let store = Store::open_in_memory().unwrap();
    for n in names {
        registry::create(&store, agent(n, &dir), &providers()).unwrap();
    }
    (d, Comms::new(store, Limits::default()))
}

fn as_agent(id: &str) -> Caller {
    Caller::agent(id.into(), None)
}

/// A running root task of `agent`, so it can delegate.
fn running_task(c: &Comms, agent: &str) -> Caller {
    let t = c
        .submit_task(&SubmitTask {
            agent: agent.into(),
            text: "work".into(),
            new_thread: true,
            ..Default::default()
        })
        .unwrap();
    c.store
        .tx(|x| tasks::claim_next(x, &agent.into(), "test", i64::MAX))
        .unwrap()
        .unwrap();
    c.store.with(|x| tasks::mark_running(x, &t.id)).unwrap();
    Caller::agent(agent.into(), Some(t.id))
}

#[test]
fn discovery_shows_eligible_agents_without_granting_contact() {
    let (_d, c) = setup(&["lead", "backend", "hidden", "picky"]);
    c.store
        .with(|x| {
            let mut h = agents::get_agent(x, "hidden")?.profile;
            h.comms.discoverable = false;
            agents::update_agent(x, &h)?;
            let mut p = agents::get_agent(x, "picky")?.profile;
            p.comms.accepts_from = vec!["backend".into()];
            agents::update_agent(x, &p)
        })
        .unwrap();
    let seen = c.list_agents(&as_agent("lead")).unwrap();
    let ids: Vec<&str> = seen.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["backend", "picky"],
        "itself and hidden agents are left out"
    );
    let picky = seen.iter().find(|a| a.id.as_str() == "picky").unwrap();
    assert!(!picky.can_message && !picky.can_delegate);
    assert_eq!(
        c.list_agents(&Caller::user()).unwrap().len(),
        4,
        "the user sees every agent"
    );
    let denied = c
        .send_message(&as_agent("lead"), "picky", "hi", None)
        .unwrap_err();
    assert!(matches!(denied, OrchestraError::Denied(_)), "{denied}");
}

#[test]
fn agents_of_another_workspace_are_out_of_reach_unless_named() {
    let (_d, dir) = temp();
    let (_e, other) = temp();
    let store = Store::open_in_memory().unwrap();
    registry::create(&store, agent("a", &dir), &providers()).unwrap();
    registry::create(&store, agent("far", &other), &providers()).unwrap();
    let c = Comms::new(store, Limits::default());
    assert!(c.list_agents(&as_agent("a")).unwrap().is_empty());
    assert!(matches!(
        c.send_message(&as_agent("a"), "far", "hi", None),
        Err(OrchestraError::Denied(_))
    ));
    c.store
        .with(|x| {
            let mut f = agents::get_agent(x, "far")?.profile;
            f.comms.accepts_from = vec!["a".into()];
            agents::update_agent(x, &f)
        })
        .unwrap();
    assert!(c.send_message(&as_agent("a"), "far", "hi", None).is_ok());
}

#[test]
fn a_message_is_stored_with_its_delivery_and_a_task_to_take_it_in() {
    let (_d, c) = setup(&["a", "b"]);
    let sent = c
        .send_message(&as_agent("a"), "b", "please review", None)
        .unwrap();
    let d = c
        .store
        .with(|x| mail::get_delivery(x, &sent.delivery_id))
        .unwrap();
    assert_eq!(d.status, DeliveryStatus::Pending);
    assert_eq!(d.task_id.as_ref(), Some(&sent.task_id));
    let m = c
        .store
        .with(|x| mail::get_message(x, &sent.message_id))
        .unwrap();
    assert_eq!(m.sender.as_ref().map(|s| s.as_str()), Some("a"));
    let t = c.store.with(|x| tasks::get_task(x, &sent.task_id)).unwrap();
    assert_eq!(t.agent_id.as_str(), "b");
    assert_eq!(t.status, TaskStatus::Queued);
    // Accepted is not done: nothing has run.
    assert_eq!(sent.status, "accepted");
}

#[test]
fn a_reply_lands_in_the_conversations_thread_and_messages_keep_their_order() {
    let (_d, c) = setup(&["a", "b"]);
    let first = c.send_message(&as_agent("a"), "b", "one", None).unwrap();
    let second = c
        .send_message(&as_agent("a"), "b", "two", Some(first.message_id.clone()))
        .unwrap();
    let t1 = c
        .store
        .with(|x| tasks::get_task(x, &first.task_id))
        .unwrap();
    let t2 = c
        .store
        .with(|x| tasks::get_task(x, &second.task_id))
        .unwrap();
    assert_eq!(t1.thread_id, t2.thread_id, "one conversation, one thread");
    let claim = || {
        c.store
            .tx(|x| tasks::claim_next(x, &"b".into(), "t", i64::MAX))
            .unwrap()
    };
    assert_eq!(claim().unwrap().id, t1.id);
    assert!(
        claim().is_none(),
        "the second waits for the first in its thread"
    );
    let reply = c
        .send_message(
            &as_agent("b"),
            "a",
            "got it",
            Some(first.message_id.clone()),
        )
        .unwrap();
    assert_eq!(reply.conversation_id, first.conversation_id);
    let reply_task = c
        .store
        .with(|x| tasks::get_task(x, &reply.task_id))
        .unwrap();
    let a_thread = c
        .store
        .with(|x| agents::conversation_thread(x, &"a".into(), &first.conversation_id))
        .unwrap();
    assert_eq!(reply_task.thread_id, a_thread.id);
}

#[test]
fn delegation_limits_hold_whatever_the_prompt_says() {
    let (_d, c) = setup(&["a", "b", "c", "d"]);
    let a = running_task(&c, "a");
    assert!(
        matches!(
            c.delegate_task(&as_agent("a"), "b", "t", "do it", None, None),
            Err(OrchestraError::Invalid(_))
        ),
        "an agent delegates from a task"
    );
    let to_b = c.delegate_task(&a, "b", "t", "do it", None, None).unwrap();
    // b runs its task and tries to hand work back to a: a cycle.
    c.store
        .tx(|x| tasks::claim_next(x, &"b".into(), "t", i64::MAX))
        .unwrap()
        .unwrap();
    c.store
        .with(|x| tasks::mark_running(x, &to_b.task_id))
        .unwrap();
    let b = Caller::agent("b".into(), Some(to_b.task_id.clone()));
    let cycle = c
        .delegate_task(&b, "a", "back", "you do it", None, None)
        .unwrap_err();
    assert!(matches!(cycle, OrchestraError::Denied(_)), "{cycle}");
    assert!(matches!(
        c.delegate_task(&b, "b", "self", "x", None, None),
        Err(OrchestraError::Denied(_))
    ));
    // Depth: a (0) → b (1) → c (2) → d (3) is the limit; d can go no deeper.
    let to_c = c.delegate_task(&b, "c", "t", "x", None, None).unwrap();
    c.store
        .tx(|x| tasks::claim_next(x, &"c".into(), "t", i64::MAX))
        .unwrap()
        .unwrap();
    c.store
        .with(|x| tasks::mark_running(x, &to_c.task_id))
        .unwrap();
    let cc = Caller::agent("c".into(), Some(to_c.task_id.clone()));
    let to_d = c.delegate_task(&cc, "d", "t", "x", None, None).unwrap();
    c.store
        .tx(|x| tasks::claim_next(x, &"d".into(), "t", i64::MAX))
        .unwrap()
        .unwrap();
    c.store
        .with(|x| tasks::mark_running(x, &to_d.task_id))
        .unwrap();
    let dd = Caller::agent("d".into(), Some(to_d.task_id.clone()));
    let deep = c.delegate_task(&dd, "c", "t", "x", None, None).unwrap_err();
    assert!(matches!(deep, OrchestraError::Limit(_)), "{deep}");
    // Size.
    let big = "x".repeat(Limits::default().max_message_bytes + 1);
    assert!(matches!(
        c.delegate_task(&a, "c", "t", &big, None, None),
        Err(OrchestraError::Limit(_))
    ));
    assert!(matches!(
        c.send_message(&a, "c", &big, None),
        Err(OrchestraError::Limit(_))
    ));
    // Another agent's task is not a parent this agent can use.
    assert!(matches!(
        c.delegate_task(
            &as_agent("c"),
            "d",
            "t",
            "x",
            Some(to_b.task_id.clone()),
            None
        ),
        Err(OrchestraError::Denied(_))
    ));
}

#[test]
fn outstanding_delegations_and_permissions_are_bounded() {
    let (_d, mut c) = setup(&["a", "b"]);
    c.limits.max_outstanding_per_root = 2;
    c.store
        .with(|x| {
            let mut b = agents::get_agent(x, "b")?.profile;
            b.permission = crate::orchestra::PermissionMode::Allow;
            agents::update_agent(x, &b)?;
            let mut a = agents::get_agent(x, "a")?.profile;
            a.permission = crate::orchestra::PermissionMode::ReadOnly;
            agents::update_agent(x, &a)
        })
        .unwrap();
    let a = running_task(&c, "a");
    let one = c.delegate_task(&a, "b", "1", "x", None, None).unwrap();
    c.delegate_task(&a, "b", "2", "x", None, None).unwrap();
    assert!(matches!(
        c.delegate_task(&a, "b", "3", "x", None, None),
        Err(OrchestraError::Limit(_))
    ));
    let child = c.store.with(|x| tasks::get_task(x, &one.task_id)).unwrap();
    assert_eq!(
        child.permission,
        crate::orchestra::PermissionMode::ReadOnly,
        "delegation never widens permissions"
    );
    // The same idempotency key makes nothing new.
    let mut c2 = c.clone();
    c2.limits.max_outstanding_per_root = 10;
    let k1 = c2
        .delegate_task(&a, "b", "k", "x", None, Some("key".into()))
        .unwrap();
    let k2 = c2
        .delegate_task(&a, "b", "k", "x", None, Some("key".into()))
        .unwrap();
    assert_eq!(k1.task_id, k2.task_id);
    assert!(!k2.created);
}

#[test]
fn tools_answer_by_name_and_check_who_may_see_a_task() {
    let (_d, c) = setup(&["a", "b", "c"]);
    let a = running_task(&c, "a");
    let d = c
        .call(
            &a,
            "delegate_task",
            &json!({"recipient": "b", "title": "API", "instructions": "build it"}),
        )
        .unwrap();
    let id = d["task_id"].as_str().unwrap().to_string();
    let status = c
        .call(&a, "get_task_status", &json!({"task_id": id}))
        .unwrap();
    assert_eq!(status["status"], "queued");
    assert_eq!(status["requested_by"], "a");
    let listed = c
        .call(&a, "list_agent_tasks", &json!({"agent": "b"}))
        .unwrap();
    assert_eq!(listed["tasks"].as_array().unwrap().len(), 1);
    assert!(matches!(
        c.call(&as_agent("c"), "get_task_status", &json!({"task_id": id})),
        Err(OrchestraError::Denied(_))
    ));
    assert!(matches!(
        c.call(&as_agent("c"), "cancel_task", &json!({"task_id": id})),
        Err(OrchestraError::Denied(_))
    ));
    let cancelled = c.call(&a, "cancel_task", &json!({"task_id": id})).unwrap();
    assert_eq!(cancelled["cancelled"][0], id.as_str());
    assert!(matches!(
        c.call(&a, "nope", &json!({})),
        Err(OrchestraError::NotFound(_))
    ));
    assert!(matches!(
        c.call(&a, "send_message", &json!({"recipient": "b"})),
        Err(OrchestraError::Invalid(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn messages_run_on_the_recipient_and_failures_reach_the_asker() {
    let (_d, dir) = temp();
    let script: Script = Arc::new(|t: Turn| {
        Box::pin(async move {
            if t.agent.as_str() == "lead" && !t.text.contains("Results of the work") {
                let r = t
                    .ctx
                    .tools
                    .call(
                        "delegate_task",
                        json!({"recipient": "flaky", "title": "risky", "instructions": "explode"}),
                    )
                    .await;
                return Ok(done(format!("delegated: {}", r.is_ok())));
            }
            if t.agent.as_str() == "flaky" {
                return Err(crate::orchestra::provider::ProviderError::Protocol(
                    "exploded".into(),
                ));
            }
            Ok(done(format!("lead saw: {}", t.text.contains("[failed]"))))
        })
    });
    let o = runtime(&dir, Scripted::new(true, script), Limits::default());
    for n in ["lead", "flaky", "inbox"] {
        o.create_agent(agent(n, &dir)).await.unwrap();
    }
    let sent = o
        .comms()
        .send_message(&Caller::user(), "inbox", "hello there", None)
        .unwrap();
    let root = o
        .submit(SubmitTask {
            agent: "lead".into(),
            text: "start".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(o.wait_idle(Duration::from_secs(10)).await);
    let d = o
        .store()
        .with(|x| mail::get_delivery(x, &sent.delivery_id))
        .unwrap();
    assert_eq!(d.status, DeliveryStatus::Delivered);
    let lead = o.task(&root.id).await.unwrap();
    assert_eq!(lead.status, TaskStatus::Completed);
    assert_eq!(lead.result.unwrap()["text"], "lead saw: true");
    o.shutdown().await;
}

#[test]
fn a_full_mailbox_and_a_long_queue_push_back() {
    let (_d, mut c) = setup(&["a", "b"]);
    c.limits.max_pending_messages = 2;
    c.send_message(&Caller::user(), "b", "1", None).unwrap();
    c.send_message(&Caller::user(), "b", "2", None).unwrap();
    assert!(matches!(
        c.send_message(&Caller::user(), "b", "3", None),
        Err(OrchestraError::Limit(_))
    ));
    c.limits.max_pending_messages = 100;
    c.limits.max_queued_per_agent = 2;
    assert!(matches!(
        c.submit_task(&SubmitTask {
            agent: "b".into(),
            text: "x".into(),
            ..Default::default()
        }),
        Err(OrchestraError::Limit(_))
    ));
}
