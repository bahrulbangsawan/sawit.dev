//! The canvas's commands as the keymap has them: their names and rows in the keymap editor, the
//! keys their hints show as the keymap binds them now, and the user's chords kept from waiting on
//! a key the canvas binds alone.

use gpui_kit::*;
use sawit_board::shortcuts;
use serde_json::{Value, json};

use super::Command;

/// What the keymap editor calls a [`Command`] binding: `canvas: ` and the command's title, as
/// FigCraft's shortcuts panel or its menu writes it. `None` for any other action.
pub fn command_label(action: &dyn Action) -> Option<String> {
    let command = action.as_any().downcast_ref::<Command>()?;
    Some(format!(
        "canvas: {}",
        command_title(&command.id).to_lowercase()
    ))
}

/// The canvas commands' keys as the hints show them, kept until the keymap changes: each lookup
/// resolves a command's bindings against the whole keymap, and the chrome asks for a dozen or so
/// on every frame.
#[derive(Default)]
struct HintKeys {
    version: Option<KeymapVersion>,
    keys: std::collections::HashMap<String, Vec<String>>,
}
impl Global for HintKeys {}

/// The keys that run canvas command `id` on the canvas as the keymap binds them now, as the
/// canvas's hints show them: the tooltips, the menus, the Actions palette and the shortcuts
/// sheet. A key that is one of FigCraft's for the command reads as FigCraft writes it (`⌃⇧?`,
/// `⌘+`), so the defaults read as FigCraft's own app shows them; the defaults come first, in
/// FigCraft's order, then the keys the user gave the command, the last bound first. A key Sawit
/// keeps, or that the user took out, isn't among them. Empty when no key runs the command: on the
/// canvas, or else while it edits text (the text commands' keys).
pub fn command_keys_all(id: &str, cx: &mut App) -> Vec<String> {
    let version = cx.key_bindings().borrow().version();
    let cache = cx.default_global::<HintKeys>();
    if cache.version != Some(version) {
        cache.version = Some(version);
        cache.keys.clear();
    }
    if let Some(keys) = cache.keys.get(id) {
        return keys.clone();
    }
    let keys = live_command_keys(id, cx);
    cx.global_mut::<HintKeys>()
        .keys
        .insert(id.to_string(), keys.clone());
    keys
}

/// [`command_keys_all`], looked up.
fn live_command_keys(id: &str, cx: &App) -> Vec<String> {
    use figcraft_engine::keymap::{self as figcraft_keys, Platform};
    let names = crate::keymap::names::normalize_keystrokes;
    // The command's default keys (FigCraft's, or Sawit's for a command FigCraft gives none),
    // each as it is bound and as FigCraft writes it.
    let figcraft: Vec<(String, String)> = shortcuts::defaults()
        .filter(|binding| binding.id == id)
        .flat_map(|binding| binding.keys.iter().copied())
        .filter_map(|spec| {
            let bound = names(&shortcuts::binding_keystroke(spec)?)?;
            Some((bound, figcraft_keys::display(spec, Platform::Mac)))
        })
        .collect();
    let command = Command { id: id.to_string() };
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let stack = |canvas: &str| -> Vec<KeyContext> {
        ["Root", "Workspace", canvas]
            .into_iter()
            .map(|context| KeyContext::parse(context).unwrap_or_default())
            .collect()
    };
    for stack in [stack("Board"), stack("Board BoardText")] {
        // As GPUI's own lookup: a binding counts where it matches and no other of its keys runs
        // first.
        let live: Vec<&KeyBinding> = keymap
            .bindings_for_action(&command)
            .filter(|binding| {
                let (found, _) = keymap.bindings_for_input(binding.keystrokes(), &stack);
                found
                    .first()
                    .is_some_and(|first| first.action().partial_eq(binding.action()))
            })
            .collect();
        if live.is_empty() {
            continue;
        }
        let (users, defaults): (Vec<&KeyBinding>, Vec<&KeyBinding>) = live
            .into_iter()
            .partition(|binding| binding.meta() == Some(crate::keymap::USER));
        let mut keys: Vec<String> = Vec::new();
        for binding in defaults.into_iter().chain(users.into_iter().rev()) {
            let typed = crate::keymap::names::unparse(binding.keystrokes());
            let shown = names(&typed)
                .and_then(|typed| figcraft.iter().find(|(bound, _)| *bound == typed))
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| crate::keymap::names::display(binding.keystrokes()));
            if !keys.contains(&shown) {
                keys.push(shown);
            }
        }
        return keys;
    }
    Vec::new()
}

