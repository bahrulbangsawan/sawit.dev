//! Screenshots: one at a time, without the overlay, and waits that end.

use serde_json::json;

use crate::browser;
use crate::browser::shots::{overlay_op, shoot_ops, within};

#[test]
fn shots_take_turns() {
    let mut gate = browser::shots::ShotGate::default();
    assert!(gate.enter().is_none());
    let mut second = gate.enter().expect("waits");
    let mut third = gate.enter().expect("waits");
    assert!(second.try_recv().unwrap().is_none());
    gate.leave();
    assert!(second.try_recv().unwrap().is_some());
    assert!(third.try_recv().unwrap().is_none());
    gate.leave();
    assert!(third.try_recv().unwrap().is_some());
    gate.leave();
    assert!(gate.enter().is_none());
}

#[test]
fn screenshot_wraps_overlay() {
    assert_eq!(overlay_op(true), json!({"op": "overlay", "hidden": true}));
    assert_eq!(shoot_ops(false), [overlay_op(true), overlay_op(false)]);
}

/// Agents' screenshots leave the agents' cursors out with the rest of the overlay, unless the
/// user keeps them in (`agent_cursors_in_screenshots`): then the page hides all but them.
#[test]
fn screenshots_leave_agents_cursors_out_unless_kept() {
    assert_eq!(
        shoot_ops(false)[0],
        json!({"op": "overlay", "hidden": true})
    );
    assert_eq!(
        shoot_ops(true)[0],
        json!({"op": "overlay", "hidden": true, "agents": true})
    );
    assert_eq!(shoot_ops(true)[1], overlay_op(false));
}

#[test]
fn a_wait_ends_when_its_timer_fires() {
    use futures::executor::block_on;
    use futures::future::{pending, ready};
    let never = pending::<u8>();
    assert_eq!(block_on(within(never, ready(()))), None);
    assert_eq!(block_on(within(ready(7), pending::<()>())), Some(7));
    let (tx, rx) = futures::channel::oneshot::channel::<u8>();
    drop(tx);
    assert!(matches!(
        block_on(within(rx, pending::<()>())),
        Some(Err(_))
    ));
}
