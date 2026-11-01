//! The keys Sawit gives the canvas's commands that FigCraft's keymap leaves without one, or whose
//! FigCraft key Sawit keeps.

use figcraft_engine::keymap::Group;

use super::title;

/// A key Sawit gives one of the canvas's commands that FigCraft's keymap leaves without one, or
/// whose FigCraft key Sawit keeps. Its keys are live on the canvas, not while text is edited.
#[derive(Clone, Copy, Debug)]
pub struct SawitKey {
    /// The group of the shortcuts sheet it is listed in.
    pub group: Group,
    pub id: &'static str,
    /// The Mac keys, written as FigCraft's keymap writes them (`Alt+Shift+Cmd+G`).
    pub keys: &'static [&'static str],
    /// What the sheet and the keymap editor call the command, where its menu label says too
    /// little out of its menu ("Fill" in Edit ▸ Select all with same); empty for its menu label.
    label: &'static str,
}

impl SawitKey {
    /// What the sheet and the keymap editor call the command: its own label, else its menu's.
    pub fn title(&self) -> &'static str {
        if self.label.is_empty() {
            title(self.id)
        } else {
            self.label
        }
    }

    const fn label(mut self, label: &'static str) -> Self {
        self.label = label;
        self
    }
}

const fn sawit(group: Group, id: &'static str, keys: &'static [&'static str]) -> SawitKey {
    SawitKey {
        group,
        id,
        keys,
        label: "",
    }
}

/// The keys Sawit gives the canvas's commands besides FigCraft's ([`SawitKey`]). Figma and
/// FigJam have no key for these either, so each family of commands shares its modifiers and
/// FigCraft's neighbouring keys:
///
/// - Polygon and Star: ⇧6 and ⇧8, the keys of `^` (a triangle, the polygon the tool draws) and
///   `*` (a star). Text on a path: ⇧T, beside Text's T.
/// - Copy as and Export: Copy as PNG is FigCraft's ⇧⌘C and Export its ⇧⌘E; ⌥ makes them SVG, ⌃
///   CSS for a copy and frames to PDF for an export, and ⌃⌥⌘C copies the text.
/// - Select all with same: ⌃⇧ and the first letter of what is the same (Fill, Stroke, Effect,
///   Text, fOnt, Instance, Properties).
/// - Where Sawit keeps FigCraft's Mac key (⌃⌘M, ⌥⌘O, ⌘J, ⇧⌘J), the command takes FigCraft's
///   Windows and Linux key, with ⌃ for Ctrl: Use as mask ⌃⌥M, Outline stroke ⌃⌥O, Join ⌃J,
///   Smooth join ⌃⇧J. Release mask is ⌃⌥⇧M; Simplify and Reverse path are ⌃⌥S and ⌃⌥R, Crop
///   image ⌃⌥C.
/// - Beside FigCraft's own: Remove frame ⌥⇧⌘G (Frame selection is ⌥⌘G), Hide other layers ⌥⇧⌘H
///   (Show/Hide is ⇧⌘H), Create multiple components ⌥⇧⌘K (Create component is ⌥⌘K), Reset
///   instance ⌥⇧⌘B (Detach is ⌥⌘B), Push changes to and Restore the main component ⌃⌥⌘P and
///   ⌃⌥⌘R (Go to main is ⌃⌥⌘K), Combine as variants ⌃⌥K, Show outlines for the selection ⌥⇧⌘O
///   (Show outlines is ⇧⌘O), Soft-proof sRGB ⌃⌥P (Pixel preview is ⌃P), Pack ⌃⌥⇧H and ⌃⌥⇧V
///   (Distribute is ⌃⌥H and ⌃⌥V).
/// - Rotate 90° left and right ⌥⇧L and ⌥⇧R, 180° ⌥⇧T; Round to pixels ⌥⇧P, Rasterize ⌥⇧⌘P.
/// - Text case: ⌥⇧⌘ and U (upper), L (lower), T (title), N (as typed).
/// - Zoom to 50% and 200%: ⌥⇧- and ⌥⇧=, beside zoom out and in.
/// - Snap to geometry and to objects: ⌥⇧⌘' and ⌃⇧⌘', beside FigCraft's ⇧⌘' (snap to pixel grid).
///
/// None is a key the app keeps on the canvas ([`SAWIT_WINS`](crate::policy::SAWIT_WINS)) or one of
/// FigCraft's, and the app's tests prove that none is a key of the app's.
pub static SAWIT_KEYS: &[SawitKey] = &[
    // Tools
    sawit(Group::Tools, "tool.polygon", &["Shift+6"]),
    sawit(Group::Tools, "tool.star", &["Shift+8"]),
    sawit(Group::Tools, "tool.textOnPath", &["Shift+T"]),
    // View
    sawit(Group::View, "view.outlinesSelection", &["Alt+Shift+Cmd+O"]),
    sawit(Group::View, "view.proofSrgb", &["Ctrl+Alt+P"]),
    sawit(Group::View, "view.snapGeometry", &["Alt+Shift+Cmd+'"]),
    sawit(Group::View, "view.snapObjects", &["Ctrl+Shift+Cmd+'"]),
    // Zoom
    sawit(Group::Zoom, "view.zoom50", &["Alt+Shift+-"]),
    sawit(Group::Zoom, "view.zoom200", &["Alt+Shift+="]),
    // Text
    sawit(Group::Text, "text.caseUpper", &["Alt+Shift+Cmd+U"]),
    sawit(Group::Text, "text.caseLower", &["Alt+Shift+Cmd+L"]),
    sawit(Group::Text, "text.caseTitle", &["Alt+Shift+Cmd+T"]),
    sawit(Group::Text, "text.caseOriginal", &["Alt+Shift+Cmd+N"]),
    // Shape
    sawit(Group::Shape, "vector.outlineStroke", &["Ctrl+Alt+O"]),
    sawit(Group::Shape, "vector.join", &["Ctrl+J"]),
    sawit(Group::Shape, "vector.smoothJoin", &["Ctrl+Shift+J"]),
    sawit(Group::Shape, "vector.simplify", &["Ctrl+Alt+S"]),
    sawit(Group::Shape, "vector.reverse", &["Ctrl+Alt+R"]),
    sawit(Group::Shape, "object.rasterize", &["Alt+Shift+Cmd+P"]),
    // Selection
    sawit(Group::Selection, "select.sameFill", &["Ctrl+Shift+F"])
        .label("Select all with same fill"),
    sawit(Group::Selection, "select.sameStroke", &["Ctrl+Shift+S"])
        .label("Select all with same stroke"),
    sawit(Group::Selection, "select.sameEffect", &["Ctrl+Shift+E"])
        .label("Select all with same effect"),
    sawit(Group::Selection, "select.sameText", &["Ctrl+Shift+T"])
        .label("Select all with same text properties"),
    sawit(Group::Selection, "select.sameFont", &["Ctrl+Shift+O"])
        .label("Select all with same font"),
    sawit(Group::Selection, "select.sameInstance", &["Ctrl+Shift+I"])
        .label("Select all with same instance"),
    sawit(Group::Selection, "select.sameProperties", &["Ctrl+Shift+P"])
        .label("Select all with same properties"),
    sawit(Group::Selection, "object.removeFrame", &["Alt+Shift+Cmd+G"]),
    sawit(Group::Selection, "object.hideOthers", &["Alt+Shift+Cmd+H"]),
    // Edit
    sawit(Group::Edit, "edit.copyAsText", &["Ctrl+Alt+Cmd+C"]),
    sawit(Group::Edit, "edit.copyAsCss", &["Ctrl+Shift+Cmd+C"]),
    sawit(Group::Edit, "edit.copyAsSvg", &["Alt+Shift+Cmd+C"]),
    sawit(Group::Edit, "file.exportSvg", &["Alt+Shift+Cmd+E"]),
    sawit(Group::Edit, "file.exportFramesPdf", &["Ctrl+Shift+Cmd+E"]),
    // Transform
    sawit(Group::Transform, "object.mask", &["Ctrl+Alt+M"]),
    sawit(
        Group::Transform,
        "object.releaseMask",
        &["Ctrl+Alt+Shift+M"],
    ),
    sawit(Group::Transform, "image.crop", &["Ctrl+Alt+C"]),
    sawit(Group::Transform, "object.rotateLeft", &["Alt+Shift+L"]),
    sawit(Group::Transform, "object.rotateRight", &["Alt+Shift+R"]),
    sawit(Group::Transform, "object.rotate180", &["Alt+Shift+T"]),
    // Arrange
    sawit(Group::Arrange, "arrange.packH", &["Ctrl+Alt+Shift+H"]),
    sawit(Group::Arrange, "arrange.packV", &["Ctrl+Alt+Shift+V"]),
    sawit(Group::Arrange, "arrange.roundToPixels", &["Alt+Shift+P"]),
    // Components
    sawit(
        Group::Components,
        "component.createMultiple",
        &["Alt+Shift+Cmd+K"],
    ),
    sawit(
        Group::Components,
        "component.combineVariants",
        &["Ctrl+Alt+K"],
    ),
    sawit(
        Group::Components,
        "component.resetInstance",
        &["Alt+Shift+Cmd+B"],
    ),
    sawit(
        Group::Components,
        "component.pushChanges",
        &["Ctrl+Alt+Cmd+P"],
    ),
    sawit(
        Group::Components,
        "component.restoreMain",
        &["Ctrl+Alt+Cmd+R"],
    ),
];
