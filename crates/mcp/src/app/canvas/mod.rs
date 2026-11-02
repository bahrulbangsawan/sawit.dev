//! The canvas tools. Each runs an op of `sawit_board::ops` on a canvas: the one the running app
//! shows for the project, or for a canvas file an agent names, and with the app closed the stored
//! canvas file itself ([`dispatch`]). canvas_design_guide reads none: it answers with Sawit's
//! design standard ([`guide`]), which the tools that make UI point agents at.

mod args;
mod dispatch;
mod files;
mod guide;
mod routes;
mod screens;
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
    CanvasAddRoutesArgs, CanvasBeginArgs, CanvasCommandArgs, CanvasCommandsArgs, CanvasDeleteArgs,
    CanvasDesignGuideArgs, CanvasElementsArgs, CanvasExportArgs, CanvasLoadArgs, CanvasMermaidArgs,
    CanvasProgressArgs, CanvasScreenExportArgs, CanvasScreenGetArgs, CanvasScreenWriteArgs,
    CanvasTargetArgs, CanvasUpdateArgs,
};
use dispatch::{command_list, in_work};
use files::{absolute_path, canvas_file, export_path, is_sawit_file, read_scene};

use super::{base64_decode, pretty};
use crate::{Sawit, blocking, fail, ok, tri};

/// The sentence that ends the description of each canvas tool that makes UI: Sawit's design
/// standard comes first.
macro_rules! design_guide_first {
    () => {
        "Designing UI? Read canvas_design_guide first and follow it"
    };
}

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
             Answers the ids of what it drew. For an SVG file of a diagram use render_diagram. ",
            design_guide_first!()
        )
    };
}

