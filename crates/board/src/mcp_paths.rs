//! Every canvas function an agent can reach through MCP (B4): for each thing a person can do on
//! the canvas (a main or context menu item, a toolbar tool or flyout choice, a key, a control of
//! the app's own around the canvas), the MCP path that does the same, or why it is the user's
//! alone. The tests fail when a command, a key or a menu item has no path, so a new one can't ship
//! without one; `canvas_commands` lists the app's controls with their paths ([`app_controls`]).

use figcraft_engine::Session;
use serde_json::{Value, json};

use crate::menu::{MenuNode, context_menu, main_menu};
use crate::policy;

/// How an agent reaches a canvas function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum McpPath {
    /// `canvas_command {command: <id>}`, the engine command itself (with the app closed too).
    Command,
    /// Another engine command through `canvas_command`, that does what the item does without its
    /// dialog: `image.place {path}` for Place image….
    CommandAs(&'static str),
    /// A dedicated tool, and what to pass it.
    Tool(&'static str, &'static str),
    /// Nothing to do: Sawit does it for every op (the save).
    Automatic(&'static str),
    /// The user's alone, and why: the view and the page on show, follow mode.
    UserOnly(&'static str),
}

impl McpPath {
    /// The path as `canvas_commands` lists it.
    pub fn to_json(self) -> Value {
        match self {
            McpPath::Command => json!({"via": "canvas_command"}),
            McpPath::CommandAs(how) => json!({"via": "canvas_command", "how": how}),
            McpPath::Tool(tool, how) => json!({"via": tool, "how": how}),
            McpPath::Automatic(why) => json!({"via": "automatic", "why": why}),
            McpPath::UserOnly(why) => json!({"via": "user only", "why": why}),
        }
    }
}

/// Why agents leave the view and the page on show alone ([`crate::ops::agent_refusal`]).
const THE_USERS_VIEW: &str = "the view and the page on show are the user's: agents address pages \
                              and layers by id (page.list, node.get), and read the view in app_state";

/// The paths of the ids `canvas_command` doesn't run as they are: Sawit's own commands, and the
/// engine's that Sawit carries out itself.
const PATHS: &[(&str, McpPath)] = &[
    (
        "file.placeImage",
        McpPath::CommandAs("image.place {path} (the file, not a dialog)"),
    ),
    (
        "file.save",
        McpPath::Automatic("every op is saved to the canvas file at once"),
    ),
    (
        "file.saveAs",
        McpPath::Tool(
            "canvas_export",
            "{format, output_path} writes a copy (.svg, .png)",
        ),
    ),
    (
        policy::ADD_ROUTES,
        McpPath::Tool("canvas_add_routes", "{url?, routes?, viewports?}"),
    ),
    (
        policy::MAXIMIZE,
        McpPath::Tool("app_maximize", "{panel: \"canvas\"}"),
    ),
    (
        policy::EXPORT_HTML,
        McpPath::Tool(
            "canvas_screen_export",
            "{id | work | screen, format: \"html\"}",
        ),
    ),
    (
        policy::EXPORT_REACT,
        McpPath::Tool(
            "canvas_screen_export",
            "{id | work | screen, format: \"react\"}",
        ),
    ),
    (
        policy::BUILD_FULL,
        McpPath::Tool("canvas_settings", "{build_animation: \"full\"}"),
    ),
    (
        policy::BUILD_FAST,
        McpPath::Tool("canvas_settings", "{build_animation: \"fast\"}"),
    ),
    (
        policy::BUILD_OFF,
        McpPath::Tool("canvas_settings", "{build_animation: \"off\"}"),
    ),
    (
        policy::BUILD_SOUND,
        McpPath::Tool("canvas_settings", "{sound}"),
    ),
    (
        policy::SUBAGENT_CURSORS,
        McpPath::Tool("canvas_settings", "{subagent_cursors}"),
    ),
    (
        policy::VARIABLES,
        McpPath::CommandAs("variable.list, variable.create, variable.set, variable.import"),
    ),
    (
        policy::COMMENTS,
        McpPath::CommandAs("comment.list, comment.add, comment.reply, comment.resolve"),
    ),
    (
        policy::START_TIMER,
        McpPath::CommandAs("board.timer {action: \"start\", minutes}"),
    ),
    (
        policy::START_VOTE,
        McpPath::CommandAs("board.vote {action: \"start\", votes}"),
    ),
    (
        policy::COPY_HANDOFF_PROMPT,
        McpPath::Tool("canvas_handoff", "{id | work | screen}: answers the prompt"),
    ),
    (
        policy::REVEAL_HANDOFF,
        McpPath::Tool("canvas_handoff", "{id | work | screen}: answers the folder"),
    ),
    ("page.select", McpPath::UserOnly(THE_USERS_VIEW)),
    // Copy as: the clipboard is the user's; the agent reads the same things directly.
    (
        "edit.copyAsText",
        McpPath::Tool("canvas_get", "the layers' text"),
    ),
    (
        "edit.copyAsCss",
        McpPath::CommandAs("inspect.code {id, language: \"css\"}"),
    ),
    (
        "edit.copyAsSvg",
        McpPath::Tool("canvas_export", "{format: \"svg\"}"),
    ),
    (
        "edit.copyAsPng",
        McpPath::Tool("canvas_export", "{format: \"png\"}"),
    ),
];

/// The controls of the app's own around the canvas, which no engine command is: each with its
/// MCP path. The control audit (`docs/notes/2026-11-07-canvas-controls.md`) lists them all.
pub const APP_CONTROLS: &[(&str, McpPath)] = &[
    (
        "Preview (screen bar)",
        McpPath::Tool(
            "canvas_screen_get",
            "{work | screen}: the page as rendered, as a PNG",
        ),
    ),
    (
        "Present",
        McpPath::UserOnly(
            "Present and Preview are the user's: app_state's canvases[].view says whether they show; canvas_screen_get and canvas_export picture what they would",
        ),
    ),
    (
        "Live (route or screen frame)",
        McpPath::Tool(
            "canvas_screen_get",
            "reads the live page; agents never take the user's keyboard",
        ),
    ),
    (
        "Reload (route frame bar)",
        McpPath::Tool("canvas_add_routes", "laying a route out again reloads it"),
    ),
    (
        "Send to Agent…",
        McpPath::Tool("canvas_handoff", "{id | work | screen, agent?}"),
    ),
    (
        "Export HTML / Export React",
        McpPath::Tool("canvas_screen_export", "{id | work | screen, format}"),
    ),
    (
        "Edit screen HTML",
        McpPath::Tool(
            "canvas_screen_write",
            "{work | screen, html | edits | files}",
        ),
    ),
    (
        "Version history",
        McpPath::CommandAs("version.list, version.restore, version.rename"),
    ),
    (
        "Dev Mode and Inspect",
        McpPath::Tool(
            "canvas_get",
            "layers with their sizes, styles and code (frame_code via canvas_screen_export)",
        ),
    ),
    (
        "Layers panel: rename, hide, lock, reorder",
        McpPath::CommandAs(
            "object.rename, object.toggleVisible, object.toggleLock, node.reorder; or canvas_update",
        ),
    ),
    (
        "Design panel fields",
        McpPath::Tool(
            "canvas_update",
            "{elements: [{id, …}]}, or node.set {id, …}",
        ),
    ),
    (
        "Prototype tab: interactions and flows",
        McpPath::CommandAs(
            "proto.interaction.add / set / remove, proto.flow.add / remove, proto.settings",
        ),
    ),
    (
        "Timer and vote pill",
        McpPath::CommandAs("board.timer, board.vote, board.voteResults"),
    ),
    (
        "Follow agent (toolbar)",
        McpPath::UserOnly(
            "only the user turns it on or off; agents read it in app_state's canvases[].view.follow",
        ),
    ),
    ("Pan and zoom", McpPath::UserOnly(THE_USERS_VIEW)),
];

/// The MCP path of canvas function `id` (a command's id), if it has one.
pub fn mcp_path(id: &str) -> Option<McpPath> {
    if let Some((_, path)) = PATHS.iter().find(|(own, _)| *own == id) {
        return Some(*path);
    }
    if id.starts_with("view.") {
        return Some(McpPath::UserOnly(THE_USERS_VIEW));
    }
    if crate::ops::agent_can_run(id) {
        return Some(McpPath::Command);
    }
    if policy::SEND_TO.iter().any(|(own, _)| *own == id) {
        return Some(McpPath::Tool(
            "canvas_handoff",
            "{id | work | screen, agent}",
        ));
    }
    None
}

/// Every id a person can reach on canvases like `sessions`: the main menu's and the context menu's
/// items, the toolbar's tools and flyout choices of both kinds of file, every key, and Sawit's own
/// commands. Sorted, each once.
pub fn person_reachable(sessions: &[&Session]) -> Vec<&'static str> {
    let mut ids: Vec<&'static str> = Vec::new();
    for s in sessions {
        walk(&main_menu(s), &mut ids);
        walk(&context_menu(s), &mut ids);
        for kind in [crate::DocKind::Board, crate::DocKind::Design] {
            for group in crate::toolbar::groups(kind) {
                ids.extend(group.tools.iter().map(|tool| tool.command));
            }
        }
    }
    ids.push(crate::toolbar::PLACE_IMAGE);
    ids.extend(crate::shortcuts::defaults().map(|keys| keys.id));
    ids.extend(policy::SAWIT_COMMANDS.iter().map(|(id, ..)| *id));
    ids.extend(policy::UI_COMMANDS);
    ids.extend(policy::CLIPBOARD);
    ids.extend(policy::SEND_TO.iter().map(|(id, _)| *id));
    ids.sort_unstable();
    ids.dedup();
    // Hidden commands are no one's on the canvas: out of the menus and dead on the keyboard.
    ids.retain(|id| !policy::is_hidden(id));
    ids
}

fn walk(nodes: &[MenuNode], ids: &mut Vec<&'static str>) {
    for node in nodes {
        match node {
            MenuNode::Item { id, .. } => ids.push(id),
            MenuNode::Submenu { children, .. } => walk(children, ids),
            MenuNode::Separator => {}
        }
    }
}

/// The app's controls with their paths, as `canvas_commands` lists them.
pub fn app_controls() -> Value {
    json!(
        APP_CONTROLS
            .iter()
            .map(|(control, path)| {
                let mut v = path.to_json();
                v["control"] = json!(control);
                v
            })
            .collect::<Vec<_>>()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BoardDoc;

    /// B4: every function a person can reach on the canvas has an MCP path. A new command, menu
    /// item, tool or key without one fails here: add it to [`PATHS`] (or make it an engine
    /// command `canvas_command` runs).
    #[test]
    fn every_canvas_function_has_an_mcp_path() {
        let board = BoardDoc::new_board("Board").unwrap();
        let mut design = BoardDoc::new_board("Design").unwrap();
        design.session.doc_mut().unwrap().kind = figcraft_doc::FileKind::Design;
        // A selection, for the context menu's items for layers.
        let made = design
            .execute(
                "frame.create",
                &json!({"x": 0, "y": 0, "width": 100, "height": 100}),
            )
            .unwrap();
        let id = made["id"].as_u64().unwrap();
        design.execute("select.set", &json!({"ids": [id]})).ok();
        let ids = person_reachable(&[&board.session, &design.session]);
        assert!(
            ids.len() > 150,
            "the canvas's functions are all found: {}",
            ids.len()
        );
        let missing: Vec<&str> = ids
            .iter()
            .copied()
            .filter(|id| mcp_path(id).is_none())
            .collect();
        assert!(missing.is_empty(), "no MCP path for: {missing:?}");
    }

    /// Every engine command an agent may run is listed by `canvas_commands`, so `canvas_command`
    /// reaches the whole engine but what Sawit hides or does itself.
    #[test]
    fn every_engine_command_is_reachable_or_the_users() {
        let unreachable: Vec<&str> = figcraft_engine::cmd::all_commands()
            .iter()
            .map(|c| c.id)
            .filter(|id| !policy::is_hidden(id))
            .filter(|id| mcp_path(id).is_none())
            .filter(|id| !crate::ops::is_document_command(id))
            .collect();
        assert!(unreachable.is_empty(), "{unreachable:?}");
    }

    #[test]
    fn the_app_controls_name_their_paths() {
        let listed = app_controls();
        let controls = listed.as_array().unwrap();
        assert_eq!(controls.len(), APP_CONTROLS.len());
        assert!(
            controls
                .iter()
                .all(|c| c["via"].is_string() && c["control"].is_string())
        );
    }
}
