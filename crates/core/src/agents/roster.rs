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
    /// What it is called on screen: "Claude Code", "Codex · shop", "Claude Code 2".
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
    /// is known, and "name 2", "name 3" for another of the same name.
    pub fn meet(&mut self, identity: &AgentIdentity, now: Duration) -> &Known {
        if let Some(ix) = self.known.iter().position(|k| k.identity.id == identity.id) {
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

    /// The agent with id `id`, if the roster knows it.
    pub fn get(&self, id: &str) -> Option<&Known> {
        self.known.iter().find(|k| k.identity.id == id)
    }

    pub fn all(&self) -> &[Known] {
        &self.known
    }

    fn label_for(&self, identity: &AgentIdentity) -> String {
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
            _ => format!("{name} {}", same.len() + 1),
        }
    }
}
