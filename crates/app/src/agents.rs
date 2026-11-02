//! The agents that drive the app through `sawit-mcp`, as every window shows them: each agent's
//! colour and label ([`meet`], from `sawit_core::agents`), whether their cursors show
//! ([`shown`], the View menu's and the toolbars' toggle, ⌃⌘A), whether motion is reduced
//! ([`reduce_motion`]), and the words the app announces for what they did ([`sentence`]).
//!
//! Where each agent is and what it does is the surface's own: the canvas draws its agents
//! (`crate::board`), the browser's page draws its own (sawit-core's `browser/page/overlay.js`).
//! The app only gives them one colour and one name per agent, so an agent reads the same on every
//! surface.

use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::*;
use sawit_core::agents::{AgentIdentity, Known, Roster};
use serde_json::{Value, json};

actions!(
    agents,
    [
        /// Shows or hides the cursors of the agents that drive the canvas and the browser.
        ToggleAgentCursors
    ]
);

/// One agent, as a surface draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct Look {
    pub id: SharedString,
    /// Its label on screen: "Claude Code", "Codex · shop", "Claude Code 2".
    pub label: SharedString,
    /// Its colour, as `#rrggbb` (for the page) and as GPUI paints it.
    pub hex: &'static str,
    pub color: Hsla,
    /// The subject of a sentence about it: its label, or "An agent".
    subject: SharedString,
}

impl Look {
    fn of(known: &Known) -> Self {
        Self {
            id: known.identity.id.clone().into(),
            label: known.label.clone().into(),
            hex: known.color_hex(),
            color: hex_color(known.color_hex()),
            subject: known.subject().to_string().into(),
        }
    }

    /// The subject of a sentence about it: its label, or "An agent".
    pub fn subject(&self) -> &SharedString {
        &self.subject
    }

    /// The agent as the page's overlay takes it: `{id, name, color}`.
    pub fn page_json(&self) -> Value {
        json!({ "id": self.id, "name": self.label, "color": self.hex })
    }

    /// An agent called "Agent `id`", in the first colour.
    #[cfg(test)]
    pub fn for_tests(id: &str) -> Self {
        let label: SharedString = format!("Agent {id}").into();
        Self {
            id: id.to_string().into(),
            label: label.clone(),
            hex: sawit_core::agents::PALETTE[0],
            color: hex_color(sawit_core::agents::PALETTE[0]),
            subject: label,
        }
    }
}

/// What the agents' cursors draw with, for the whole app. A browser watches it, to clear its
/// page's cursors when they are turned off.
pub(crate) struct Agents {
    roster: Roster,
    shown: bool,
    /// Agents' cursors stay in the screenshots agents take of the page.
    in_screenshots: bool,
    /// Where [`shown`] is kept between launches; `None` in tests, where it lasts the run.
    file: Option<PathBuf>,
    /// The roster's clock starts here.
    epoch: Instant,
}

impl Default for Agents {
    fn default() -> Self {
        Self {
            roster: Roster::default(),
            shown: true,
            in_screenshots: false,
            file: None,
            epoch: Instant::now(),
        }
    }
}

impl Global for Agents {}

/// Handles [`ToggleAgentCursors`] wherever it comes from: its key, the View menu, the toolbars.
pub fn init(cx: &mut App) {
    cx.on_action(|_: &ToggleAgentCursors, cx| toggle(cx));
}

/// Reads whether agents' cursors show from `file` (`agent-cursors.json` in the app's folder), and
/// keeps the choice there from now on. Startup does it once; without it, as in tests, the cursors
/// show and a choice lasts the run. The file's `agent_cursors_in_screenshots: true` keeps them in
/// the screenshots agents take of the page, which leave them out otherwise.
pub fn keep_setting(file: PathBuf, cx: &mut App) {
    let saved = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .unwrap_or_default();
    let agents = cx.default_global::<Agents>();
    agents.shown = saved["show_agent_cursors"].as_bool().unwrap_or(true);
    agents.in_screenshots = saved["agent_cursors_in_screenshots"].as_bool() == Some(true);
    agents.file = Some(file);
}

