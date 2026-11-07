//! The colour and label the app gives each agent it meets, kept for as long as it keeps the agent.

use std::time::Duration;

use super::{AgentIdentity, GONE_AFTER};

/// A colour the app gives agents, as `#rrggbb`: twelve, so that a dozen agents at work at once each
/// have their own. Each one carries white text at 4.5:1 or more and reads at 4.5:1 or more on the
/// canvas's light page ([`LIGHT_PAGE`]); on the dark UI the app outlines the pointer and the pill
/// in white, which keeps their shape clear there too. None is red, which says that work failed.
pub const PALETTE: [&str; 12] = [
    "#2563eb", // blue
    "#c2410c", // orange
    "#15803d", // green
    "#7c3aed", // violet
    "#be185d", // pink
    "#0e7490", // cyan
    "#b45309", // amber
    "#a21caf", // fuchsia
    "#4338ca", // indigo
    "#4d7c0f", // olive
    "#0f766e", // teal
    "#854d0e", // brown
];

/// The canvas's light page, FigCraft's default page colour: every [`PALETTE`] colour reads on it.
pub const LIGHT_PAGE: &str = "#f5f5f5";

/// The text colour on a [`PALETTE`] colour.
pub const INK: &str = "#ffffff";

/// One agent the app has met, with the colour and label it shows with.
#[derive(Debug, Clone, PartialEq)]
pub struct Known {
    pub identity: AgentIdentity,
    /// An index into [`PALETTE`].
    pub color: usize,
    /// What it is called on screen: "Claude Code", "Codex · shop", or a random name for a second
    /// one ("Swift Durian").
    pub label: String,
    /// When the app last heard from it, in the caller's clock.
    pub seen: Duration,
}

impl Known {
    pub fn color_hex(&self) -> &'static str {
        PALETTE[self.color % PALETTE.len()]
    }

    /// "Claude Code" as the subject of a sentence; an agent without a name is "An agent".
    pub fn subject(&self) -> &str {
        if self.identity.is_anonymous() {
            "An agent"
        } else {
            &self.label
        }
    }
}

/// How many agents the roster keeps: the ones heard from longest ago go first.
const MAX_KNOWN: usize = 64;

/// The agents the app has met, each with its colour and label. Both are given once, when the
/// agent is first met, and kept for as long as the roster keeps it: an agent never changes
/// colour or name while it works, and a second one never renames the first.
#[derive(Debug, Default)]
pub struct Roster {
    known: Vec<Known>,
    /// How many agents have been met, for the colour of one when every colour is in use.
    met: usize,
}

impl Roster {
    /// The agent `identity` names, met now (`now`, in the caller's clock, which only goes
    /// forward): as it was met before, or met now for the first time. A new agent takes the
    /// first colour no agent heard from in the last [`GONE_AFTER`] has, else the next in turn;
    /// its label is its name, "name · project" when an agent of that name in another project
    /// is known, and a random name ([`Roster::random_name`]) for another of the same name.
    pub fn meet(&mut self, identity: &AgentIdentity, now: Duration) -> &Known {
        let key = identity.key();
        if let Some(ix) = self.known.iter().position(|k| k.identity.key() == key) {
            let known = &mut self.known[ix];
            known.seen = now;
            // A later op may know its project where the first didn't.
            if known.identity.label.is_none() {
                known.identity.label = identity.label.clone();
            }
            return &self.known[ix];
        }
        let active = |k: &&Known| now.saturating_sub(k.seen) < GONE_AFTER;
        let taken: Vec<usize> = self.known.iter().filter(active).map(|k| k.color).collect();
        let color = (0..PALETTE.len())
            .find(|c| !taken.contains(c))
            .unwrap_or(self.met % PALETTE.len());
        let label = self.label_for(identity);
        self.met += 1;
        if self.known.len() >= MAX_KNOWN
            && let Some(oldest) = (0..self.known.len()).min_by_key(|&ix| self.known[ix].seen)
        {
            self.known.remove(oldest);
        }
        self.known.push(Known {
            identity: identity.clone(),
            color,
            label,
            seen: now,
        });
        self.known.last().expect("just pushed")
    }

    /// The agent whose key ([`AgentIdentity::key`]) is `key`, if the roster knows it.
    pub fn get(&self, key: &str) -> Option<&Known> {
        self.known.iter().find(|k| k.identity.key() == key)
    }

    pub fn all(&self) -> &[Known] {
        &self.known
    }

    fn label_for(&self, identity: &AgentIdentity) -> String {
        // A lane that names itself is called so; one that doesn't is its session's client, and a
        // second agent of that client a short random name ("Swift Durian"), never a number. An
        // agent that doesn't say what it is (the anonymous one) has a random name too.
        if let Some(name) = identity.lane.as_ref().and_then(|l| l.name.as_deref()) {
            return name.to_string();
        }
        if identity.is_anonymous() {
            return self.random_name(&identity.key());
        }
        let name = identity.name.trim();
        let same: Vec<&Known> = (self.known.iter())
            .filter(|k| k.identity.name.trim() == name)
            .collect();
        if same.is_empty() {
            return name.to_string();
        }
        match &identity.label {
            Some(project)
                if same
                    .iter()
                    .all(|k| k.identity.label.as_ref() != Some(project)) =>
            {
                format!("{name} · {project}")
            }
            _ => self.random_name(&identity.key()),
        }
    }

    /// A short, friendly name for the agent whose key is `key`: an adjective and a fruit or plant,
    /// picked from the key, so that it is the same each time the agent is met, and moved on to
    /// the next free one while an agent known now has it.
    pub fn random_name(&self, key: &str) -> String {
        let taken = |name: &str| self.known.iter().any(|k| k.label == name);
        let count = ADJECTIVES.len() * PLANTS.len();
        let start = (fnv(key) % count as u64) as usize;
        (0..count)
            .map(|i| name_at((start + i * 7919) % count))
            .find(|name| !taken(name))
            .unwrap_or_else(|| name_at(start))
    }
}

/// The first word of a random agent name: friendly, short, and nothing that could read as rude.
pub const ADJECTIVES: [&str; 16] = [
    "Swift", "Calm", "Bright", "Brave", "Gentle", "Lucky", "Merry", "Quiet", "Sunny", "Clever",
    "Happy", "Kind", "Bold", "Keen", "Jolly", "Nimble",
];

/// The second word: a fruit or a plant.
pub const PLANTS: [&str; 16] = [
    "Durian", "Pandan", "Mango", "Lychee", "Papaya", "Guava", "Salak", "Melon", "Kiwi", "Lime",
    "Basil", "Lotus", "Orchid", "Bamboo", "Ginger", "Clove",
];

/// The `i`th of the names [`ADJECTIVES`] × [`PLANTS`] make.
fn name_at(i: usize) -> String {
    format!(
        "{} {}",
        ADJECTIVES[i / PLANTS.len() % ADJECTIVES.len()],
        PLANTS[i % PLANTS.len()]
    )
}

/// FNV-1a of `text`: the same on every run, so an agent's name stays put.
fn fnv(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}
