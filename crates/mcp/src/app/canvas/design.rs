//! Sawit's design standard, applied without the agent having to ask: the digest with the first
//! UI work of a session and the sections its later works name ([`Sent`], `canvas_begin`), the
//! machine checks after each screen write and when a work is done ([`Sawit::design_checks`]),
//! and the guide as MCP resources and a `design-ui` prompt ([`resources`], [`prompts`]).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use rmcp::model::{
    GetPromptResult, Prompt, PromptArgument, PromptMessage, ReadResourceResult, Resource,
    ResourceContents, Role,
};
use sawit_board::checks::{Finding, MAX_FINDINGS, html};
use sawit_core::skills::bundled::DESIGN_DIGEST;
use sawit_core::skills::digest::sections;
use serde_json::{Value, json};

use super::guide;
use crate::Sawit;

/// The URI of the whole guide; a section's is this, a slash, and its name's slug.
pub const GUIDE_URI: &str = "sawit://design-guide";
/// The prompt that starts UI work with the standard.
pub const PROMPT: &str = "design-ui";

/// What of the standard this session has been sent.
#[derive(Debug, Default)]
pub struct Sent(Mutex<Seen>);

#[derive(Debug, Default)]
struct Seen {
    digest: bool,
    sections: HashSet<String>,
}

/// Words in a work's steps or name that call for a section of the guide.
const SECTION_WORDS: &[(&str, &[&str])] = &[
    (
        "Layout",
        &["layout", "grid", "responsive", "breakpoint", "column"],
    ),
    (
        "Typography",
        &["typography", "type scale", "font", "heading"],
    ),
    (
        "Color",
        &[
            "color",
            "colour",
            "palette",
            "theme",
            "dark mode",
            "contrast",
        ],
    ),
    ("Spacing", &["spacing", "padding", "density"]),
    (
        "Copywriting",
        &["copy", "writing", "microcopy", "wording", "error message"],
    ),
    (
        "Components",
        &[
            "component",
            "button",
            "form",
            "input",
            "field",
            "table",
            "list",
            "card",
            "nav",
            "menu",
            "tab",
            "dialog",
            "modal",
            "sidebar",
            "toolbar",
            "header",
            "footer",
            "search",
        ],
    ),
    (
        "States",
        &[
            "state", "empty", "loading", "error", "hover", "disabled", "skeleton",
        ],
    ),
    (
        "Accessibility",
        &["accessib", "a11y", "focus", "keyboard", "screen reader"],
    ),
    ("Motion", &["motion", "animat", "transition"]),
    ("Polish", &["polish", "shadow", "radius", "icon"]),
];

impl Sent {
    /// The digest, the first time a UI work asks.
    pub fn digest(&self) -> Option<&'static str> {
        let mut seen = self.0.lock().ok()?;
        (!std::mem::replace(&mut seen.digest, true)).then_some(DESIGN_DIGEST)
    }

    /// The sections `words` call for that this session hasn't been sent, two at most, now sent.
    pub fn sections(&self, words: &str) -> Vec<&'static str> {
        let words = words.to_lowercase();
        let Ok(mut seen) = self.0.lock() else {
            return Vec::new();
        };
        let wanted = SECTION_WORDS
            .iter()
            .filter(|(_, keys)| keys.iter().any(|k| words.contains(k)))
            .map(|(name, _)| *name);
        let new: Vec<&str> = wanted
            .filter(|s| !seen.sections.contains(*s))
            .take(2)
            .collect();
        seen.sections.extend(new.iter().map(|s| s.to_string()));
        new
    }
}

/// Whether a work named `name` in a frame `frame` of `kind` is UI: a screen, a frame named or
/// sized like one ([`screen_like`]), or a component named for a UI part (a card, a form...).
pub fn is_ui(kind: Option<&str>, name: &str, frame: &Value) -> bool {
    let kind = kind.map(|k| k.trim().to_ascii_lowercase());
    match kind.as_deref() {
        Some("screen") => return true,
        Some("routes") => return false,
        _ => {}
    }
    const PARTS: &[&str] = &[
        "card", "button", "form", "modal", "dialog", "nav", "navbar", "sidebar", "header",
        "footer", "menu", "table", "list", "hero", "pricing", "toolbar", "tabs", "input",
    ];
    screen_like(name, frame) || words(name).iter().any(|w| PARTS.contains(&w.as_str()))
}

