//! The canvas tools. Each runs an op of `sawit_board::ops` on a canvas: the one the running app
//! shows for the project, or for a canvas file an agent names, and with the app closed the stored
//! canvas file itself ([`dispatch`]).

mod args;
mod dispatch;
mod files;
mod stored;
#[cfg(test)]
mod tests;

use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use sawit_board::store;
use serde_json::{Value, json};

pub use args::{
    CanvasCommandArgs, CanvasCommandsArgs, CanvasDeleteArgs, CanvasElementsArgs, CanvasExportArgs,
    CanvasLoadArgs, CanvasMermaidArgs, CanvasTargetArgs, CanvasUpdateArgs,
};
use dispatch::command_list;
use files::{absolute_path, canvas_file, export_path, is_sawit_file, read_scene};

use super::{base64_decode, pretty};
use crate::{Sawit, blocking, fail, ok, tri};

/// What `canvas_add_diagram` says it does, and `canvas_add_mermaid` after the word that it is the
/// old name. How each type comes in is `sawit_board::mermaid::WAYS`.
macro_rules! canvas_add_diagram_says {
    () => {
        concat!(
            "Draw a diagram written in Mermaid syntax on Sawit's canvas as shapes you can edit: \
             boxes, connectors bound to them, labels. Pele (https://pele.run) lays it out offline; \
             mermaid.js isn't used. Every type Pele draws comes in: ",
            diagram_types!(),
            " (not ZenUML or other Mermaid plugins). Flowcharts, swimlanes, sequence, class, \
             state, ER, mindmaps, kanban, block, requirement, C4, git graphs, timelines, \
             architecture, agentflow, use case, event modeling, Ishikawa, packet and quadrant \
             charts come in as shapes; tree views, railroads, Wardley maps, Cynefin and Venn \
             diagrams as shapes over one picture of their frame (tracks, axes, regions, sets); \
             charts whose shapes are their data (journey, pie, gantt, xychart, sankey, radar, \
             treemap, info) as one picture. Icons and turned text come in as small pictures. \
             Answers the ids of what it drew. For an SVG file of a diagram use render_diagram"
        )
    };
}

