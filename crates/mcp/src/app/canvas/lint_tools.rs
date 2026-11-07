//! The design lint's tools (B12), `canvas_lint` and `canvas_starter_system`, and Convert to
//! Layers (B15), `canvas_screen_to_layers`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use super::tab::CanvasTab as _;
use super::{CanvasLintArgs, CanvasScreenGetArgs, CanvasStarterArgs};
use crate::app::pretty;
use crate::{Sawit, fail, ok, tri};

#[tool_router(router = canvas_lint_router, vis = "pub(in crate::app)")]
impl Sawit {
    #[tool(
        description = "Lint a design on the canvas against Sawit's design standard (B12): a frame by `id`, a work's frame by `work`, or the page on show. Finds values off the file's tokens (with the nearest token), copies that should be components, frames placed by hand where auto layout belongs, uneven spacing, too many type sizes or colours, placeholder copy (\"Lorem ipsum\", \"Click here\"), generic generated looks (purple-to-blue gradients, emoji as icons, glow and glass everywhere, card inside card, everything centred) and the machine checks (contrast, targets, text floors). Each finding has its rule, the layer's id, the fix, its severity (error or warning), the guide section, and a command that fixes it where one does. Also answers `status`: where the file is in the workflow (no system, system, components, screens, cover) and the next step. `sketch` keeps the warnings quiet. Run it before you say a design is done; fix every error. Works with the app closed too"
    )]
    async fn canvas_lint(&self, Parameters(mut a): Parameters<CanvasLintArgs>) -> CallToolResult {
        tri!(a.tab_into_file());
        self.canvas_lint_tool(a).await
    }

    #[tool(
        description = "Lay out a starter design system to build on, so a design never starts from nothing (B12): Sawit's tokens as variables (colours with Light and Dark modes: bg/surface, bg/subtle, fg/default, fg/muted, border, accent, accent/fg, danger; space/1–8, radius/sm–lg, type/caption–title) and a Components frame with Button / Primary, Button / Secondary, Input and Card, each a component bound to the tokens with auto layout. Use instance.create of them for screens. A file that has the tokens keeps them. Answers the components' ids and the workflow's status. Works with the app closed too"
    )]
    async fn canvas_starter_system(
        &self,
        Parameters(mut a): Parameters<CanvasStarterArgs>,
    ) -> CallToolResult {
        tri!(a.tab_into_file());
        match self
            .canvas(
                a.project,
                a.file,
                json!({"op": "starter", "x": a.x, "y": a.y}),
            )
            .await
        {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    #[tool(
        description = "Convert a UI screen (HTML, kind \"screen\") to editable canvas layers, as Convert to Layers does in the app (B15): the page as the canvas renders it becomes a frame beside the screen, named as the screen was, with frames, text, images and SVG icons as vectors, auto layout where the page's rows and columns sit at even gaps, and its colours and type sizes as variables (a file's existing token of the same value is used). The screen's frame gets \" / HTML\" after its name and is kept with its folder; one undo step takes it all back. Answers the new frame's id, how many layers, texts, auto layouts and pictures it made, the variables it added, and what couldn't be converted (CSS background images, ::before/::after content, pictures from elsewhere). Needs the app running with the screen's page on show"
    )]
    async fn canvas_screen_to_layers(
        &self,
        Parameters(mut a): Parameters<CanvasScreenGetArgs>,
    ) -> CallToolResult {
        tri!(a.tab_into_file());
        if a.work.is_none() && a.screen.is_none() {
            return fail("canvas_screen_to_layers: name the screen with `screen` or `work`");
        }
        let op = json!({"op": "screen_layers", "work": a.work, "screen": a.screen});
        let done = self.screen_call(a.project, a.file, op, |_, _| Err(NEEDS_THE_APP.to_string()));
        match done.await {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }
}

/// What `canvas_screen_to_layers` says with the app closed.
const NEEDS_THE_APP: &str = "canvas_screen_to_layers reads the screen as Sawit renders it: \
     start the app (launch_app), open the canvas with the screen's page on show, then try again";
