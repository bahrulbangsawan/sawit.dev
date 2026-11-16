//! The runtime's identifiers and its clock. Every id is a newtype over its text, so an agent's id
//! can never be passed where a task's is wanted. Ids made here are a prefix and 16 random hex
//! digits (`tsk_4f0c…`); an agent's id may also be a slug its creator picks (`backend`).

use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident, $prefix:literal) => {
        $(#[$doc])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// A new random id.
            pub fn generate() -> Self {
                Self(format!(concat!($prefix, "_{}"), random_hex(8)))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }
    };
}

id_type!(
    /// An agent's stable identity, independent of any provider process.
    AgentId,
    "agt"
);
id_type!(
    /// One unit of scheduled work.
    TaskId,
    "tsk"
);
id_type!(
    /// A logical conversation of one agent: an execution lane.
    ThreadId,
    "thr"
);
id_type!(
    /// An immutable message between agents.
    MessageId,
    "msg"
);
id_type!(
    /// One delivery of a message to its recipient: its state, apart from the message.
    DeliveryId,
    "dlv"
);
id_type!(
    /// The conversation a message belongs to, shared by a message and its replies.
    ConversationId,
    "cnv"
);

/// The provider adapter an agent runs on (`acp`), a key into the provider registry.
pub type ProviderId = String;

/// Milliseconds since the Unix epoch.
pub type Timestamp = i64;

/// Now, in milliseconds since the Unix epoch.
pub fn now_ms() -> Timestamp {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// `bytes` random bytes as lowercase hex. Falls back to the clock and a counter if the system's
/// random source fails, which keeps ids unique within the process.
pub fn random_hex(bytes: usize) -> String {
    use ring::rand::SecureRandom;
    let mut buf = vec![0u8; bytes];
    if ring::rand::SystemRandom::new().fill(&mut buf).is_err() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seed = (now_ms() as u64) ^ COUNTER.fetch_add(1, Ordering::Relaxed).rotate_left(32);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (seed >> ((i % 8) * 8)) as u8 ^ i as u8;
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

/// A slug for an agent id made from its name: lowercase letters, digits and dashes, at most 40
/// characters; `agent` when nothing is left.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.len() >= 40 {
            break;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "agent".into() } else { out }
}