/// Whether agents' cursors stay in the screenshots agents take of the page: only when the user
/// asked for it, and the cursors show.
pub fn in_screenshots(cx: &App) -> bool {
    cx.try_global::<Agents>()
        .is_some_and(|agents| agents.shown && agents.in_screenshots)
}

/// Whether agents' cursors show on the canvas and in the browser. On unless the user turned them
/// off.
pub fn shown(cx: &App) -> bool {
    cx.try_global::<Agents>().is_none_or(|agents| agents.shown)
}

/// Turns agents' cursors on or off, everywhere at once: the canvases redraw, the pages clear
/// theirs (each browser watches this), and the View menu says what its item does now.
pub fn toggle(cx: &mut App) {
    let agents = cx.default_global::<Agents>();
    agents.shown = !agents.shown;
    if let Some(file) = agents.file.clone() {
        let text = json!({
            "show_agent_cursors": agents.shown,
            "agent_cursors_in_screenshots": agents.in_screenshots,
        })
        .to_string();
        cx.background_spawn(async move {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = sawit_core::files::write_atomic(&file, text.as_bytes());
        })
        .detach();
    }
    crate::cli::set_menus(cx);
    cx.refresh_windows();
}

/// The picture of the agent cursors' toggle in the canvas's and the browser's toolbars.
pub const TOGGLE_ICON: gpui_kit::assets::IconName = gpui_kit::assets::IconName::MousePointer2;

/// The toolbars' toggle's tooltip: what a click does now, with the key that does it too as the
/// keymap is now (`Hide agent cursors  ⌃⌘A`).
pub fn toggle_tooltip(window: &Window, cx: &App) -> String {
    let what = if shown(cx) {
        "Hide agent cursors"
    } else {
        "Show agent cursors"
    };
    let keys = crate::keymap::keys_for(&ToggleAgentCursors, "Workspace", window);
    match keys.is_empty() {
        true => what.to_string(),
        false => format!("{what}  {keys}"),
    }
}

/// The View menu's item for [`ToggleAgentCursors`], as it reads now.
pub fn menu_label(cx: &App) -> &'static str {
    if shown(cx) {
        "Hide Agent Cursors"
    } else {
        "Show Agent Cursors"
    }
}

/// The agent `identity` names, met now: the colour and label it has had since the app first met
/// it. An op without an identity (from a `sawit-mcp` that predates them) is the anonymous
/// agent's.
pub fn meet(identity: Option<&AgentIdentity>, cx: &mut App) -> Look {
    let anonymous = AgentIdentity::anonymous();
    let agents = cx.default_global::<Agents>();
    let now = agents.epoch.elapsed();
    Look::of(agents.roster.meet(identity.unwrap_or(&anonymous), now))
}

/// The sentence the app announces for what an agent did: "Claude Code added 3 stickies".
pub fn sentence(look: &Look, done: &str) -> SharedString {
    format!("{} {done}", look.subject).into()
}

/// How often the system's Reduce motion setting is read again.
const MOTION_RECHECK: Duration = Duration::from_secs(2);

thread_local! {
    /// The system's last answer and when it was read.
    static SYSTEM_MOTION: Cell<Option<(Instant, bool)>> = const { Cell::new(None) };
}

/// Whether motion is reduced: GPUI's own setting, `SAWIT_REDUCE_MOTION=1` for this run, or the
/// system's (Accessibility, Display, Reduce motion). Agents' cursors then jump rather than glide,
/// skeletons and placeholders don't shimmer, badges don't spin and text shows whole.
pub fn reduce_motion(cx: &App) -> bool {
    cx.reduce_motion()
        || reduced_for_this_run()
        || SYSTEM_MOTION.with(|cell| match cell.get() {
            Some((at, reduced)) if at.elapsed() < MOTION_RECHECK => reduced,
            _ => {
                let reduced = system_reduces_motion();
                cell.set(Some((Instant::now(), reduced)));
                reduced
            }
        })
}

