//! When a menu item is enabled: the engine or Sawit can run its command ([`available`]), and the
//! command applies to what the canvas holds now ([`applies`]).

use figcraft_engine::Session;
use figcraft_engine::cmd::find_command;

use crate::policy::{is_ui, keeps_text_edit};

/// Whether menu item `id` can run: the engine has the command, or Sawit carries it out itself.
pub(super) fn available(id: &str) -> bool {
    find_command(id).is_some() || is_ui(id)
}

/// The commands that act on the selected layers, which have nothing to act on while no layer is
/// selected: the engine refuses them then ("nothing is selected"), and Sawit's Copy as text and
/// CSS copy nothing. The Select all with same commands are among them too: with nothing selected
/// there is nothing to match, and they select nothing.
pub const NEEDS_SELECTION: &[&str] = &[
    // Edit
    "edit.copy",
    "edit.cut",
    "edit.pasteOver",
    "edit.pasteReplace",
    "edit.duplicate",
    "edit.delete",
    "edit.copyProperties",
    "edit.copyAsText",
    "edit.copyAsCss",
    "select.sameFill",
    "select.sameStroke",
    "select.sameEffect",
    "select.sameText",
    "select.sameFont",
    "select.sameInstance",
    "select.sameProperties",
    // Object
    "object.frameSelection",
    "object.group",
    "object.ungroup",
    "object.removeFrame",
    "object.resizeToFit",
    "layout.add",
    "layout.remove",
    "layout.suggest",
    "component.create",
    "component.createMultiple",
    "component.combineVariants",
    "component.resetInstance",
    "component.detach",
    "component.goToMain",
    "component.pushChanges",
    "component.restoreMain",
    "object.mask",
    "object.releaseMask",
    "image.crop",
    "arrange.front",
    "arrange.forward",
    "arrange.backward",
    "arrange.back",
    "object.flipH",
    "object.flipV",
    "object.rotate180",
    "object.rotateLeft",
    "object.rotateRight",
    "vector.flatten",
    "vector.outlineStroke",
    "bool.union",
    "bool.subtract",
    "bool.intersect",
    "bool.exclude",
    "bool.flatten",
    "object.rasterize",
    "object.toggleVisible",
    "object.toggleLock",
    "object.removeFill",
    "object.removeStroke",
    "object.swapFillStroke",
    "object.rename",
    // Text
    "text.bold",
    "text.italic",
    "text.underline",
    "text.strikethrough",
    "text.bulletList",
    "text.numberList",
    "text.alignLeft",
    "text.alignCenter",
    "text.alignRight",
    "text.alignJustify",
    "text.sizeUp",
    "text.sizeDown",
    "text.weightUp",
    "text.weightDown",
    "text.lineHeightUp",
    "text.lineHeightDown",
    "text.trackingUp",
    "text.trackingDown",
    "text.caseOriginal",
    "text.caseUpper",
    "text.caseLower",
    "text.caseTitle",
    // Arrange
    "align.left",
    "align.hcenter",
    "align.right",
    "align.top",
    "align.vcenter",
    "align.bottom",
    "distribute.horizontal",
    "distribute.vertical",
    "arrange.tidy",
    "arrange.packH",
    "arrange.packV",
    "arrange.roundToPixels",
    // Vector
    "vector.join",
    "vector.smoothJoin",
    "vector.simplify",
    "vector.reverse",
];

/// The commands the menus show that need what the canvas has no dialog to ask for yet: a link's
/// URL, a page, the new names, an offset, a colour profile, the nudge amounts (ruling P6-C18's
/// follow-up for input dialogs). Run as a menu item runs them, without it, they only refuse or
/// read the values back. Their items are greyed out and their keys do nothing until the canvas
/// asks for it; an agent passes it in the command's parameters.
pub const NEEDS_INPUT: &[&str] = &[
    "text.link",
    "vector.offset",
    "object.moveToPage",
    "object.batchRename",
    "file.colorProfile",
    "pref.nudge",
];

/// The commands the menus show that never apply on the canvas: Revert to saved, because the canvas
/// writes its file as it is edited and never holds unsaved changes to drop (the engine says "the
/// file was never saved").
pub const NEVER_ON_THE_CANVAS: &[&str] = &["file.revert"];

/// Whether command `id` applies to what the canvas `s` holds now, so that its menu item is enabled
/// and its key acts:
/// - not one that needs input the canvas can't ask for ([`NEEDS_INPUT`]), nor one that never
///   applies on the canvas ([`NEVER_ON_THE_CANVAS`]);
/// - for a command on the selection ([`NEEDS_SELECTION`]), only with layers selected, or for copy
///   and paste and the Text menu's styles while text is edited, which act on its characters;
/// - Undo and Redo only with a step to take back or do again (an edit of text under way is one);
/// - Delete and heal only while the layers are edited as vectors.
///
/// Any other command applies.
pub fn applies(s: &Session, id: &str) -> bool {
    if NEEDS_INPUT.contains(&id) || NEVER_ON_THE_CANVAS.contains(&id) {
        return false;
    }
    let ds = s.ds().ok();
    let editing_text = ds.is_some_and(|ds| ds.text_edit.is_some());
    let (undo, redo) = s.undo_depth();
    match id {
        "edit.undo" => undo > 0 || editing_text,
        "edit.redo" => redo > 0,
        "vector.deleteHeal" => ds.is_some_and(|ds| ds.vector_edit.is_some()),
        _ if NEEDS_SELECTION.contains(&id) => {
            !s.selection().is_empty() || (editing_text && keeps_text_edit(id))
        }
        _ => true,
    }
}
