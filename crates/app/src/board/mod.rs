//! The native canvas tab: a FigCraft document, rendered off the UI thread and painted by GPUI.
//! Only the GPUI side lives here; the document logic is `sawit_board` (`crates/board`).
//!
//! [`BoardView`] shows one canvas file. Each frame it asks the render thread ([`worker`]) for the
//! view when that changed, and paints the newest frame it has, moved and scaled to the view, so
//! pans and zooms show at once and the render catches up ([`paint`]). Over it go the selection,
//! handles, names and previews `sawit_board::overlay` lists, as the view is now
//! ([`overlay_paint`]). Edits are saved once the canvas has been still for a moment, and when the
//! tab closes or the app quits.
//!
//! The pointer drives FigCraft's tools ([`events`]). FigCraft's shortcuts are key bindings of the
//! app, to [`Command`], which the keymap editor shows and changes; the ones Sawit uses too are
//! shared by the spec's conflict policy ([`bindings`]). Other keys go to the tools and the text
//! editor ([`BoardView::on_key`]). Copy and paste reach the other canvas tabs and the system clipboard
//! ([`BoardView::run_clipboard`]). While a text is edited, what the keys type and what an input
//! method composes go into it ([`text_input`]).
//!
//! Around the canvas goes its chrome (spec D11): the top bar over it ([`top_bar`]), the toolbar
//! floating at its bottom centre ([`toolbar`]), the context bar floating over a board's
//! selection ([`context_bar`]), the layers panel on its left ([`layers_panel`]) and the Design
//! panel on its right ([`design_panel`]). Their buttons run their commands through
//! [`BoardView::choose`]. The menus (spec D8: the top bar's main menu, the context menu, the
//! Actions palette and the shortcuts sheet) run theirs through [`BoardView::run_menu`]
//! ([`menus`]). Both end the text being typed in first, unless the command acts on it
//! ([`BoardView::end_text_edit`]). What the canvas carries out itself instead of the engine, from
//! the menus and the keys alike (import, save, export, copy as), is in [`commands`]. While the UI
//! is hidden (`⌘\`, `view.toggleUi`) only the canvas shows, and the context menu, the palette and
//! the sheet when they are open.
//!
//! Agents that drive the canvas through `sawit-mcp` show on it as other people do on a
//! multiplayer canvas: a small pointer in each one's colour with its name, and each component an
//! agent builds with a skeleton, a row with its step and a progress badge; a roster in the top bar
//! and chips at the edges for those off screen ([`presence`], [`BoardView::agent_op`]). They never
//! move the user's tab, page or view.

mod commands;
mod context_bar;
mod copy_paste;
mod design_panel;
mod events;
mod frames;
mod keymap;
mod keys;
mod layers_panel;
mod menus;
mod open;
pub(crate) mod option_icons;
mod overlay_paint;
mod paint;
mod presence;
mod render;
mod routes;
mod run;
mod save;
mod screens;
mod scroll;
mod shown;
mod text_input;
mod toast;
mod toolbar;
mod top_bar;
mod web;
mod worker;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use figcraft_doc::{Document, NodeId};
use figcraft_engine::ViewState;
use figcraft_engine::textedit::ImeEditOwner;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::*;
use sawit_board::overlay::Composition;
use sawit_board::store::{Backup, Writer};
use sawit_board::toolbar::Flyout;
use sawit_board::{BoardDoc, DocKind, shortcuts};
use serde_json::json;

use keymap::{command_keys, command_keys_all};
pub(crate) use keymap::{command_label, let_through_chords, unbound_rows};
use toolbar::Picker;
use worker::{Frame, Worker};

/// How long the canvas must be still after an edit before the edit is saved.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// How long after the last step of a zoom gesture frames are rendered at full quality again.
const ZOOM_SETTLE: Duration = Duration::from_millis(150);
/// How long a toast stays up.
const TOAST_TIME: Duration = Duration::from_secs(3);

/// The canvas tab's icon, in the tab bar and on the title bar's Canvas button.
pub const ICON: Lucide = Lucide::Shapes;
/// The canvas tab's title, and the top bar's name for the canvas a project or the app has.
pub const TITLE: &str = "Canvas";

/// Asks the workspace to fill the window with the canvas, or to put back the layout it replaced:
/// the top bar's Maximize button. ⇧⌘↩, the workspace's own key, does the same.
pub struct ToggleCanvasMaximize;