/// The first of [`command_keys_all`]: what a tooltip or a menu item shows.
pub fn command_keys(id: &str, cx: &mut App) -> Option<String> {
    command_keys_all(id, cx).into_iter().next()
}

/// The title of FigCraft's command `id`: its shortcut's (FigCraft's, or Sawit's for a command it
/// gives a key, `shortcuts::defaults`), else its menu item's, else its own.
fn command_title(id: &str) -> &str {
    shortcuts::defaults()
        .find(|binding| binding.id == id)
        .map(|binding| binding.title)
        .or_else(|| {
            figcraft_engine::catalog::items()
                .find(|item| item.id == id)
                .map(|item| item.label)
        })
        .or_else(|| figcraft_engine::find_command(id).map(|command| command.label))
        .unwrap_or(id)
}

/// The keymap editor's rows for the canvas commands no row of `rows` binds: those of FigCraft's
/// shortcuts and menus that the canvas runs or carries out itself (import, export, save and copy
/// as, [`commands`]), so a key can be given to any of them. The commands the canvas doesn't offer
/// at all ([`shortcuts::is_offered`]: hidden, or of no use in it) aren't among them.
pub fn unbound_rows(rows: &[crate::keymap::Row]) -> Vec<crate::keymap::Row> {
    let bound: std::collections::HashSet<String> = rows
        .iter()
        .filter(|row| row.action.as_ref() == <Command as Action>::name_for_type())
        .filter_map(|row| {
            let args: Value = serde_json::from_str(row.args.as_deref()?).ok()?;
            Some(args.get("id")?.as_str()?.to_string())
        })
        .collect();
    let mut seen = std::collections::HashSet::new();
    figcraft_engine::keymap::KEYS
        .iter()
        .map(|binding| binding.id)
        .chain(figcraft_engine::catalog::items().map(|item| item.id))
        .filter(|id| shortcuts::is_offered(id) && !bound.contains(*id))
        .filter(|id| seen.insert(*id))
        .map(|id| crate::keymap::Row {
            source: crate::keymap::Source::Unbound,
            action: <Command as Action>::name_for_type().into(),
            label: format!("canvas: {}", command_title(id).to_lowercase()).into(),
            args: Some(json!({ "id": id }).to_string().into()),
            keystrokes: SharedString::default(),
            display: SharedString::default(),
            context: Some(shortcuts::CANVAS_CONTEXT.into()),
        })
        .collect()
}

/// `NoAction` on the canvas for each chord of `user` (the keymap file's bindings, bound after
/// `defaults`) that starts with a key the canvas binds alone in `defaults`: bound after them, it
/// keeps that key from waiting for the chord's next key (GPUI waits up to a second when a chord
/// bound after a key's own binding goes on from it), as [`let_through`] does for the
/// workspace's ⌘K chords. Such a chord then works everywhere but on the canvas, unless the user
/// bound it on the canvas itself.
pub fn let_through_chords<'a>(
    defaults: &[KeyBinding],
    user: impl IntoIterator<Item = &'a KeyBinding>,
    cx: &App,
) -> Vec<KeyBinding> {
    let canvas = [
        KeyContext::parse("Workspace").unwrap_or_default(),
        KeyContext::parse("Board").unwrap_or_default(),
    ];
    let editing = [
        KeyContext::parse("Workspace").unwrap_or_default(),
        KeyContext::parse("Board BoardText").unwrap_or_default(),
    ];
    let on_canvas = |binding: &KeyBinding| {
        binding
            .predicate()
            .is_some_and(|p| p.eval(&canvas) || p.eval(&editing))
    };
    let alone: Vec<&KeybindingKeystroke> = defaults
        .iter()
        .filter(|binding| binding.keystrokes().len() == 1 && on_canvas(binding))
        .map(|binding| &binding.keystrokes()[0])
        .collect();
    let board = KeyBindingContextPredicate::parse("Board")
        .ok()
        .map(std::rc::Rc::new);
    user.into_iter()
        .filter(|chord| {
            chord.keystrokes().len() > 1
                && !is_no_action(chord.action())
                && !is_unbind(chord.action())
                && !on_canvas(chord)
                && alone.contains(&&chord.keystrokes()[0])
        })
        .filter_map(|chord| {
            let keys = crate::keymap::names::unparse(chord.keystrokes());
            KeyBinding::load(&keys, Box::new(NoAction), board.clone(), false, None, cx).ok()
        })
        .collect()
}
