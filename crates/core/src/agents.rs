//! The agents that drive the app through `sawit-mcp`, as the app shows them: who each one is
//! ([`AgentIdentity`], sent with every control op), the colour and label the app gives it
//! ([`Roster`]), how long its presence lasts on a surface ([`opacity`]), and what it is doing
//! there, in words ([`canvas_action`]).
//!
//! Each `sawit-mcp` process is one agent session: Claude Code, Codex and Cursor each start their
//! own stdio server per session, and a session's subagents share it. So the identity is the
//! process's: the client's name from MCP `initialize`, an id made once per process, and the
//! folder the session works in. An op from a `sawit-mcp` that predates identities carries none,
//! and the app counts it as one anonymous agent ([`AgentIdentity::anonymous`]).

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who sent a control op: one `sawit-mcp` process. `name` is the client's own name for itself,
/// made readable ([`client_name`]), `label` the project or folder its session works in, when it
/// works in one, and `version` the client's version. The app adds the colour and the label it
/// shows ([`Roster`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

/// The id of the agent an op without an identity counts as.
pub const ANONYMOUS_ID: &str = "agent";

impl AgentIdentity {
    /// The agent an op from an older `sawit-mcp` counts as: they all share it, as nothing tells
    /// them apart.
    pub fn anonymous() -> Self {
        Self {
            id: ANONYMOUS_ID.into(),
            name: "Agent".into(),
            label: None,
            version: None,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.id == ANONYMOUS_ID
    }
}

/// The names AI clients give themselves in MCP `initialize` (`clientInfo.name`), by a part of the
/// name, lowercased, and the name people know each one by. The first that matches wins, so the
/// longer, more particular parts come first.
const CLIENTS: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("claude code", "Claude Code"),
    ("claude-ai", "Claude"),
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("windsurf", "Windsurf"),
    ("gemini", "Gemini CLI"),
    ("opencode", "OpenCode"),
    ("roo", "Roo Code"),
    ("cline", "Cline"),
    ("copilot", "Copilot"),
    ("visual studio code", "VS Code"),
    ("vscode", "VS Code"),
    ("zed", "Zed"),
    ("goose", "Goose"),
    ("continue", "Continue"),
    ("kiro", "Kiro"),
    ("warp", "Warp"),
    ("amp", "Amp"),
    ("droid", "Droid"),
];

/// How long a label may be before it is cut, in characters.
const MAX_NAME: usize = 24;

/// The name to show for an MCP client that calls itself `raw` (`clientInfo.name`, such as
/// `claude-code` or `codex-mcp-client`), with `title` (`clientInfo.title`) when it gives one.
/// A known client gets the name people know it by; another gets its title, else its own name
/// with the `-mcp-client` and `-client` endings off, its dashes as spaces and each word
/// capitalised. Never empty, and never longer than [`MAX_NAME`] characters.
pub fn client_name(raw: &str, title: Option<&str>) -> String {
    let lower = raw.trim().to_lowercase();
    if let Some((_, known)) = CLIENTS.iter().find(|(part, _)| lower.contains(part)) {
        return (*known).to_string();
    }
    let name = match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(title) => title.to_string(),
        None => {
            let bare = ["-mcp-client", "_mcp_client", "-client", "_client", "-mcp"]
                .iter()
                .fold(lower.as_str(), |name, end| {
                    name.strip_suffix(end).unwrap_or(name)
                });
            bare.split(['-', '_', ' '])
                .filter(|word| !word.is_empty())
                .map(|word| {
                    let mut letters = word.chars();
                    letters.next().map_or_else(String::new, |first| {
                        first.to_uppercase().chain(letters).collect()
                    })
                })
                .collect::<Vec<_>>()
                .join(" ")
        }
    };
    match name.trim() {
        "" => AgentIdentity::anonymous().name,
        name => name.chars().take(MAX_NAME).collect(),
    }
}

/// The id of a `sawit-mcp` process started as `pid` at `started_nanos` (since the epoch): the
/// pid alone could be another process's once this one has ended.
pub fn session_id(pid: u32, started_nanos: u128) -> String {
    format!("mcp-{pid}-{:08x}", (started_nanos & 0xffff_ffff) as u32)
}