/// Starts what every canvas shares, once at startup: its key bindings ([`bindings`]), bound after
/// the workspace's, and Sawit's own fonts ([`sawit_board::fonts::register`]). The system's fonts
/// are read once a canvas first opens ([`read_system_fonts`]).
pub fn init(cx: &mut App) {
    init_keys(cx);
    // Before the scan, so that an Inter installed on the Mac doesn't take the place of Sawit's.
    sawit_board::fonts::register();
}

/// Binds the canvas's keys ([`bindings`]), and keeps the key a [`Command`] binding was pressed
/// for, which the tools may take first ([`BoardView::on_command`]): set before the bindings run,
/// cleared once they have.
fn init_keys(cx: &mut App) {
    let keys = bindings(cx);
    cx.bind_keys(keys);
    cx.intercept_keystrokes(|event, _, cx| {
        cx.set_global(PressedKey(Some(event.keystroke.clone())))
    })
    .detach();
    cx.observe_keystrokes(|_, _, cx| cx.set_global(PressedKey(None)))
        .detach();
}

/// Runs a FigCraft command on the canvas, by its id (`tool.move`, `edit.copy`, `view.zoomIn`):
/// what the canvas's shortcuts are bound to.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, schemars::JsonSchema, Action)]
#[action(namespace = board)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// The command's id, as FigCraft names it.
    pub id: String,
}

/// The keystroke being dispatched, while its key bindings run.
struct PressedKey(Option<Keystroke>);
impl Global for PressedKey {}

/// The canvas's key bindings (spec, "Keyboard conflict policy"), bound after the workspace's:
///
/// - FigCraft's shortcuts, each to its [`Command`] in the canvas's key context
///   ([`sawit_board::shortcuts::key_bindings`]): the user may change them in the keymap editor.
/// - The keys the canvas takes from the app while it has the keyboard, bound to `NoAction` first,
///   so that a shortcut the user takes out of the keymap still goes on to [`BoardView::on_key`],
///   not to the app: ⌘0 and ⌘K (FigCraft's zoom to 100% and Actions), the keys of gpui-kit's root
///   that FigCraft uses, and while the canvas edits text ⌘B, ⌥⌘T and ⇧⌘↩ (bold, centre and the
///   next sticky), which are Sawit's otherwise.
///
/// Bound after the workspace's keys, or its ⌘K chords would still keep ⌘K waiting.
pub fn bindings(cx: &App) -> Vec<KeyBinding> {
    let mut keys = let_through();
    for key in shortcuts::key_bindings() {
        let context = KeyBindingContextPredicate::parse(key.context)
            .ok()
            .map(std::rc::Rc::new);
        let args = json!({ "id": key.id }).to_string();
        let action = Box::new(Command {
            id: key.id.to_string(),
        });
        match KeyBinding::load(
            &key.keystroke,
            action,
            context,
            false,
            Some(args.into()),
            cx,
        ) {
            Ok(binding) => keys.push(binding),
            Err(error) => log::warn!("the canvas can't bind {}: {error}", key.keystroke),
        }
    }
    keys
}

/// The canvas's keys that stay off the app's bindings ([`bindings`]).
fn let_through() -> Vec<KeyBinding> {
    let board = Some("Board");
    let text = Some("BoardText");
    vec![
        KeyBinding::new("cmd-0", NoAction, board),
        KeyBinding::new("cmd-k", NoAction, board),
        // The workspace's ⌘K chords, which would hold ⌘K until their second key or a timeout.
        KeyBinding::new("cmd-k e", NoAction, board),
        KeyBinding::new("cmd-k t", NoAction, board),
        KeyBinding::new("cmd-k u", NoAction, board),
        KeyBinding::new("cmd-k w", NoAction, board),
        KeyBinding::new("cmd-k shift-enter", NoAction, board),
        KeyBinding::new("cmd-k cmd-s", NoAction, board),
        // Root's Tab (root::Tab, the next focus): FigCraft's select.next.
        KeyBinding::new("tab", NoAction, board),
        // Root's ⇧Tab (root::TabPrev, the previous focus): FigCraft's select.previous.
        KeyBinding::new("shift-tab", NoAction, board),
        // Root's ⌘C (input::Copy): FigCraft's edit.copy.
        KeyBinding::new("cmd-c", NoAction, board),
        KeyBinding::new("cmd-b", NoAction, text),
        KeyBinding::new("alt-cmd-t", NoAction, text),
        KeyBinding::new("shift-cmd-enter", NoAction, text),
    ]
}

