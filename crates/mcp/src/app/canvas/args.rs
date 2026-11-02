//! The canvas tools' arguments, and what their `file` and `project` name.

use rmcp::schemars;
use serde_json::Value;

/// What a canvas tool's `file` names: a canvas file to work on instead of the project's canvas
/// ([`canvas_file`](super::files::canvas_file)).
const CANVAS_FILE: &str = "A canvas file to work on instead of the project's canvas: an absolute path (or ~/…) to a .sawit file (a design file or a board; one not there yet, in a folder that is, starts as a new board) or to an .excalidraw file, imported once into the .sawit file of its name beside it (flow.sawit for flow.excalidraw) and left as it is. With the app running the op runs on that file's canvas tab, opened if need be without taking the user's keyboard; with it closed, on the file itself. Its backups go beside it (home.sawit.bak and home.sawit.del.bak for home.sawit): in a repository they show in git status, and staging everything would commit them";

/// What a canvas tool's `project` names: the project whose canvas it uses ([`crate::agent_project`]).
const CANVAS_PROJECT: &str = "Project folder whose canvas (.nyawit/canvas.sawit) to use, in the window that has it open. Default: the project your working folder is in (with the app closed: within its git repository, the nearest folder up from it whose .nyawit/ holds a canvas, else the repository's root; outside a repository, the folder itself)";

/// What a canvas tool's `work` names: the work item its op belongs to ([`CanvasBeginArgs`]).
const CANVAS_WORK: &str = "The work this belongs to, as canvas_begin answered it (\"w_7f3a91c2\"): the op goes inside that work's frame, and its row on the canvas shows the agent at work. With a work, x and y are from the frame's top-left corner, and what is drawn goes into the frame";

/// What `canvas_delete`'s and `canvas_command`'s `work` names.
const CANVAS_WORK_TAG: &str = "The work this belongs to, as canvas_begin answered it: its row on the canvas shows the agent at work, and the undo step joins the work's";

/// canvas_get's and canvas_clear's arguments: which canvas.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasTargetArgs {
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasElementsArgs {
    #[schemars(
        description = "Element skeletons, e.g. {\"type\":\"rectangle\",\"id\":\"a\",\"x\":0,\"y\":0,\"width\":160,\"height\":60,\"label\":{\"text\":\"API\"}}, {\"type\":\"arrow\",\"x\":160,\"y\":30,\"start\":{\"id\":\"a\"},\"end\":{\"id\":\"b\"}}, {\"type\":\"text\",\"x\":0,\"y\":-40,\"text\":\"Title\"}. Types: rectangle, ellipse, diamond, sticky, text, arrow, line, frame. With no colour given, shapes and stickies are sticky yellow (#FFE78A): pass backgroundColor for another, \"transparent\" for no fill. A line through more than two points is drawn as ink, #1E1E1E unless strokeColor names another. Arrows and lines run straight"
    )]
    pub elements: Vec<Value>,
    #[schemars(description = CANVAS_WORK)]
    pub work: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// The arguments of `canvas_add_diagram`, and of `canvas_add_mermaid`, its old name.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasMermaidArgs {
    #[schemars(
        description = "The diagram in Mermaid syntax, starting with its type: flowchart, sequenceDiagram, architecture-beta, C4Context, venn-beta, … (any type Pele draws)"
    )]
    pub source: String,
    #[schemars(description = "Where to put it; default right of the existing content")]
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[schemars(description = CANVAS_WORK)]
    pub work: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasUpdateArgs {
    #[schemars(
        description = "Changes per element: {\"id\":\"a\", \"x\":10, \"text\":\"New label\", \"strokeColor\":\"#e03131\", \"backgroundColor\":\"#ffc9c9\", ...}. {\"id\":\"a\", \"append\":\" more words\"} adds text to the end of the layer's, so long copy can arrive in parts"
    )]
    pub elements: Vec<Value>,
    #[schemars(description = CANVAS_WORK)]
    pub work: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasDeleteArgs {
    #[schemars(description = "Element ids (from canvas_get)")]
    pub ids: Vec<String>,
    #[schemars(description = CANVAS_WORK_TAG)]
    pub work: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasLoadArgs {
    #[schemars(
        description = "A .sawit file to load, or an .excalidraw file to import (replaces the board). A backup Sawit keeps loads too: beside the canvas file, <file>.bak holds the board the last clear or load took off, and <file>.del.bak the board the last deletion took layers off (canvas.sawit.bak and canvas.sawit.del.bak for the project's canvas), with the ones before them in .bak.1 to .bak.4 and .del.bak.1 to .del.bak.4"
    )]
    pub path: Option<String>,
    #[schemars(description = "Or the .excalidraw scene JSON itself")]
    pub scene: Option<Value>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasCommandArgs {
    #[schemars(
        description = "Command id, as canvas_commands lists them: board.sticky, node.set, align.left, edit.undo, ..."
    )]
    pub command: String,
    #[schemars(
        description = "The command's parameters as an object, e.g. {\"x\": 0, \"y\": 0, \"text\": \"Idea\"} for board.sticky"
    )]
    pub params: Option<Value>,
    #[schemars(description = CANVAS_WORK_TAG)]
    pub work: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasCommandsArgs {
    #[schemars(
        description = "Only the commands whose id or label contains this text (any case), such as board. or align, each with the parameters it takes. Without it: every command's id and label"
    )]
    pub filter: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// canvas_design_guide's arguments: the guide reads no canvas.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasDesignGuideArgs {
    #[schemars(
        description = "A heading of the guide, such as Typography: only that section, with the Checklist section after it. A name no heading has lists the headings. Default: the whole guide"
    )]
    pub section: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasExportArgs {
    #[schemars(description = "svg (default) or png")]
    pub format: Option<String>,
    #[schemars(
        description = "Absolute path (or ~/…) to write the file to; default the project's .nyawit/exports folder (Sawit's cache folder outside a project)"
    )]
    pub output_path: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// `canvas_begin`'s arguments: the component an agent is about to build.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasBeginArgs {
    #[schemars(
        description = "The component's name, as the canvas shows it on its frame: \"Pricing card\""
    )]
    pub name: String,
    #[schemars(
        description = "Where its frame goes: {\"x\": 0, \"y\": 0, \"width\": 320, \"height\": 240} in page units on the page on show, or beside a layer: {\"near\": \"<id>\", \"side\": \"right\" | \"left\" | \"above\" | \"below\", \"width\"?, \"height\"?}. The size is 320 × 240 when not given"
    )]
    pub frame: Value,
    #[schemars(
        description = "The steps it goes through, shown one at a time on its row: [\"Laying out\", \"Writing copy\", \"Styling\"]. canvas_progress says which is running"
    )]
    pub steps: Option<Vec<String>>,
    #[schemars(
        description = "\"component\" (the default): a frame you draw into with the canvas tools. \"screen\": a UI screen, an HTML page in .nyawit/screens/<screen>/ shown in the frame (1440 × 900 when `frame` names no size, its CSS viewport); write it with canvas_screen_write and the answer's `screen`"
    )]
    pub kind: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// `canvas_progress`'s arguments: where a work stands.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasProgressArgs {
    #[schemars(description = "The work, as canvas_begin answered it")]
    pub work: String,
    #[schemars(description = "The step now running: one of the work's steps, or other words")]
    pub step: Option<String>,
    #[schemars(
        description = "working, done or failed. A failed work keeps its message on the canvas until you retry (any op or progress of the work) or the user dismisses it"
    )]
    pub status: Option<String>,
    #[schemars(
        description = "Why it failed, in a few plain words: \"Couldn't load the logo image\""
    )]
    pub message: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// What names a screen: the work that made it, or its name.