/// Whether a frame `frame` named `name` is a screen's: a phone's size or more, named
/// `<Screen> / <Breakpoint>`, or named for a page.
pub fn screen_like(name: &str, frame: &Value) -> bool {
    let size = |key: &str| frame.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let (w, h) = (size("width"), size("height"));
    let sized = w.min(h) >= 320.0 && w.max(h) >= 480.0;
    const SCREENS: &[&str] = &[
        "screen",
        "page",
        "dashboard",
        "settings",
        "login",
        "signup",
        "checkout",
        "home",
        "landing",
        "profile",
        "onboarding",
        "desktop",
        "mobile",
        "tablet",
        "app",
    ];
    let named = words(name).iter().any(|w| SCREENS.contains(&w.as_str()));
    sized || name.contains(" / ") || named
}

/// The lowercase words of `name`.
fn words(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let split = lower.split(|c: char| !c.is_alphanumeric());
    split
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// What a screen-sized frame drawn from shapes is told.
pub const USE_A_SCREEN: &str = "This frame is a screen's size: build UI as a screen \
     (canvas_begin {kind: \"screen\"}, then canvas_screen_write), which previews live, exports \
     to HTML and React and hands off to a coding agent. Keep vector frames for diagrams, \
     wireframes the user asked to be sketchy, and illustrations.";

/// A guide section's text, without the checklist the digest already holds.
pub fn section_text(name: &str) -> Option<String> {
    let text = guide::text().ok()?;
    let answer = guide::answer(text, Some(name)).ok()?;
    let cut = answer.find("\n## Checklist").unwrap_or(answer.len());
    Some(answer[..cut].trim_end().to_string())
}

/// The findings as an answer's `design_checks`: what failed, and what to do about it.
pub fn checks_value(findings: &[Finding]) -> Value {
    let shown: Vec<&Finding> = findings.iter().take(MAX_FINDINGS).collect();
    let said = match findings.len() {
        0 => "Every machine check of the design standard passes.".to_string(),
        n => format!(
            "{n} failure{} of the design standard (rule ids from canvas_design_guide): fix each, \
             write again, and finish only when none is left.{}",
            if n == 1 { "" } else { "s" },
            if n > MAX_FINDINGS {
                format!(" The first {MAX_FINDINGS} are listed.")
            } else {
                String::new()
            }
        ),
    };
    json!({"passed": findings.is_empty(), "summary": said, "failures": shown})
}

impl Sawit {
    /// The checks of the screen `found` (an answer of the `screen` op: its folder and viewport),
    /// shown on its frame in the app when `shown` (the app is running).
    pub(super) async fn screen_checks(
        &self,
        project: Option<String>,
        file: Option<String>,
        found: &Value,
        shown: bool,
    ) -> Option<Value> {
        let folder = PathBuf::from(found.get("folder")?.as_str()?);
        let width = found["viewport"]["w"].as_f64().unwrap_or(1440.0);
        let checked = crate::blocking(move || html::check_folder(&folder, width)).await;
        let findings = checked.ok()?.ok()?;
        if shown {
            let op = json!({"op": "checks", "frame": found["frame"], "findings": findings});
            let _ = self.canvas(project, file, op).await;
        }
        Some(checks_value(&findings))
    }

    /// The checks of work `work`'s frame when it is done: its screen's, or its layers'.
    pub(super) async fn work_checks(
        &self,
        project: Option<String>,
        file: Option<String>,
        work: &str,
    ) -> Option<Value> {
        let find = json!({"op": "screen", "work": work});
        let found = self.screen_call(project.clone(), file.clone(), find, |at, op| {
            let mut found = at.find(&op)?;
            found["rendered"] = json!(false);
            Ok(found)
        });
        if let Ok(found) = found.await
            && found["ok"] == json!(true)
        {
            let shown = found["rendered"] != json!(false);
            return self.screen_checks(project, file, &found, shown).await;
        }
        let checked = self
            .canvas(project, file, json!({"op": "checks", "work": work}))
            .await;
        let findings: Vec<Finding> =
            serde_json::from_value(checked.ok()?.get("findings")?.clone()).ok()?;
        Some(checks_value(&findings))
    }
}

/// The guide's resources: the whole, and a resource per section.
pub fn resources() -> Vec<Resource> {
    let Ok(text) = guide::text() else {
        return Vec::new();
    };
    let lines: Vec<&str> = text.lines().collect();
    let whole = Resource::new(GUIDE_URI, "design-guide")
        .with_title("Sawit's design standard")
        .with_description(
            "The whole design standard for UI on Sawit's canvas (the sawit-canvas-design skill): \
             rules with ids, values and the checklist",
        )
        .with_mime_type("text/markdown");
    let mut out = vec![whole];
    for name in sections(&lines) {
        let uri = format!("{GUIDE_URI}/{}", slug(name));
        let resource = Resource::new(uri, format!("design-guide-{}", slug(name)))
            .with_title(format!("Design standard: {name}"))
            .with_description(format!("The {name} section of Sawit's design standard"))
            .with_mime_type("text/markdown");
        out.push(resource);
    }
    out
}

/// The text of resource `uri`, or why there is none.
pub fn read(uri: &str) -> Result<ReadResourceResult, String> {
    let text = guide::text()?;
    let body = match uri.strip_prefix(GUIDE_URI) {
        Some("" | "/") => text.to_string(),
        Some(rest) => {
            let wanted = rest.trim_start_matches('/');
            let lines: Vec<&str> = text.lines().collect();
            let name = sections(&lines).into_iter().find(|n| slug(n) == wanted);
            let name = name.ok_or_else(|| format!("No section {wanted} in the design guide"))?;
            guide::answer(text, Some(name))?
        }
        None => return Err(format!("No resource {uri}")),
    };
    let contents = ResourceContents::TextResourceContents {
        uri: uri.to_string(),
        mime_type: Some("text/markdown".into()),
        text: body,
        meta: None,
    };
    Ok(ReadResourceResult::new(vec![contents]))
}

/// A section's name as a URI segment: "Canvas craft" → `canvas-craft`.
pub fn slug(name: &str) -> String {
    let words = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty());
    words.map(str::to_lowercase).collect::<Vec<_>>().join("-")
}

