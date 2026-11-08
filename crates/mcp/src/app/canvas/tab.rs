//! A canvas tool's `tab`: one of the project's canvases by name, which the tool works on as if
//! its `file` named that canvas's `.sawit` file ([`sawit_core::canvases`]).

use super::args::*;

/// Turns a canvas tool's `tab` into its `file`.
pub(super) trait CanvasTab {
    /// Sets `file` to the file of the canvas `tab` names, in the agent's project; refused when
    /// both are given, or the name can't be a canvas's.
    fn tab_into_file(&mut self) -> Result<(), String>;
}

/// The file of the canvas `tab` of `project` (the agent's when `None`).
fn tab_file(project: Option<&str>, tab: &str) -> Result<String, String> {
    let root = crate::agent_project(project)
        .ok_or("Canvas tabs are a project's: name the project, or work in its folder.")?;
    let file = sawit_core::canvases::canvas_file(&root, tab)?;
    Ok(file.to_string_lossy().into_owned())
}

macro_rules! canvas_tab {
    ($($args:ty),* $(,)?) => {
        $(impl CanvasTab for $args {
            fn tab_into_file(&mut self) -> Result<(), String> {
                let Some(tab) = self.tab.take().filter(|t| !t.trim().is_empty()) else {
                    return Ok(());
                };
                if self.file.as_deref().is_some_and(|f| !f.trim().is_empty()) {
                    return Err("Give the canvas's tab or its file, not both.".into());
                }
                self.file = Some(tab_file(self.project.as_deref(), &tab)?);
                Ok(())
            }
        })*
    };
}

canvas_tab!(
    CanvasTargetArgs,
    CanvasElementsArgs,
    CanvasMermaidArgs,
    CanvasUpdateArgs,
    CanvasDeleteArgs,
    CanvasLoadArgs,
    CanvasCommandArgs,
    CanvasCommandsArgs,
    CanvasExportArgs,
    CanvasBeginArgs,
    CanvasProgressArgs,
    CanvasScreenWriteArgs,
    CanvasScreenGetArgs,
    CanvasScreenExportArgs,
    CanvasAddRoutesArgs,
    CanvasLintArgs,
    CanvasStarterArgs,
    CanvasPreviewArgs,
);
