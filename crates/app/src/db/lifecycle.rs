//! The life of one database connection, as a pure state machine the store
//! drives: idle until used, reconnecting with backoff after a drop, closed
//! after a while unused or while the app is in the background, and never
//! retrying what retrying can't fix (a wrong password).

use std::time::{Duration, Instant};

use sawit_core::db::FailureKind;

/// Reconnect attempts before giving up (about a minute and a half in all).
pub const MAX_ATTEMPTS: u32 = 8;

/// Wait before reconnect attempt `attempt` (1-based).
pub fn backoff(attempt: u32) -> Duration {
    const STEPS: [u64; 6] = [1, 2, 4, 8, 15, 30];
    Duration::from_secs(STEPS[(attempt.max(1) as usize - 1).min(STEPS.len() - 1)])
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConnState {
    /// Not connected; connects when something needs it. Every connection
    /// starts here, and returns here after going unused for a while.
    Idle,
    /// Opening. `attempt` 0 is a first connect; higher ones reconnect.
    Connecting {
        attempt: u32,
    },
    Connected,
    /// Closed while the app was in the background; connects again on use.
    Inactive,
    /// Closed by the user; connects again only when asked to.
    Disconnected,
    /// Reconnecting after a failure; the next try starts at `retry_at`.
    Retrying {
        attempt: u32,
        retry_at: Instant,
        error: String,
    },
    /// Gave up until asked again: retrying wouldn't help, or didn't.
    Failed {
        error: String,
        kind: FailureKind,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Something needs the connection (a click, a table, a query).
    Connect,
    Opened,
    OpenFailed {
        kind: FailureKind,
        error: String,
    },
    /// The live connection closed on its own (network, server restart,
    /// session killed). `active`: the app is in the foreground.
    Dropped {
        active: bool,
    },
    Disconnect,
    /// Unused for too long while the app is open.
    IdleTimeout,
    /// The app has been in the background for a while.
    Background,
    /// A clock tick; starts a due reconnect attempt.
    Tick,
}

impl ConnState {
    /// The state after `event` at `now`.
    pub fn step(&self, event: Event, now: Instant) -> ConnState {
        use ConnState::*;
        match (self, event) {
            (_, Event::Disconnect) => Disconnected,

            (Connecting { .. } | Connected, Event::Connect) => self.clone(),
            // Asked for while waiting to retry: try now.
            (Retrying { attempt, .. }, Event::Connect) => Connecting { attempt: *attempt },
            (_, Event::Connect) => Connecting { attempt: 0 },

            (Connecting { .. }, Event::Opened) => Connected,

            (Connecting { attempt }, Event::OpenFailed { kind, error }) => {
                // A first connect only retries a busy or waking server; a
                // reconnect also rides out network trouble.
                let retry = kind.retryable() && (*attempt > 0 || kind == FailureKind::ServerBusy);
                let next = attempt + 1;
                if retry && next <= MAX_ATTEMPTS {
                    Retrying {
                        attempt: next,
                        retry_at: now + backoff(next),
                        error,
                    }
                } else if retry {
                    Failed {
                        error: format!("Gave up after {MAX_ATTEMPTS} attempts. {error}"),
                        kind,
                    }
                } else {
                    Failed { error, kind }
                }
            }

            (Connected, Event::Dropped { active: true }) => Connecting { attempt: 1 },
            (Connected, Event::Dropped { active: false }) => Inactive,

            (Connected, Event::IdleTimeout) => Idle,

            // Nothing reconnects while the app is in the background.
            (Connected | Retrying { .. }, Event::Background) => Inactive,

            (
                Retrying {
                    attempt, retry_at, ..
                },
                Event::Tick,
            ) if *retry_at <= now => Connecting { attempt: *attempt },

            // Late results of an abandoned attempt, and events that don't
            // apply, change nothing.
            _ => self.clone(),
        }
    }

    pub fn is_connected(&self) -> bool {
        matches!(self, ConnState::Connected)
    }

    /// A connection is on its way (now or after a wait).
    pub fn is_pending(&self) -> bool {
        matches!(
            self,
            ConnState::Connecting { .. } | ConnState::Retrying { .. }
        )
    }

    /// Short status for the connection list; `None` when connected or when
    /// the error line says it all.
    pub fn label(&self, now: Instant) -> Option<String> {
        Some(match self {
            ConnState::Idle => "idle".into(),
            ConnState::Connecting { attempt: 0 } => "connecting…".into(),
            ConnState::Connecting { attempt } => {
                format!("reconnecting {attempt}/{MAX_ATTEMPTS}…")
            }
            ConnState::Inactive => "inactive".into(),
            ConnState::Disconnected => "disconnected".into(),
            ConnState::Retrying { retry_at, .. } => {
                let secs = retry_at.saturating_duration_since(now).as_secs_f32().ceil();
                format!("retrying in {secs:.0}s")
            }
            ConnState::Connected | ConnState::Failed { .. } => return None,
        })
    }

    /// Why the connection isn't working, for the error line.
    pub fn error(&self) -> Option<&str> {
        match self {
            ConnState::Failed { error, .. } | ConnState::Retrying { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ConnState::*;

    fn failed(kind: FailureKind) -> Event {
        Event::OpenFailed {
            kind,
            error: "boom".into(),
        }
    }

    #[test]
    fn starts_idle_and_connects_on_first_use() {
        let now = Instant::now();
        let s = Idle.step(Event::Connect, now);
        assert_eq!(s, Connecting { attempt: 0 });
        assert_eq!(s.step(Event::Opened, now), Connected);
        // Asking again while connecting or connected changes nothing.
        assert_eq!(s.step(Event::Connect, now), s);
        assert_eq!(Connected.step(Event::Connect, now), Connected);
    }

    #[test]
    fn every_closed_state_reconnects_when_asked() {
        let now = Instant::now();
        let failed_state = Failed {
            error: "x".into(),
            kind: FailureKind::Auth,
        };
        for s in [Idle, Inactive, Disconnected, failed_state] {
            assert_eq!(
                s.step(Event::Connect, now),
                Connecting { attempt: 0 },
                "{s:?}"
            );
        }
    }

    #[test]
    fn first_connect_failures_do_not_retry_unless_the_server_is_busy() {
        let now = Instant::now();
        let first = Connecting { attempt: 0 };
        for kind in [
            FailureKind::Auth,
            FailureKind::DatabaseMissing,
            FailureKind::Tls,
            FailureKind::Config,
            FailureKind::Network, // wrong host: say so at once
            FailureKind::Other,
        ] {
            assert!(
                matches!(first.step(failed(kind), now), Failed { kind: k, .. } if k == kind),
                "{kind:?}"
            );
        }
        // A waking serverless compute or a full server is worth a retry.
        assert_eq!(
            first.step(failed(FailureKind::ServerBusy), now),
            Retrying {
                attempt: 1,
                retry_at: now + backoff(1),
                error: "boom".into()
            }
        );
    }

    #[test]
    fn a_dropped_connection_reconnects_with_backoff_then_gives_up() {
        let mut now = Instant::now();
        let mut s = Connected.step(Event::Dropped { active: true }, now);
        assert_eq!(s, Connecting { attempt: 1 });
        let mut waits = Vec::new();
        loop {
            s = s.step(failed(FailureKind::Network), now);
            match &s {
                Retrying { retry_at, .. } => {
                    waits.push(retry_at.duration_since(now).as_secs());
                    // Not due yet: still waiting.
                    assert_eq!(s.step(Event::Tick, now), s);
                    now = *retry_at;
                    s = s.step(Event::Tick, now);
                    assert!(matches!(s, Connecting { .. }));
                }
                Failed { error, kind } => {
                    assert_eq!(*kind, FailureKind::Network);
                    assert!(error.starts_with("Gave up after 8 attempts"), "{error}");
                    break;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(waits, vec![2, 4, 8, 15, 30, 30, 30]);
    }

    #[test]
    fn a_reconnect_stops_retrying_on_errors_retrying_cannot_fix() {
        let now = Instant::now();
        // The password was rotated while we were connected.
        let s = Connecting { attempt: 3 }.step(failed(FailureKind::Auth), now);
        assert!(matches!(
            s,
            Failed {
                kind: FailureKind::Auth,
                ..
            }
        ));
        // Success at any attempt resets to connected.
        assert_eq!(
            Connecting { attempt: 5 }.step(Event::Opened, now),
            Connected
        );
    }

    #[test]
    fn asking_while_waiting_to_retry_tries_now() {
        let now = Instant::now();
        let waiting = Retrying {
            attempt: 3,
            retry_at: now + Duration::from_secs(8),
            error: "x".into(),
        };
        assert_eq!(waiting.step(Event::Connect, now), Connecting { attempt: 3 });
    }

    #[test]
    fn leaving_the_app_closes_and_coming_back_connects_on_use() {
        let now = Instant::now();
        assert_eq!(Connected.step(Event::Background, now), Inactive);
        // Retries pause in the background too.
        let waiting = Retrying {
            attempt: 2,
            retry_at: now,
            error: "x".into(),
        };
        assert_eq!(waiting.step(Event::Background, now), Inactive);
        // A drop noticed while away doesn't start reconnecting.
        assert_eq!(
            Connected.step(Event::Dropped { active: false }, now),
            Inactive
        );
        // An attempt under way finishes; the store closes it later.
        let connecting = Connecting { attempt: 0 };
        assert_eq!(connecting.step(Event::Background, now), connecting);
        // Back in the app: stays inactive until something needs it.
        assert_eq!(Inactive.step(Event::Tick, now), Inactive);
        assert_eq!(
            Inactive.step(Event::Connect, now),
            Connecting { attempt: 0 }
        );
    }

    #[test]
    fn unused_connections_go_idle() {
        let now = Instant::now();
        assert_eq!(Connected.step(Event::IdleTimeout, now), Idle);
        assert_eq!(Idle.step(Event::IdleTimeout, now), Idle);
    }

    #[test]
    fn disconnect_wins_from_anywhere_and_late_results_are_ignored() {
        let now = Instant::now();
        let states = [
            Idle,
            Connecting { attempt: 2 },
            Connected,
            Inactive,
            Retrying {
                attempt: 1,
                retry_at: now,
                error: "x".into(),
            },
        ];
        for s in states {
            assert_eq!(s.step(Event::Disconnect, now), Disconnected, "{s:?}");
        }
        // A connect that finishes after the user disconnected stays out.
        assert_eq!(Disconnected.step(Event::Opened, now), Disconnected);
        assert_eq!(
            Disconnected.step(failed(FailureKind::Network), now),
            Disconnected
        );
        assert_eq!(Disconnected.step(Event::Tick, now), Disconnected);
        assert_eq!(
            Disconnected.step(Event::Dropped { active: true }, now),
            Disconnected
        );
    }

    #[test]
    fn labels_and_errors_describe_each_state() {
        let now = Instant::now();
        assert_eq!(Idle.label(now).as_deref(), Some("idle"));
        assert_eq!(Inactive.label(now).as_deref(), Some("inactive"));
        assert_eq!(Disconnected.label(now).as_deref(), Some("disconnected"));
        assert_eq!(
            Connecting { attempt: 0 }.label(now).as_deref(),
            Some("connecting…")
        );
        assert_eq!(
            Connecting { attempt: 2 }.label(now).as_deref(),
            Some("reconnecting 2/8…")
        );
        let waiting = Retrying {
            attempt: 2,
            retry_at: now + Duration::from_millis(3200),
            error: "lost".into(),
        };
        assert_eq!(waiting.label(now).as_deref(), Some("retrying in 4s"));
        assert_eq!(waiting.error(), Some("lost"));
        assert!(waiting.is_pending() && !waiting.is_connected());
        assert_eq!(Connected.label(now), None);
        assert!(Connected.is_connected());
    }
}
