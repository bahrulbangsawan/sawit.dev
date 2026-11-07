//! The design lint's tools (B12): `canvas_lint` and `canvas_starter_system`.

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{tool, tool_router};
use serde_json::json;

use super::tab::CanvasTab as _;
use super::{CanvasLintArgs, CanvasStarterArgs};
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
}
