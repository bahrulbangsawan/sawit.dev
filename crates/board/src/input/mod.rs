//! Keys and the pointer on the canvas.
//!
//! GPUI and FigCraft name keys differently, so [`key_name`] and [`gpui_keystroke`] translate
//! between them, and [`route_key`] sends a key where FigCraft's own keymap says, under Sawit's
//! command policy ([`crate::policy`]).
//!
//! The pointer goes to FigCraft's tools as page points ([`pointer_event`], [`pointer`]). What the
//! tools and commands ask of the view comes back as requests: [`handle_requests`] carries out the
//! ones that need no view, and hands back the rest.

pub use figcraft_engine::keymap::Mods;

mod keys;
mod pointer;
mod route;
mod text;

pub use keys::{gpui_keystroke, key_name};
pub(crate) use pointer::page_pointer_event;
pub use pointer::{
    ResizeDir, handle_requests, pointer, pointer_event, pointer_left, resize_dir, triple_click,
};
pub use route::{command_id, route_command, route_default_key, route_key, route_unbound_key};
pub use text::{TEXT_CLIPBOARD, end_text_edit, end_text_edit_before};

/// What became of a key sent to the canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyOutcome {
    /// The canvas used it: a command ran, or a tool or the text editor took it.
    Handled,
    /// It is the shortcut of a command Sawit carries out itself ([`crate::policy::UI_COMMANDS`]),
    /// or of a copy or a paste, which Sawit carries out around the engine
    /// ([`crate::policy::CLIPBOARD`]). Nothing ran: carrying it out is the caller's job.
    Ui(&'static str),
    /// The canvas has no use for it: it is no shortcut, or a hidden command's, or it types into
    /// the text being edited, which the input method does.
    Unhandled,
}

#[cfg(test)]
mod tests;