#[tool_router(router = canvas_router, vis = "pub(super)")]
impl Sawit {
    #[tool(
        description = "Sawit's design standard for UI on the canvas, as Markdown: the SKILL.md of the Canvas Design skill Sawit ships. Read it before you design, build or review any UI on the canvas, follow it, and run its checklist before you say a design is done. `section` (a heading, such as Typography) gives that section with the checklist. Works without the app"
    )]
    async fn canvas_design_guide(
        &self,
        Parameters(a): Parameters<CanvasDesignGuideArgs>,
    ) -> CallToolResult {
        let text = tri!(guide::text());
        match guide::answer(text, a.section.as_deref()) {
            Ok(answer) => ok(answer),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "List the top-level layers of the canvas's page on show: id, type, position, size, text, color and what a connector joins. For the other pages and nested layers use canvas_command: page.list lists the pages, and node.get {id} reads a page's or a layer's own layers, without showing them: the page and the view on show are the user's, so page.select and view.* are refused. Works from the saved file when the app isn't running; the page on show is then the one last saved"
    )]
    async fn canvas_get(&self, Parameters(a): Parameters<CanvasTargetArgs>) -> CallToolResult {
        self.canvas_get_tool(a).await
    }

    #[tool(description = concat!(
        "Draw on Sawit's canvas: add shapes, stickies, labels, text and arrows (arrows bind to \
         shapes by id). Shapes and stickies with no backgroundColor come in sticky yellow. To \
         build a component, begin it with canvas_begin first and pass the `work` it answers here: \
         the elements go inside its frame, x and y from its top-left corner. With the app running \
         the canvas's tab opens in the background if it isn't open; the user's tab, page and view \
         stay as they are. ",
        design_guide_first!()
    ))]
    async fn canvas_add_elements(
        &self,
        Parameters(a): Parameters<CanvasElementsArgs>,
    ) -> CallToolResult {
        let op = in_work(json!({ "op": "add", "elements": a.elements }), a.work);
        match self.canvas(a.project, a.file, op).await {
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

    #[tool(description = concat!(
        "Change canvas elements by id: position, size, colors, text. `append` adds text to the \
         end of a layer's, for long copy sent in parts; new text shows at reading speed on the \
         canvas. ",
        design_guide_first!()
    ))]
    async fn canvas_update(&self, Parameters(a): Parameters<CanvasUpdateArgs>) -> CallToolResult {
        let op = in_work(json!({ "op": "update", "elements": a.elements }), a.work);
        match self.canvas(a.project, a.file, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Delete canvas elements by id, on any page (a connector's label goes with it). The board they were on is first kept beside the canvas file as <file>.del.bak (canvas.sawit.del.bak for the project's canvas, home.sawit.del.bak for home.sawit), the 4 kept before it moving to .del.bak.1 to .del.bak.4 (the oldest goes). Deletions keep these apart from the .bak files canvas_clear and canvas_load keep, so they never push those out; a deletion that leaves the canvas empty keeps a .bak, as a clear does. canvas_load brings any of them back; in the app the user can also undo it"
    )]
    async fn canvas_delete(&self, Parameters(a): Parameters<CanvasDeleteArgs>) -> CallToolResult {
        let op = in_work(json!({ "op": "delete", "ids": a.ids }), a.work);
        match self.canvas(a.project, a.file, op).await {
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

    #[tool(description = concat!(
        "Run one command of the canvas engine by its id, with its parameters as JSON: \
         board.sticky {x, y, text}, node.set, align.left, edit.undo, ... Commands that move the \
         view or the page the user has on show (view.*, page.select) are refused: address pages \
         and layers by id. The reply is the command's result, and a command the engine refuses \
         is an error. canvas_commands lists the ids, and with a filter what each takes. With the \
         app closed each call starts from the saved file, so nothing carries over between calls \
         (no undo, no selection, no view): pass ids, or run the steps in one command.batch. A \
         command that deletes layers (edit.delete, edit.cut, page.delete, ...) keeps the board \
         they were on beside the canvas file as <file>.del.bak first, as canvas_delete does. ",
        design_guide_first!()
    ))]
    async fn canvas_command(&self, Parameters(a): Parameters<CanvasCommandArgs>) -> CallToolResult {
        self.canvas_command_tool(a).await
    }

    #[tool(
        description = "Begin a component before you build it, and begin each component that way: its named frame appears on the canvas at once, and while you build the canvas shows a skeleton in it, a row above it with its name and step, your name and a progress badge. Pass the `work` this answers to canvas_add_elements, canvas_update, canvas_add_diagram, canvas_delete and canvas_command: their ops go inside the frame (x and y from its top-left corner) and its undo step. Say each step, and that it is done or failed, with canvas_progress. Answers {work, frame, page}. kind \"screen\" makes a UI screen instead: the frame and its HTML folder (.nyawit/screens/<screen>/) at once; build it with canvas_screen_write. With the app closed the frame is made in the saved file. Designing UI? Read canvas_design_guide first and follow it"
    )]
    async fn canvas_begin(&self, Parameters(a): Parameters<CanvasBeginArgs>) -> CallToolResult {
        let screen = (a.kind.as_deref()).is_some_and(|k| k.trim().eq_ignore_ascii_case("screen"));
        let op = json!({
            "op": "begin", "name": a.name, "frame": a.frame, "steps": a.steps, "kind": a.kind,
        });
        let begun = match screen {
            true => self.begin_screen(a.project, a.file, op).await,
            false => self.canvas(a.project, a.file, op).await,
        };
        match begun {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Write a UI screen's HTML (begun with canvas_begin {kind: \"screen\"}): the whole page (`html`), or `edits` to it, and its other files (`files`: CSS, scripts, SVG, images as base64). Each write re-renders the screen on the canvas, so the user watches it build: lay it out, then fill in copy, then style it, writing as you go, and say each step with canvas_progress. The page has no network: put fonts, images and scripts in its folder, or use system fonts. Answers the files written. With the app closed the files are written and nothing renders. Designing UI? Read canvas_design_guide first and follow it"
    )]
    async fn canvas_screen_write(
        &self,
        Parameters(a): Parameters<CanvasScreenWriteArgs>,
    ) -> CallToolResult {
        self.canvas_screen_write_tool(a).await
    }

    #[tool(
        description = "Read a UI screen: its HTML (index.html), its files, its frame and viewport, and a PNG of it as the canvas renders it (with the app running). Without `work` or `screen`, list the screens on the canvas. With the app closed nothing is rendered, so there is no PNG"
    )]
    async fn canvas_screen_get(
        &self,
        Parameters(a): Parameters<CanvasScreenGetArgs>,
    ) -> CallToolResult {
        self.canvas_screen_get_tool(a).await
    }

    #[tool(
        description = "Export a UI screen to a file: format html, one .html file with its CSS and images inlined; or react, a mechanical conversion to a .tsx component with its .css (and the images it uses) beside it, for an app to adapt. Works with the app closed too"
    )]
    async fn canvas_screen_export(
        &self,
        Parameters(a): Parameters<CanvasScreenExportArgs>,
    ) -> CallToolResult {
        self.canvas_screen_export_tool(a).await
    }

    #[tool(
        description = "Lay a dev server's routes out on the canvas as live frames: a row per route, a frame per viewport (1440, 768 and 390 wide by default), named \"<route> / <viewport>\", in a section named for the server. Each frame shows the page itself, rendered by WebKit as the Browser tab renders it: it refreshes when the page changes (a hot update or a reload), keeps its last picture and says \"Server stopped\" while the server is down, and the user can use it live. The server is the one running in the project (as list_ports finds it), or `url` on this Mac (localhost, 127.0.0.1, [::1]; other hosts are refused for now). The routes are found in the app's files (Next.js app and pages, TanStack Router or Start, SvelteKit, Remix, React Router, Nuxt, Astro) or by following the site's links; a dynamic route (/blog/[slug]) is added with a page of it the site links to, else skipped and named in the answer. While it runs the canvas shows it as your work, with its steps. Answers the section, each frame's id, route and URL, and the routes skipped. With the app closed the frames are saved and nothing renders"
    )]
    async fn canvas_add_routes(
        &self,
        Parameters(a): Parameters<CanvasAddRoutesArgs>,
    ) -> CallToolResult {
        self.canvas_add_routes_tool(a).await
    }

    #[tool(
        description = "Say where a component begun with canvas_begin stands: `step`, the step now running, and `status` done or failed (with a `message`, the reason, which stays on the canvas until you retry or the user dismisses it). A work with no op or progress for 60 s shows as stalled. With the app closed nothing shows progress, so it changes nothing and says so"
    )]
    async fn canvas_progress(
        &self,
        Parameters(a): Parameters<CanvasProgressArgs>,
    ) -> CallToolResult {
        let op = json!({
            "op": "progress", "work": a.work, "step": a.step, "status": a.status,
            "message": a.message,
        });
        match self.canvas(a.project, a.file, op).await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(description = concat!(
        "List the commands canvas_command runs: the id and label of each. The parameters each \
         takes make the full list too long to return at once, so pass `filter` (board., align, \
         sticky, ...) to get the matching commands with theirs. ",
        design_guide_first!()
    ))]
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
