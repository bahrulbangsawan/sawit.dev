//! Diagrams written in Mermaid syntax, drawn to SVG by Pele (https://pele.run),
//! the MIT-licensed renderer from Obsidian. Mermaid is only the syntax here:
//! no part of mermaid.js is bundled or called. Pele is JavaScript, so it runs
//! in an embedded QuickJS engine on its own thread; the bundle is compiled into
//! the binary, so diagrams render offline. Every type Pele draws ([`TYPES`]) is
//! drawn here, and so by every user of [`render`]: the canvas, the Markdown and
//! `.mmd` previews, the schema diagram and the MCP tools.
//!
//! Pele themes its SVG with CSS variables (and a `color-mix()` or two), which
//! the SVG rasterizer behind GPUI images (usvg) doesn't understand, so
//! [`flatten`] bakes the theme's colors into the markup. The result is a
//! standalone SVG that any viewer draws the same way, which is also what
//! "Save Diagram as SVG…" saves.
//!
//! Pele's own limits bound what one render costs: the source's length, the
//! edges of a flowchart or an agentflow, and the SVG's length ([`SOURCE_LIMIT`],
//! [`MAX_EDGES`], [`OUTPUT_LIMIT`]). A diagram past one of them is refused at
//! once, in words that say so, instead of holding the worker until
//! [`RENDER_TIMEOUT`] stops it.

mod flattening;
mod markdown;
mod pele;
#[cfg(test)]
mod tests;

use std::time::Duration;

use pele::run;

pub use flattening::flatten;
pub use markdown::{Fence, IMAGE_SCHEME, SAVE_SCHEME, fences, index_in, rewrite_markdown};

/// The version of Pele that [`PELE_JS`] is.
pub const PELE_VERSION: &str = "1.13.0";

/// Pele 1.13.0, `dist/pele.global.js` (defines the global `pele`). The canvas
/// turns the SVG this copy draws into shapes (`sawit_board::mermaid`).
pub const PELE_JS: &str = include_str!("../../assets/pele/pele.global.js");

/// Every diagram type Pele draws, by the keyword its Mermaid source starts
/// with, and Pele's name for the type, which [`Diagram::kind`] is (Pele's
/// compatibility table, https://pele.run/compatibility). ZenUML and other
/// Mermaid plugins aren't among them.
pub const TYPES: &[(&str, &str)] = &[
    ("flowchart", "flowchart"),
    ("graph", "flowchart"),
    ("flowchart-elk", "flowchart"),
    ("swimlane-beta", "swimlane"),
    ("sequenceDiagram", "sequence"),
    ("classDiagram", "class"),
    ("stateDiagram", "state"),
    ("stateDiagram-v2", "state"),
    ("erDiagram", "er"),
    ("gantt", "gantt"),
    ("gitGraph", "gitGraph"),
    ("pie", "pie"),
    ("mindmap", "mindmap"),
    ("kanban", "kanban"),
    ("timeline", "timeline"),
    ("journey", "journey"),
    ("quadrantChart", "quadrantChart"),
    ("xychart", "xychart"),
    ("xychart-beta", "xychart"),
    ("requirementDiagram", "requirement"),
    ("sankey", "sankey"),
    ("sankey-beta", "sankey"),
    ("architecture-beta", "architecture"),
    ("C4Context", "c4"),
    ("C4Container", "c4"),
    ("C4Component", "c4"),
    ("C4Dynamic", "c4"),
    ("C4Deployment", "c4"),
    ("block", "block"),
    ("block-beta", "block"),
    ("agentflow-beta", "agentflow"),
    ("railroad-beta", "railroad"),
    ("railroad-ebnf-beta", "railroad"),
    ("railroad-abnf-beta", "railroad"),
    ("railroad-peg-beta", "railroad"),
    ("treeView-beta", "treeView"),
    ("cynefin-beta", "cynefin"),
    ("wardley-beta", "wardley"),
    ("eventmodeling", "eventmodeling"),
    ("usecase-beta", "usecase"),
    ("venn-beta", "venn"),
    ("ishikawa", "ishikawa"),
    ("ishikawa-beta", "ishikawa"),
    ("packet", "packet"),
    ("packet-beta", "packet"),
    ("radar-beta", "radar"),
    ("treemap", "treemap"),
    ("treemap-beta", "treemap"),
    ("info", "info"),
];

/// A runaway diagram is interrupted after this long.
const RENDER_TIMEOUT: Duration = Duration::from_secs(5);

/// The longest source Pele takes, in characters (Pele's own default): a longer
/// one is refused before it is parsed.
pub const SOURCE_LIMIT: u32 = 50_000;