/// The prompts: `design-ui`.
pub fn prompts() -> Vec<Prompt> {
    let what = PromptArgument::new("what")
        .with_description("What to design: \"a settings page for a mobile app\"")
        .with_required(false);
    vec![Prompt::new(
        PROMPT,
        Some("Design UI on Sawit's canvas as a screen, following Sawit's design standard"),
        Some(vec![what]),
    )]
}

/// Prompt `name` with `what`, the thing to design.
pub fn prompt(name: &str, what: Option<&str>) -> Result<GetPromptResult, String> {
    if name != PROMPT {
        return Err(format!("No prompt {name}: the prompt is {PROMPT}"));
    }
    let what = what
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .unwrap_or("the UI the user asks for");
    let text = format!(
        "Design {what} on Sawit's canvas, as a screen: canvas_begin {{kind: \"screen\", name, \
         frame, steps}}, then canvas_screen_write, saying each step with canvas_progress. Follow \
         Sawit's design standard below; read the sections your work needs (canvas_design_guide \
         {{section}}, or the resources under {GUIDE_URI}). Fix every failure the checks after each \
         write report, and finish with canvas_progress {{status: \"done\"}} only when none is \
         left.\n\n{DESIGN_DIGEST}"
    );
    let mut result = GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)]);
    result.description = Some("Design UI on Sawit's canvas with Sawit's design standard".into());
    Ok(result)
}