const SCREEN_WORK: &str = "The work canvas_begin {kind: \"screen\"} answered (\"w_7f3a91c2\"): the screen it made, and its row on the canvas shows you at work";
const SCREEN_NAME: &str = "The screen's name: its folder's (\"pricing-page\", as canvas_begin answered it) or its frame's (\"Pricing page\"). Give `work` or `screen`";

/// `canvas_screen_write`'s arguments.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasScreenWriteArgs {
    #[schemars(description = SCREEN_WORK)]
    pub work: Option<String>,
    #[schemars(description = SCREEN_NAME)]
    pub screen: Option<String>,
    #[schemars(
        description = "The whole page, index.html: a complete HTML document. Relative links reach the screen's other files (css/site.css, img/logo.png); nothing loads from the network"
    )]
    pub html: Option<String>,
    #[schemars(
        description = "Changes instead of the whole page, in order: [{\"find\": \"<h1>Hi</h1>\", \"replace\": \"<h1>Hello</h1>\"}]. Each `find` must be in the file exactly once; `path` picks another file than index.html. If one fails, nothing is written"
    )]
    pub edits: Option<Vec<Value>>,
    #[schemars(
        description = "Other files of the screen, whole: [{\"path\": \"css/site.css\", \"content\": \"...\"}], or an image as {\"path\": \"img/logo.png\", \"base64\": \"...\"}. Paths are from the screen's folder"
    )]
    pub files: Option<Vec<Value>>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// `canvas_screen_get`'s arguments.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasScreenGetArgs {
    #[schemars(description = SCREEN_WORK)]
    pub work: Option<String>,
    #[schemars(
        description = "The screen's name: its folder's (\"pricing-page\") or its frame's (\"Pricing page\"). Without `work` or `screen`, the screens on the canvas are listed"
    )]
    pub screen: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}

/// `canvas_screen_export`'s arguments.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CanvasScreenExportArgs {
    #[schemars(description = SCREEN_WORK)]
    pub work: Option<String>,
    #[schemars(description = SCREEN_NAME)]
    pub screen: Option<String>,
    #[schemars(
        description = "html (default): one .html file with its CSS and images inlined. react: a .tsx component, its .css beside it, and the images it uses in a <Component>-assets folder beside them"
    )]
    pub format: Option<String>,
    #[schemars(
        description = "Absolute path (or ~/…) of the .html or .tsx file to write; default the project's .nyawit/exports folder"
    )]
    pub path: Option<String>,
    #[schemars(description = CANVAS_FILE)]
    pub file: Option<String>,
    #[schemars(description = CANVAS_PROJECT)]
    pub project: Option<String>,
}