/// A canvas tab: the board in one `.sawit` file, drawn as its view and its document change, and
/// saved as it is edited. An `.excalidraw` file opens
/// imported, in the `.sawit` file beside it ([`BoardView::import`]).
pub struct BoardView {
    /// The canvas file the canvas is bound to: the one it opened, or the one an imported
    /// `.excalidraw` file is saved to.
    path: PathBuf,
    /// The document, or why the file couldn't be opened: the tab then says so, and never saves.
    board: Result<BoardDoc, SharedString>,
    focus: FocusHandle,
    worker: Worker,
    /// The newest render request, and its document. The document is kept so that no other one
    /// can take its address, which the request's key holds.
    requested: Option<(u64, Arc<Document>)>,
    /// What an input method composes in the text being edited, kept while it lasts: the canvas
    /// renders its document in place of the document, and lays out the caret in it
    /// ([`Composition::refresh`]). Kept, its render is asked for once.
    composition: Option<Composition>,
    /// The text edit the input method composed in when it last marked text: its unmark commits
    /// the composition only while that edit still has it (ruling P2-C9, [`text_input`]).
    ime_owner: Option<ImeEditOwner>,
    /// The newest frame, painted until a newer one arrives.
    frame: Option<Frame>,
    /// The images of the frames replaced since the last paint, which takes them out of the
    /// window's sprite atlas.
    stale: Vec<Arc<RenderImage>>,
    /// The view has been fitted to the content: on the first frame with a size.
    fitted: bool,
    /// Where the canvas was in the window when it was last painted.
    bounds: Bounds<Pixels>,
    /// The left button went down on the canvas and hasn't come up: its moves and its release go
    /// to the tools wherever the pointer is ([`events`]).
    dragging: bool,
    /// Where the pointer was in the window, and the modifiers held, at the last pointer event the
    /// tools had: where a drag cut off by the canvas going off screen ends ([`Self::set_shown`]).
    pointer: (Point<Pixels>, Modifiers),
    /// Space is held down on the canvas, and no text is edited: a press of the left button pans
    /// instead of reaching the tools ([`events`]).
    space_held: bool,
    /// The canvas is being panned by hand, with Space and the left button or with the middle
    /// button ([`events`]).
    pan: Option<events::Pan>,
    /// Watches for the canvas losing the keyboard, whereupon Space's release goes elsewhere. Set
    /// up with the first render, which has the window.
    keyboard_watch: Option<Subscription>,
    /// The window it was last painted in, whose sprite atlas holds its images.
    window: Option<AnyWindowHandle>,
    /// A zoom gesture is on until then: frames render at [`PREVIEW_QUALITY`] to keep up.
    zooming_until: Option<Instant>,
    /// Ends the zoom gesture [`ZOOM_SETTLE`] after its last step, for a full-quality render.
    zoom_settle: Option<Task<()>>,
    /// This canvas's own writer: the error it keeps is of its newest save, so it's read right
    /// after each save it waits for.
    writer: Arc<Writer>,
    /// The document the file holds: as loaded, or as last saved. `None` when the file holds
    /// something else (the last save failed or was refused), or nothing yet of a board with layers
    /// (the old canvas migrated, an `.excalidraw` file imported).
    saved: Option<Arc<Document>>,
    /// The document the autosave waits to save, or saved last.
    pending: Option<Arc<Document>>,
    /// The backup the next save keeps of the board the file holds, as an agent's ops since the
    /// last save was handed over asked for one ([`Self::run_op`], [`store::backup_after`]): the
    /// board's for a `clear` or another canvas loaded in its place, a deletion's for an op that
    /// took some layers off. A save that empties the canvas keeps the board's anyway
    /// ([`Self::queue_save`], CANVAS-CORE-3).
    next_backup: Backup,
    /// The document last kept in a copy of its own because it couldn't be saved as the canvas
    /// closed ([`Self::keep_unsaved`]): it isn't kept twice.
    kept_unsaved: Option<Arc<Document>>,
    /// Saves the document once the canvas has been still for [`SAVE_DELAY`].
    save_later: Option<Task<()>>,
    /// A message at the bottom of the canvas (a failed save, what opening the file dropped), up
    /// for [`TOAST_TIME`].
    pub(crate) toast: Option<SharedString>,
    /// Takes the toast down.
    toast_timer: Option<Task<()>>,
    /// The toolbar's flyout that is open, if any ([`toolbar`]).
    open_flyout: Option<Flyout>,
    /// The tool of each of the toolbar's groups used last, by the group's key: a group that shows
    /// one of its tools at a time shows it while none of them is in use ([`toolbar`]).
    last_tools: std::collections::HashMap<&'static str, figcraft_engine::ToolId>,
    /// The toolbar's Templates & table picker, while it is open ([`toolbar`]).
    picker: Option<Picker>,
    /// The kind of file the chrome was last drawn for: when another kind is on show, an open
    /// flyout or picker closes ([`Self::settle_chrome`]).
    shown_kind: Option<DocKind>,
    /// The layers panel shows beside the canvas: at first beside a design file, not beside a
    /// board (spec D11). `view.layers` (⌥1, the top bar's toggle) and `view.leftSidebar` (⇧⌘\)
    /// turn it over (ruling P4-C10). Hiding the UI leaves it as it is, and the panel comes back
    /// with the UI (ruling P4-C8).
    show_layers: bool,
    /// The layers whose own layers the layer tree lists ([`layers_panel`]): those opened by hand,
    /// and those opened to show a selection. A filter shows the layers holding what it finds open
    /// without adding them here, so clearing it gives the tree back as it was (ruling P4-C5).
    expanded: HashSet<NodeId>,
    /// The layers panel's name filter, as typed in its field.
    layer_filter: String,
    /// The layer whose new name is being typed in its row of the layers panel.
    renaming: Option<NodeId>,
    /// The layers being dragged in the layers panel, and where they would land.
    drag: Option<layers_panel::LayerDrag>,
    /// The row a ⇧-click in the layers panel selects from: the last one clicked alone or with ⌘.
    layer_anchor: Option<NodeId>,
    /// What the layers panel keeps between frames ([`layers_panel`]).
    layers: layers_panel::LayersPanel,
    /// The Design panel shows beside the canvas: at first beside a design file, not beside a
    /// board (spec D11). `view.design` (⌥8, the top bar's toggle) turns it over. Hiding the UI
    /// leaves it as it is, and the panel comes back with the UI (ruling P5-C18).
    show_design: bool,
    /// What the Design panel keeps between frames ([`design_panel`]).
    design: design_panel::DesignPanel,
    /// The scrub of a Design panel field that is on ([`BoardView::scrub_begin`]).
    scrub: Option<design_panel::Scrub>,
    /// The context menu, the Actions palette and the keyboard-shortcuts sheet: which is open, and
    /// what each keeps between frames ([`menus`]).
    menus: menus::Menus,
    /// The canvas's tab is on screen, as the workspace last said ([`Self::set_shown`]).
    shown: bool,
    /// The canvas takes the keyboard with the next frame it is drawn in
    /// ([`Self::take_keyboard`]).
    wants_keyboard: bool,
    /// The canvas had the keyboard when its tab went off screen, and gave it to the workspace:
    /// shown again, it takes it back, unless something else took it meanwhile
    /// ([`Self::set_shown`]).
    gave_keyboard: bool,
    /// The agents at work on the canvas, their placeholders and their ops waiting to run
    /// ([`presence`]).
    agents: presence::Presence,
    /// Web frames ([`web`]): UI screens, and route frames of a dev server's pages. `None` only in
    /// a view made without them.
    web: Option<web::WebFrames>,
    /// UI screens' bar, Preview and Send to Agent ([`screens`]).
    screen_ui: screens::ScreenUi,
    /// Add Routes from Dev Server, the route frames' bar, and agents' routes works ([`routes`]).
    routes_ui: routes::RoutesUi,
    _frames: Task<()>,
    _quit: Subscription,
    _release: Subscription,
}

impl EventEmitter<ToggleCanvasMaximize> for BoardView {}
impl EventEmitter<AgentActivity> for BoardView {}

pub use presence::AgentActivity;

impl BoardView {
    /// The canvas file the canvas is bound to, and saves to.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The document. `None` when the file couldn't be opened.
    pub fn board(&self) -> Option<&BoardDoc> {
        self.board.as_ref().ok()
    }

    /// The document itself, as a save would write it ([`BoardDoc::document_itself`]). `None` when
    /// the file couldn't be opened.
    fn document(&self) -> Option<Arc<Document>> {
        self.board().and_then(|board| board.document_itself().ok())
    }

    pub fn board_mut(&mut self) -> Option<&mut BoardDoc> {
        self.board.as_mut().ok()
    }

    /// Puts `board` on show in place of the document, as a file opened over it does.
    #[cfg(test)]
    fn replace_doc(&mut self, board: BoardDoc) {
        self.board = Ok(board);
    }

    fn view_mut(&mut self) -> Option<&mut ViewState> {
        let ds = self.board_mut()?.session.ds_mut().ok()?;
        Some(&mut ds.view)
    }
}

impl Focusable for BoardView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

#[cfg(test)]
mod tests;