#[tool_router(router = canvas_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "List the top-level layers of the canvas's page on show: id, type, position, size, text, color and what a connector joins. For the other pages and nested layers use canvas_command: page.list lists the pages, node.get {id} reads a page's or a layer's own layers, and page.select shows another page (with the app closed, only for the rest of one command.batch). Works from the saved file when the app isn't running; the page on show is then the one last saved"
    )]
    async fn canvas_get(&self, Parameters(a): Parameters<CanvasTargetArgs>) -> CallToolResult {
        self.canvas_get_tool(a).await
    }

    #[tool(
        description = "Draw on Sawit's canvas: add shapes, stickies, labels, text and arrows (arrows bind to shapes by id). Shapes and stickies with no backgroundColor come in sticky yellow. Opens the canvas tab when the app is running"
    )]
    async fn canvas_add_elements(
        &self,
        Parameters(a): Parameters<CanvasElementsArgs>,
    ) -> CallToolResult {
        match self
            .canvas(
                a.project,
                a.file,
                json!({ "op": "add", "elements": a.elements }),
            )
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = canvas_add_diagram_says!())]
    async fn canvas_add_diagram(
        &self,
        Parameters(a): Parameters<CanvasMermaidArgs>,
    ) -> CallToolResult {
        self.canvas_add_diagram_tool(a).await
    }

    #[tool(description = concat!(
        "Deprecated: the old name of canvas_add_diagram, which takes the same arguments; use \
         canvas_add_diagram. ",
        canvas_add_diagram_says!()
    ))]
    async fn canvas_add_mermaid(
        &self,
        Parameters(a): Parameters<CanvasMermaidArgs>,
    ) -> CallToolResult {
        self.canvas_add_diagram_tool(a).await
    }

    #[tool(description = "Change canvas elements by id: position, size, colors, text")]
    async fn canvas_update(&self, Parameters(a): Parameters<CanvasUpdateArgs>) -> CallToolResult {
        match self
            .canvas(
                a.project,
                a.file,
                json!({ "op": "update", "elements": a.elements }),
            )
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Delete canvas elements by id, on any page (a connector's label goes with it). The board they were on is first kept beside the canvas file as <file>.del.bak (canvas.sawit.del.bak for the project's canvas, home.sawit.del.bak for home.sawit), the 4 kept before it moving to .del.bak.1 to .del.bak.4 (the oldest goes). Deletions keep these apart from the .bak files canvas_clear and canvas_load keep, so they never push those out; a deletion that leaves the canvas empty keeps a .bak, as a clear does. canvas_load brings any of them back; in the app the user can also undo it"
    )]
    async fn canvas_delete(&self, Parameters(a): Parameters<CanvasDeleteArgs>) -> CallToolResult {
        match self
            .canvas(a.project, a.file, json!({ "op": "delete", "ids": a.ids }))
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Delete every layer of the canvas's page on show; other pages keep theirs. The board it held is kept beside the canvas file as <file>.bak (canvas.sawit.bak for the project's canvas, home.sawit.bak for home.sawit), the 4 kept before it moving to .bak.1 to .bak.4 (the oldest goes); only a clear, a load or a save that empties the canvas moves them along, never a deletion that leaves layers on the canvas. canvas_load brings it back; in the app the user can also undo it"
    )]
    async fn canvas_clear(&self, Parameters(a): Parameters<CanvasTargetArgs>) -> CallToolResult {
        match self
            .canvas(a.project, a.file, json!({ "op": "clear" }))
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Replace the canvas with a .sawit file, or import an .excalidraw file or scene JSON in its place. The board it replaces is kept beside the canvas file as <file>.bak (canvas.sawit.bak for the project's canvas, home.sawit.bak for home.sawit), as canvas_clear keeps one"
    )]
    async fn canvas_load(&self, Parameters(a): Parameters<CanvasLoadArgs>) -> CallToolResult {
        let op = match (a.scene, a.path) {
            (Some(scene), _) => json!({ "op": "replace", "scene": scene }),
            // A canvas file is loaded by the canvas itself, from where it is, which refuses a
            // file it can't read whole, a pipe or a device included.
            (None, Some(path)) if is_sawit_file(&path) => {
                json!({ "op": "replace", "path": absolute_path(&path) })
            }
            // Read here, as an agent means the path (MCP-7): trimmed, `~` for the home folder,
            // and from the agent's folder.
            (None, Some(path)) => {
                let file = crate::agent_path(&path);
                let text = match blocking({
                    let file = file.clone();
                    move || read_scene(&file)
                })
                .await
                {
                    Ok(Ok(text)) => text,
                    Ok(Err(e)) => return fail(format!("Unable to read {}: {e}", file.display())),
                    Err(e) => return fail(e),
                };
                match serde_json::from_str::<Value>(&text) {
                    Ok(scene) => json!({ "op": "replace", "scene": scene }),
                    Err(e) => return fail(format!("{} isn't valid JSON: {e}", file.display())),
                }
            }
            (None, None) => return fail("Give a path or a scene."),
        };
        match self.canvas(a.project, a.file, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Export the canvas's page on show as one SVG or PNG file, at 1×. For other pages, chosen layers, another format (pdf, jpg, webp) or scale, use canvas_command file.export {ids, format, scale, path}"
    )]
    async fn canvas_export(&self, Parameters(a): Parameters<CanvasExportArgs>) -> CallToolResult {
        let format = a.format.as_deref().map(str::trim).unwrap_or_default();
        let png = match format.to_lowercase().as_str() {
            "svg" | "" => false,
            "png" => true,
            _ => return fail(format!("Unknown format “{format}”: use svg or png.")),
        };
        // Read as every tool that writes a file reads its path (MCP-11): absolute, and never a
        // canvas file, since nothing else holds the board when the app is closed.
        let named = tri!(crate::output_path(
            "canvas_export",
            a.output_path.as_deref()
        ));
        let op = json!({ "op": if png { "export_png" } else { "export_svg" } });
        let project = crate::agent_project(a.project.as_deref());
        // Named after the canvas file: `canvas-…` for the project's, `home-…` for home.sawit.
        let canvas = tri!(canvas_file(a.file.as_deref()));
        let stem = canvas.as_deref().map(store::bound_file);
        let stem = stem.as_deref().and_then(Path::file_stem);
        let stem = stem.map_or_else(|| "canvas".into(), |stem| stem.to_string_lossy());
        let value = match self.canvas(a.project, a.file, op).await {
            Ok(v) => v,
            Err(e) => return fail(e),
        };
        let bytes = if png {
            match base64_decode(
                value["png_base64"].as_str().unwrap_or_default(),
                "The canvas",
            ) {
                Ok(b) => b,
                Err(e) => return fail(e),
            }
        } else {
            value["svg"]
                .as_str()
                .unwrap_or_default()
                .as_bytes()
                .to_vec()
        };
        let path = named.unwrap_or_else(|| {
            export_path(
                project.as_deref(),
                &self.paths.cache_dir(),
                &stem,
                if png { "png" } else { "svg" },
            )
        });
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            return fail(format!("Unable to create {}: {e}", dir.display()));
        }
        match std::fs::write(&path, &bytes) {
            Ok(()) => ok(format!(
                "Canvas exported to {} ({} bytes)",
                path.display(),
                bytes.len()
            )),
            Err(e) => fail(format!("Unable to write {}: {e}", path.display())),
        }
    }

    #[tool(
        description = "Run one command of the canvas engine by its id, with its parameters as JSON: board.sticky {x, y, text}, node.set, align.left, edit.undo, view.zoomFit, ... The reply is the command's result, and a command the engine refuses is an error. canvas_commands lists the ids, and with a filter what each takes. With the app closed each call starts from the saved file, so nothing carries over between calls (no undo, no selection, no view): pass ids, or run the steps in one command.batch. A command that deletes layers (edit.delete, edit.cut, page.delete, ...) keeps the board they were on beside the canvas file as <file>.del.bak first, as canvas_delete does"
    )]
    async fn canvas_command(&self, Parameters(a): Parameters<CanvasCommandArgs>) -> CallToolResult {
        self.canvas_command_tool(a).await
    }

    #[tool(
        description = "List the commands canvas_command runs: the id and label of each. The parameters each takes make the full list too long to return at once, so pass `filter` (board., align, sticky, ...) to get the matching commands with theirs"
    )]
    async fn canvas_commands(
        &self,
        Parameters(a): Parameters<CanvasCommandsArgs>,
    ) -> CallToolResult {
        match self
            .canvas(a.project, a.file, json!({ "op": "commands" }))
            .await
        {
            Ok(reply) => ok(command_list(&reply, a.filter.as_deref())),
            Err(e) => fail(e),
        }
    }
}