/// `SAWIT_REDUCE_MOTION` set (to anything but `0`) reduces motion for this run, as the Mac's
/// setting does, so the canvas can be checked under it without changing the Mac's setting.
fn reduced_for_this_run() -> bool {
    static REDUCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *REDUCED.get_or_init(|| {
        std::env::var_os("SAWIT_REDUCE_MOTION").is_some_and(|v| !v.is_empty() && v != "0")
    })
}

#[cfg(all(target_os = "macos", not(test)))]
fn system_reduces_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// Tests run with motion as GPUI says, whatever the Mac running them is set to.
#[cfg(any(not(target_os = "macos"), test))]
fn system_reduces_motion() -> bool {
    false
}

/// `#rrggbb` as GPUI paints it; black for anything else.
pub fn hex_color(hex: &str) -> Hsla {
    let value = hex
        .strip_prefix('#')
        .and_then(|digits| u32::from_str_radix(digits, 16).ok())
        .unwrap_or(0);
    rgb(value).into()
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Rgba, TestAppContext};
    use sawit_core::agents::{AgentIdentity, PALETTE};
    use serde_json::json;

    use super::{hex_color, meet, menu_label, sentence, shown, toggle};

    fn agent(id: &str, name: &str) -> AgentIdentity {
        AgentIdentity {
            id: id.into(),
            name: name.into(),
            label: Some("shop".into()),
            version: None,
        }
    }

    /// Each agent keeps its colour and label for the app's whole run, an op without an identity
    /// is the one anonymous agent's, and a second session of a client is numbered.
    #[gpui_kit::test]
    fn agents_keep_their_colour_and_label(cx: &mut TestAppContext) {
        cx.update(|cx| {
            let claude = meet(Some(&agent("a", "Claude Code")), cx);
            let codex = meet(Some(&agent("b", "Codex")), cx);
            let second = meet(Some(&agent("c", "Claude Code")), cx);
            let old = meet(None, cx);
            assert_eq!(claude.label.as_ref(), "Claude Code");
            assert_eq!(second.label.as_ref(), "Claude Code 2");
            assert_eq!((claude.hex, codex.hex), (PALETTE[0], PALETTE[1]));
            assert_eq!(meet(Some(&agent("a", "Claude Code")), cx), claude);
            assert_eq!(meet(None, cx), old);
            assert_eq!(meet(Some(&agent("b", "Codex")), cx), codex);
            assert_eq!(
                sentence(&codex, "added a sticky").as_ref(),
                "Codex added a sticky"
            );
            assert_eq!(
                sentence(&old, "clicked Sign up").as_ref(),
                "An agent clicked Sign up"
            );
            assert_eq!(
                codex.page_json(),
                json!({"id": "b", "name": "Codex", "color": PALETTE[1]})
            );
        });
    }

    /// The cursors show unless turned off, the toggle turns them over, and the View menu's item
    /// says what it will do.
    #[gpui_kit::test]
    fn the_toggle_turns_the_cursors_over(cx: &mut TestAppContext) {
        cx.update(|cx| {
            assert!(shown(cx));
            assert_eq!(menu_label(cx), "Hide Agent Cursors");
            toggle(cx);
            assert!(!shown(cx));
            assert_eq!(menu_label(cx), "Show Agent Cursors");
            toggle(cx);
            assert!(shown(cx));
        });
    }

    #[test]
    fn colours_read_as_gpui_paints_them() {
        let blue: Rgba = hex_color("#2563eb").into();
        assert!((blue.r - 0x25 as f32 / 255.).abs() < 0.01, "{blue:?}");
        assert!((blue.b - 0xeb as f32 / 255.).abs() < 0.01, "{blue:?}");
        let none: Rgba = hex_color("teal").into();
        assert_eq!((none.r, none.g, none.b), (0., 0., 0.));
    }
}
