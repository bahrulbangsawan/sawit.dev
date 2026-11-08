//! UI screens in the canvas's tab ([`sawit_board::screens`]): what agents ask of them while the
//! app runs ([`ops`]), the bar over a selected screen with Preview, the downloads and Send to
//! Agent ([`bar`]), the full-size Preview ([`preview`]), the handoff to a coding agent
//! ([`handoff`]), and the screens' pictures in exports ([`pictures`]). How a screen is rendered,
//! watched and made live is the web frames' ([`super::web`]).

#[cfg(test)]
mod agent_tests;
mod bar;
mod checks;
mod convert;
mod frame_bar;
mod handoff;
mod ops;
mod pictures;
mod preview;
mod preview_bar;
mod preview_ops;
mod prototype;
#[cfg(test)]
mod tests;

use std::path::PathBuf;
use std::sync::Arc;

use figcraft_doc::NodeId;
use gpui_kit::*;
use sawit_board::screens;
use sawit_core::terminal::AgentKind;

use super::BoardView;
pub(super) use bar::{code_export_button, render_screen_bar};
pub(super) use frame_bar::render_frame_bar;
pub(super) use handoff::SendTo;
pub(super) use preview::Preview;
pub(super) use preview_bar::render_preview;

/// The screens' side of the canvas's tab.
#[derive(Default)]
pub(in crate::board) struct ScreenUi {
    /// The screen shown full size, if one is.
    pub preview: Option<Preview>,
    /// The coding agents on this Mac that Send to Agent can start, once they are looked for.
    pub agents: Option<Vec<AgentKind>>,
    pub finding_agents: bool,
    /// The design standard's failures each frame's last check found ([`checks`]), and the
    /// frame whose list is open.
    pub checks: std::collections::HashMap<NodeId, Vec<sawit_board::checks::Finding>>,
    pub checks_open: Option<NodeId>,
    /// A Preview to open at the next draw (the `web` op's `preview`, for the e2e specs).
    pub open_preview: Option<NodeId>,
    /// The screen Preview walks to at the next draw, a click in its page having led there
    /// ([`prototype`]), with the way back.
    pub walk: Option<(NodeId, Vec<NodeId>)>,
    /// Custom size typed at the next draw (the `web` op's `preview_custom`): width, height, and
    /// whether Apply is pressed.
    pub custom_typed: Option<(String, String, bool)>,
}

/// The one screen selected on the canvas: its frame.
#[derive(Clone, Debug)]
pub(in crate::board) struct Selected {
    pub frame: NodeId,
}

impl BoardView {
    /// The canvas's screens folder ([`screens::screens_dir`]).
    pub(in crate::board) fn screens_dir(&self) -> PathBuf {
        screens::screens_dir(&sawit_board::store::bound_file(self.path()))
    }

    /// The project the canvas is a project's canvas of (its `.nyawit` folder's parent).
    pub(in crate::board) fn canvas_project(&self) -> Option<PathBuf> {
        let dir = self.path().parent()?;
        let in_nyawit = dir
            .file_name()
            .is_some_and(|n| n == sawit_core::nyawit::DIR);
        in_nyawit.then(|| dir.parent().map(PathBuf::from)).flatten()
    }

    /// The screen selected alone on the canvas, when one is and it isn't live.
    pub(in crate::board) fn selected_screen(&self) -> Option<Selected> {
        let ds = self.board()?.session.ds().ok()?;
        let [frame] = ds.selection.as_slice() else {
            return None;
        };
        if self.web_live() == Some(*frame) {
            return None;
        }
        screens::screen_of(ds.doc.node(*frame)?)?;
        Some(Selected { frame: *frame })
    }

    /// The latest picture of screen frame `frame`, if it has been rendered.
    pub(in crate::board) fn screen_picture(&self, frame: NodeId) -> Option<Arc<RenderImage>> {
        let web = self.web.as_ref()?;
        let texture = web.frames.get(&frame)?.texture.as_ref()?;
        Some(texture.image.clone())
    }
}

/// The group a device of width `width` belongs to in the size menus (Preview's, the Design
/// panel's): Mobile, Tablet, Laptop or Desktop.
pub(in crate::board) fn device_group(width: u32) -> &'static str {
    match width {
        0..600 => "Mobile",
        600..1100 => "Tablet",
        1100..1500 => "Laptop",
        _ => "Desktop",
    }
}

/// The picture of device `name`, `width` wide: a phone, a tablet, a laptop or a screen.
pub(in crate::board) fn device_icon(name: &str, width: u32) -> gpui_kit::assets::IconName {
    use gpui_kit::assets::IconName as Lucide;
    match device_group(width) {
        _ if name.starts_with("iPad") => Lucide::Tablet,
        "Mobile" => Lucide::Smartphone,
        "Tablet" => Lucide::Tablet,
        "Laptop" => Lucide::Laptop,
        _ => Lucide::Monitor,
    }
}