/// A colour the app gives agents, as `#rrggbb`. Each one carries white text at 4.5:1 or more and
/// reads at 4.5:1 or more on the canvas's light page; on the dark UI the app outlines the pointer
/// and the pill in white, which keeps their shape clear there too.
pub const PALETTE: [&str; 8] = [
    "#2563eb", // blue
    "#c2410c", // orange
    "#15803d", // green
    "#7c3aed", // violet
    "#be185d", // pink
    "#0e7490", // cyan
    "#b45309", // amber
    "#a21caf", // fuchsia
];

/// The text colour on a [`PALETTE`] colour.
pub const INK: &str = "#ffffff";

/// How long an agent stays on a surface at full strength after its last op.
pub const FADE_AFTER: Duration = Duration::from_secs(5);
/// How long the fade itself takes.
pub const FADE_TIME: Duration = Duration::from_secs(1);
/// How strong a faded agent stays until it goes.
pub const FADED: f32 = 0.4;
/// How long after its last op an agent leaves a surface.
pub const GONE_AFTER: Duration = Duration::from_secs(30);

/// How strong an agent shows on a surface `idle` after its last op there: whole for
/// [`FADE_AFTER`], then down to [`FADED`] over [`FADE_TIME`], and gone (`None`) after
/// [`GONE_AFTER`].
pub fn opacity(idle: Duration) -> Option<f32> {
    if idle >= GONE_AFTER {
        return None;
    }
    if idle <= FADE_AFTER {
        return Some(1.0);
    }
    let into = (idle - FADE_AFTER).as_secs_f32() / FADE_TIME.as_secs_f32();
    if into >= 1.0 {
        return Some(FADED);
    }
    Some(1.0 - (1.0 - FADED) * into)
}

/// Whether the agent seen `idle` ago still shows the words of what it does: only while it is
/// at full strength.
pub fn shows_action(idle: Duration) -> bool {
    idle <= FADE_AFTER
}

/// How far along a glide of `total` is `elapsed` into it, eased out (fast, then settling), from
/// 0 to 1. A glide of no length is over at once, as every glide is under reduced motion.
pub fn glide(elapsed: Duration, total: Duration) -> f64 {
    if total.is_zero() || elapsed >= total {
        return 1.0;
    }
    let t = elapsed.as_secs_f64() / total.as_secs_f64();
    1.0 - (1.0 - t).powi(3)
}

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

/// What an agent does with one op, in words: `doing` for its caption while it works ("adding 3
/// stickies"), `done` for the sentence the app announces after ("added 3 stickies").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub doing: String,
    pub done: String,
}

impl Action {
    fn new(doing: impl Into<String>, done: impl Into<String>) -> Self {
        Self {
            doing: doing.into(),
            done: done.into(),
        }
    }

    /// `doing` and `done` of one verb on one object: "adding" and "added" "3 stickies".
    fn of(doing: &str, done: &str, object: &str) -> Self {
        Self::new(format!("{doing} {object}"), format!("{done} {object}"))
    }
}

/// `n` things of a kind: "a sticky", "3 stickies".
fn count(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("{} {one}", if starts_with_vowel(one) { "an" } else { "a" }),
        n => format!("{n} {many}"),
    }
}

fn starts_with_vowel(word: &str) -> bool {
    word.starts_with(['a', 'e', 'i', 'o', 'u'])
}

/// What an element of `add` is called, one and many, by its `type`.
fn element_kind(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "sticky" | "note" => ("sticky", "stickies"),
        "rectangle" | "ellipse" | "diamond" | "shape" | "triangle" => ("shape", "shapes"),
        "text" => ("text", "texts"),
        "arrow" => ("arrow", "arrows"),
        "line" => ("line", "lines"),
        "frame" => ("frame", "frames"),
        "section" => ("section", "sections"),
        "image" => ("image", "images"),
        "freedraw" | "ink" => ("drawing", "drawings"),
        _ => ("element", "elements"),
    }
}