/// The most edges a flowchart or an agentflow may have. Pele's default is
/// 5,000, but Pele runs here in QuickJS's interpreter, much slower than a
/// browser's JIT: a flowchart of a few thousand edges held the worker for
/// seconds, and every diagram behind it waited. Past this many it is refused at
/// once, and the layout of one at the limit stays well inside [`RENDER_TIMEOUT`].
pub const MAX_EDGES: u32 = 2_000;

/// The longest SVG Pele writes, in characters (Pele's own default). A bigger
/// one is no picture a preview or the canvas can show in one piece.
pub const OUTPUT_LIMIT: u32 = 4_000_000;

/// How a [`render`] error ends when the diagram ran past [`RENDER_TIMEOUT`] and was stopped
/// ([`took_too_long`]).
const TOOK_TOO_LONG: &str = "it took too long.";

/// Whether `error`, as [`render`] gave it, says the diagram was stopped for running past the
/// render's time limit: another try of the same diagram takes as long again.
pub fn took_too_long(error: &str) -> bool {
    error.ends_with(TOOK_TOO_LONG)
}

/// The JS heap Pele may use.
const MEMORY_LIMIT: usize = 256 << 20;

/// Colors for Pele's `--pele-*` variables, as CSS colors (`#rrggbb`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Palette {
    pub background: String,
    pub foreground: String,
    pub muted: String,
    pub line: String,
    pub surface: String,
    pub surface_alt: String,
    pub border: String,
    pub accent: String,
    pub critical: String,
}

impl Default for Palette {
    /// Pele's own light defaults.
    fn default() -> Self {
        Palette {
            background: "#ffffff".into(),
            foreground: "#1f1f1f".into(),
            muted: "#6e6e6e".into(),
            line: "#1f1f1f".into(),
            surface: "#f3f3f3".into(),
            surface_alt: "#e6e6e6".into(),
            border: "#8a8a8a".into(),
            accent: "#5b7bd5".into(),
            critical: "#e45756".into(),
        }
    }
}

impl Palette {
    fn vars(&self) -> [(&'static str, &str); 9] {
        [
            ("--pele-bg", &self.background),
            ("--pele-fg", &self.foreground),
            ("--pele-muted", &self.muted),
            ("--pele-line", &self.line),
            ("--pele-surface", &self.surface),
            ("--pele-surface-alt", &self.surface_alt),
            ("--pele-border", &self.border),
            ("--pele-accent", &self.accent),
            ("--pele-critical", &self.critical),
        ]
    }
}

/// A rendered diagram: a standalone SVG with the palette baked in.
#[derive(Debug, Clone)]
pub struct Diagram {
    pub svg: String,
    pub width: f64,
    pub height: f64,
    /// Pele's name for the diagram type, such as `flowchart`.
    pub kind: String,
}

/// Renders Mermaid `source`. Blocks until done (well under a millisecond for
/// typical diagrams; the first call also loads Pele, about 60 ms), so call it
/// off the UI thread. The error is Pele's message, which quotes the source.
pub fn render(source: &str, palette: &Palette) -> Result<Diagram, String> {
    let raw = run(source.to_owned())?;
    Ok(Diagram {
        svg: flatten(&raw.svg, palette),
        width: raw.width,
        height: raw.height,
        kind: raw.kind,
    })
}

struct Raw {
    svg: String,
    width: f64,
    height: f64,
    kind: String,
}

/// Why `source` can't be drawn when Pele finds no type it draws in it: what
/// the source starts with, and the types it can start with instead. ZenUML,
/// which mermaid.js draws with a plugin, is named as such.
pub fn unsupported(source: &str) -> String {
    let keywords: Vec<&str> = TYPES.iter().map(|(keyword, _)| *keyword).collect();
    let types = keywords.join(", ");
    match first_word(source) {
        None => format!("The diagram is empty. Start it with its type: {types}."),
        Some(word) if word.eq_ignore_ascii_case("zenuml") => format!(
            "ZenUML diagrams can't be drawn: Sawit draws Mermaid syntax with Pele, which has no \
             ZenUML or other Mermaid plugins. Start the diagram with one of: {types}."
        ),
        Some(word) => format!(
            "“{word}” isn't a diagram type Sawit can draw. Start the diagram with one of: {types}."
        ),
    }
}

/// The first word of a Mermaid source, past its front matter (`---` … `---`),
/// its `%%` comments and directives and its blank lines: the diagram's type.
fn first_word(source: &str) -> Option<&str> {
    let mut lines = source.lines().map(str::trim);
    let mut in_front_matter = false;
    let mut first = true;
    for line in lines.by_ref() {
        if std::mem::take(&mut first) && line == "---" {
            in_front_matter = true;
            continue;
        }
        if in_front_matter {
            in_front_matter = line != "---";
            continue;
        }
        if line.is_empty() || line.starts_with("%%") {
            continue;
        }
        return line.split_whitespace().next();
    }
    None
}