/// The items of an op's `elements`.
fn elements(op: &Value) -> &[Value] {
    op.get("elements")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// What an agent's canvas op (`sawit_board::ops`) does, in words: "adding 3 stickies", "moving
/// 2 layers", "drawing a diagram", "running align.left".
pub fn canvas_action(op: &Value) -> Action {
    let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
    match name {
        "add" => {
            let items = elements(op);
            let kinds: Vec<&str> = items
                .iter()
                .map(|e| e.get("type").and_then(Value::as_str).unwrap_or_default())
                .map(|kind| element_kind(kind).0)
                .collect();
            let (one, many) = match kinds.first() {
                Some(first) if kinds.iter().all(|k| k == first) => {
                    let kind = items[0].get("type").and_then(Value::as_str);
                    element_kind(kind.unwrap_or_default())
                }
                _ => ("element", "elements"),
            };
            Action::of("adding", "added", &count(items.len(), one, many))
        }
        "update" => {
            let items = elements(op);
            let has = |keys: &[&str]| {
                items
                    .iter()
                    .any(|e| keys.iter().any(|k| e.get(*k).is_some()))
            };
            let layers = count(items.len(), "layer", "layers");
            if has(&["text", "label"]) {
                Action::new("editing text", "edited text")
            } else if has(&["width", "height"]) {
                Action::of("resizing", "resized", &layers)
            } else if has(&["x", "y"]) {
                Action::of("moving", "moved", &layers)
            } else if has(&["backgroundColor", "strokeColor"]) {
                Action::of("recolouring", "recoloured", &layers)
            } else {
                Action::of("updating", "updated", &layers)
            }
        }
        "delete" => {
            let n = op.get("ids").and_then(Value::as_array).map_or(0, Vec::len);
            Action::of("deleting", "deleted", &count(n, "layer", "layers"))
        }
        "clear" => Action::new("clearing the canvas", "cleared the canvas"),
        "replace" => Action::new("loading a canvas", "loaded a canvas"),
        "mermaid" => Action::new("drawing a diagram", "drew a diagram"),
        "get" => Action::new("reading the canvas", "read the canvas"),
        "export_svg" | "export_png" => Action::new("exporting the page", "exported the page"),
        "key" => {
            let key = op.get("key").and_then(Value::as_str).unwrap_or("a key");
            Action::of("pressing", "pressed", key)
        }
        "type" => Action::new("typing", "typed"),
        "pointer" => Action::new("using the pointer", "used the pointer"),
        "command" => {
            let id = op
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let params = op.get("params").unwrap_or(&Value::Null);
            match id {
                "command.batch" => {
                    let n = params
                        .get("steps")
                        .or_else(|| params.get("commands"))
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len);
                    Action::of("running", "ran", &count(n, "step", "steps"))
                }
                "image.place" => Action::new("placing an image", "placed an image"),
                "file.import" => Action::new("importing a file", "imported a file"),
                "" => Action::new("running a command", "ran a command"),
                id => Action::of("running", "ran", id),
            }
        }
        _ => Action::new("working", "worked"),
    }
}

/// The path of `url` as an agent's caption names a page it opens: `/checkout`, or the host for
/// its root, cut to 40 characters.
pub fn page_name(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let path = path.split(['?', '#']).next().unwrap_or_default();
    let name = match path {
        "" => host.to_string(),
        path => format!("/{path}"),
    };
    let name = match name.chars().count() > 40 {
        true => format!("{}…", name.chars().take(39).collect::<String>()),
        false => name,
    };
    if name.is_empty() {
        url.to_string()
    } else {
        name
    }
}

/// What an agent's browser op does, in words, as the app tells it without the page: the page's
/// overlay names the element too ("clicking Sign up"), and has the last word for the ops it
/// draws.
pub fn browser_action(op: &crate::browser::BrowserOp) -> Action {
    use crate::browser::BrowserOp as Op;
    match op {
        Op::Navigate { url } => Action::of("opening", "opened", &page_name(url)),
        Op::Back => Action::new("going back", "went back"),
        Op::Forward => Action::new("going forward", "went forward"),
        Op::Reload => Action::new("reloading the page", "reloaded the page"),
        Op::Click { .. } => Action::new("clicking", "clicked"),
        Op::Hover { .. } => Action::new("pointing", "pointed"),
        Op::Type { .. } => Action::new("typing", "typed"),
        Op::Press { key } => Action::of("pressing", "pressed", key),
        Op::Select { .. } => Action::new("choosing an option", "chose an option"),
        Op::Scroll { .. } => Action::new("scrolling", "scrolled"),
        Op::Screenshot { .. } => Action::new("taking a screenshot", "took a screenshot"),
        Op::Snapshot => Action::new("reading the page", "read the page"),
        Op::Query { .. } | Op::Settled { .. } => Action::new("waiting", "waited"),
        Op::Evaluate { .. } => Action::new("running a script", "ran a script"),
        Op::Resize { .. } => Action::new("resizing the page", "resized the page"),
        Op::Inspect { .. } => Action::new("inspecting", "inspected"),
        Op::Design { .. } | Op::ExportDesign { .. } | Op::ExportStatus => {
            Action::new("reading the design", "read the design")
        }
        Op::State => Action::new("checking the page", "checked the page"),
        Op::Console { .. } | Op::Network { .. } => Action::new("reading the logs", "read the logs"),
        Op::Annotations { .. } => Action::new("reading your notes", "read your notes"),
        Op::Devtools | Op::Panel { .. } => Action::new("working", "worked"),
    }
}

/// Whether what an agent's browser op did is worth saying to a screen reader: what changes the
/// page (opening one, clicking, typing), not what only reads it, which an agent does many times
/// a second while it waits for a page.
pub fn browser_op_changes_the_page(op: &crate::browser::BrowserOp) -> bool {
    use crate::browser::BrowserOp as Op;
    matches!(
        op,
        Op::Navigate { .. }
            | Op::Back
            | Op::Forward
            | Op::Reload
            | Op::Click { .. }
            | Op::Hover { .. }
            | Op::Type { .. }
            | Op::Press { .. }
            | Op::Select { .. }
            | Op::Scroll { .. }
            | Op::Resize { .. }
    )
}

/// How many steps of a `command.batch` make it one that takes noticeable time.
pub const SLOW_BATCH: usize = 20;
/// How many elements of an `add` make it one that takes noticeable time.
pub const SLOW_ADD: usize = 50;

/// Whether an agent's canvas op takes noticeable time, and so shows a placeholder where its
/// result will land until it does: a Mermaid diagram's layout, a canvas loaded in, a file
/// imported or an image placed, a `command.batch` of [`SLOW_BATCH`] steps or more, and an `add`
/// of [`SLOW_ADD`] elements or more, or of an image.
pub fn slow_canvas_op(op: &Value) -> bool {
    let name = op.get("op").and_then(Value::as_str).unwrap_or_default();
    match name {
        "mermaid" | "replace" => true,
        "add" => {
            let items = elements(op);
            items.len() >= SLOW_ADD
                || items
                    .iter()
                    .any(|e| e.get("type").and_then(Value::as_str) == Some("image"))
        }
        "command" => {
            let id = op
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let params = op.get("params").unwrap_or(&Value::Null);
            match id {
                "image.place" | "file.import" => true,
                "command.batch" => params
                    .get("steps")
                    .or_else(|| params.get("commands"))
                    .and_then(Value::as_array)
                    .is_some_and(|steps| steps.len() >= SLOW_BATCH),
                _ => false,
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn agent(id: &str, name: &str, project: Option<&str>) -> AgentIdentity {
        AgentIdentity {
            id: id.into(),
            name: name.into(),
            label: project.map(Into::into),
            version: None,
        }
    }

    const fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    /// The clients people use are called what people call them; others get a readable name.
    #[test]
    fn clients_are_called_what_people_call_them() {
        assert_eq!(client_name("claude-code", None), "Claude Code");
        assert_eq!(client_name("codex-mcp-client", Some("Codex")), "Codex");
        assert_eq!(client_name("cursor-vscode", None), "Cursor");
        assert_eq!(client_name("Visual Studio Code", None), "VS Code");
        assert_eq!(client_name("gemini-cli-mcp-client", None), "Gemini CLI");
        assert_eq!(client_name("acme-agent-mcp-client", None), "Acme Agent");
        assert_eq!(client_name("my_tool", Some("  My Tool  ")), "My Tool");
        assert_eq!(client_name("", None), "Agent");
        assert_eq!(client_name("--", None), "Agent");
        let long = client_name(&"x".repeat(80), None);
        assert_eq!(long.chars().count(), MAX_NAME);
    }

    #[test]
    fn a_session_id_is_the_process_and_when_it_started() {
        assert_eq!(session_id(4821, 0x1_2345_6789), "mcp-4821-23456789");
        assert_ne!(session_id(4821, 1), session_id(4821, 2));
    }

    /// Each agent keeps the colour and label it was given when first met, a second agent never
    /// renames the first, and agents of one client are told apart by project, else by number.
    #[test]
    fn colours_and_labels_are_given_once_and_duplicates_are_numbered() {
        let mut roster = Roster::default();
        let first = roster
            .meet(&agent("a", "Claude Code", Some("shop")), secs(0))
            .clone();
        assert_eq!((first.label.as_str(), first.color), ("Claude Code", 0));
        let codex = roster
            .meet(&agent("b", "Codex", Some("shop")), secs(1))
            .clone();
        assert_eq!((codex.label.as_str(), codex.color), ("Codex", 1));
        let blog = roster
            .meet(&agent("c", "Claude Code", Some("blog")), secs(2))
            .clone();
        assert_eq!(blog.label, "Claude Code · blog");
        let again = roster
            .meet(&agent("d", "Claude Code", Some("shop")), secs(3))
            .clone();
        assert_eq!(again.label, "Claude Code 3");
        let unnamed = roster
            .meet(&agent("e", "Claude Code", None), secs(4))
            .clone();
        assert_eq!(unnamed.label, "Claude Code 4");
        // Met again, each is as it was: the same colour and the same label.
        for known in [&first, &codex, &blog, &again, &unnamed] {
            let met = roster.meet(&known.identity, secs(5));
            assert_eq!((&met.label, met.color), (&known.label, known.color));
        }
        let colours: Vec<usize> = roster.all().iter().map(|k| k.color).collect();
        assert_eq!(
            colours,
            [0, 1, 2, 3, 4],
            "five agents at once, five colours"
        );
    }

    /// A colour comes free once its agent has gone quiet for [`GONE_AFTER`], and when all are in
    /// use the next one in turn is shared.
    #[test]
    fn colours_are_reused_once_free() {
        let mut roster = Roster::default();
        for (i, id) in ["a", "b", "c", "d", "e", "f", "g", "h"].iter().enumerate() {
            assert_eq!(roster.meet(&agent(id, id, None), secs(0)).color, i);
        }
        assert_eq!(roster.meet(&agent("i", "i", None), secs(1)).color, 0);
        // `a` to `h` went quiet; `i` (colour 0) too, but `j` meets it at 31 s, and `i` at 1 s.
        roster.meet(&agent("b", "b", None), secs(29));
        let later = roster.meet(&agent("j", "j", None), secs(31)).color;
        assert_eq!(later, 0, "the first colour no recent agent has");
    }

    #[test]
    fn the_anonymous_agent_is_one_agent() {
        let mut roster = Roster::default();
        let one = roster.meet(&AgentIdentity::anonymous(), secs(0)).clone();
        let two = roster.meet(&AgentIdentity::anonymous(), secs(1)).clone();
        assert_eq!(one.color, two.color);
        assert_eq!(roster.all().len(), 1);
        assert_eq!(one.subject(), "An agent");
        let named = roster.meet(&agent("x", "Codex", None), secs(1)).clone();
        assert_eq!(named.subject(), "Codex");
    }

    /// Presence is whole for a few seconds, fades, stays faded, and is gone after 30 s.
    #[test]
    fn presence_fades_then_expires() {
        assert_eq!(opacity(secs(0)), Some(1.0));
        assert_eq!(opacity(FADE_AFTER), Some(1.0));
        let half = opacity(FADE_AFTER + FADE_TIME / 2).unwrap();
        assert!(half < 1.0 && half > FADED, "{half}");
        assert_eq!(opacity(FADE_AFTER + FADE_TIME), Some(FADED));
        assert_eq!(opacity(secs(29)), Some(FADED));
        assert_eq!(opacity(GONE_AFTER), None);
        assert_eq!(opacity(secs(600)), None);
        assert!(shows_action(secs(2)) && !shows_action(secs(10)));
    }

    #[test]
    fn glides_ease_out_and_end() {
        let total = Duration::from_millis(200);
        assert_eq!(glide(Duration::ZERO, total), 0.0);
        let half = glide(total / 2, total);
        assert!(half > 0.5, "eased out: {half}");
        assert_eq!(glide(total, total), 1.0);
        assert_eq!(glide(Duration::ZERO, Duration::ZERO), 1.0, "reduced motion");
    }

    #[test]
    fn canvas_ops_say_what_they_do() {
        let doing = |op: Value| canvas_action(&op).doing;
        let stickies = json!({"op": "add", "elements": [
            {"type": "sticky"}, {"type": "sticky"}, {"type": "sticky"}]});
        assert_eq!(doing(stickies.clone()), "adding 3 stickies");
        assert_eq!(canvas_action(&stickies).done, "added 3 stickies");
        assert_eq!(
            doing(json!({"op": "add", "elements": [{"type": "rectangle"}]})),
            "adding a shape"
        );
        assert_eq!(
            doing(json!({"op": "add", "elements": [{"type": "image"}]})),
            "adding an image"
        );
        assert_eq!(
            doing(json!({"op": "add", "elements": [{"type": "text"}, {"type": "arrow"}]})),
            "adding 2 elements"
        );
        assert_eq!(
            doing(json!({"op": "update", "elements": [{"id": "a", "x": 1}, {"id": "b", "y": 2}]})),
            "moving 2 layers"
        );
        assert_eq!(
            doing(json!({"op": "update", "elements": [{"id": "a", "text": "Hi"}]})),
            "editing text"
        );
        assert_eq!(
            doing(json!({"op": "delete", "ids": ["a"]})),
            "deleting a layer"
        );
        assert_eq!(
            doing(json!({"op": "mermaid", "source": "graph TD"})),
            "drawing a diagram"
        );
        assert_eq!(
            doing(json!({"op": "command", "command": "align.left"})),
            "running align.left"
        );
        assert_eq!(
            doing(
                json!({"op": "command", "command": "command.batch", "params": {"steps": [{}, {}]}})
            ),
            "running 2 steps"
        );
        assert_eq!(doing(json!({"op": "nope"})), "working");
    }

    /// A page an agent opens is named by its path, or its host at the root.
    #[test]
    fn browser_ops_say_what_they_do() {
        use crate::browser::BrowserOp;
        assert_eq!(
            page_name("http://localhost:5173/checkout?step=2"),
            "/checkout"
        );
        assert_eq!(page_name("https://example.com/"), "example.com");
        assert_eq!(page_name("about:blank"), "about:blank");
        let open = browser_action(&BrowserOp::Navigate {
            url: "http://localhost:5173/checkout".into(),
        });
        assert_eq!(open.doing, "opening /checkout");
        let press = browser_action(&BrowserOp::Press {
            key: "Enter".into(),
        });
        assert_eq!(press.done, "pressed Enter");
        assert_eq!(
            browser_action(&BrowserOp::Snapshot).doing,
            "reading the page"
        );
        assert!(browser_op_changes_the_page(&BrowserOp::Press {
            key: "a".into()
        }));
        assert!(!browser_op_changes_the_page(&BrowserOp::State));
        assert!(!browser_op_changes_the_page(&BrowserOp::Snapshot));
    }

    #[test]
    fn slow_ops_are_the_ones_that_take_time() {
        assert!(slow_canvas_op(
            &json!({"op": "mermaid", "source": "graph TD"})
        ));
        assert!(slow_canvas_op(
            &json!({"op": "replace", "path": "/a.sawit"})
        ));
        assert!(slow_canvas_op(
            &json!({"op": "command", "command": "image.place"})
        ));
        let steps: Vec<Value> = (0..SLOW_BATCH).map(|_| json!({})).collect();
        assert!(slow_canvas_op(
            &json!({"op": "command", "command": "command.batch", "params": {"steps": steps}})
        ));
        assert!(!slow_canvas_op(
            &json!({"op": "command", "command": "command.batch", "params": {"steps": [{}]}})
        ));
        assert!(slow_canvas_op(
            &json!({"op": "add", "elements": [{"type": "image"}]})
        ));
        assert!(!slow_canvas_op(
            &json!({"op": "add", "elements": [{"type": "sticky"}]})
        ));
        assert!(!slow_canvas_op(&json!({"op": "get"})));
    }

    /// The identity goes on the wire as `{id, name, label}`, the version only when known.
    #[test]
    fn identities_round_trip() {
        let identity = agent("mcp-1-2", "Codex", Some("shop"));
        let json = serde_json::to_value(&identity).unwrap();
        assert_eq!(
            json,
            json!({"id": "mcp-1-2", "name": "Codex", "label": "shop"})
        );
        assert_eq!(
            serde_json::from_value::<AgentIdentity>(json).unwrap(),
            identity
        );
        let bare: AgentIdentity = serde_json::from_value(json!({"id": "x", "name": "Y"})).unwrap();
        assert_eq!(bare.label, None);
    }
}
