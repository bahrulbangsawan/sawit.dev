//! What the editor reads in the keymap's rows: which bindings run, which have keys another one
//! has in their context, which a chord or its first keys would leave out of reach, and the notes
//! the form (and the agent's `keymap_bind`) gives on them.

use std::collections::HashMap;

use gpui_kit::{Keystroke, OperatingSystem, SharedString};

use super::starts_with_keys;
use crate::keymap::{Row, Source, names};

/// Whether `row` is a binding that runs: a default or a user's, not one that only turns keys off.
pub(crate) fn is_active(row: &Row) -> bool {
    matches!(row.source, Source::Default(_) | Source::User(_))
        && row.action.as_ref() != "zed::NoAction"
}

/// How many active bindings each (context, keystrokes) of `rows` has.
pub(crate) type BindingCounts = HashMap<(Option<SharedString>, SharedString), usize>;

/// The active bindings of `rows` counted by their context and keystrokes, for [`conflicts`].
pub(crate) fn binding_counts(rows: &[Row]) -> BindingCounts {
    let mut counts = BindingCounts::new();
    for row in rows.iter().filter(|r| is_active(r)) {
        *counts
            .entry((row.context.clone(), row.keystrokes.clone()))
            .or_default() += 1;
    }
    counts
}

/// Whether `row` is a binding that runs and another one that runs has its keys in its context
/// (`counts`, from [`binding_counts`]): only one of them runs.
pub(crate) fn conflicts(row: &Row, counts: &BindingCounts) -> bool {
    is_active(row)
        && counts
            .get(&(row.context.clone(), row.keystrokes.clone()))
            .is_some_and(|n| *n > 1)
}

/// The other active bindings of `keys` (as the keymap file writes them) among `rows`, the row
/// `own` aside (the one being changed): the labels of those in `context` (as a binding's context
/// reads, `""` for anywhere), which would clash, then how many in other contexts.
pub(super) fn clashes(
    rows: &[Row],
    keys: &str,
    context: &str,
    own: Option<Source>,
) -> (Vec<String>, usize) {
    if keys.is_empty() {
        return (Vec::new(), 0);
    }
    let mut same = Vec::new();
    let mut elsewhere = 0;
    for row in rows
        .iter()
        .filter(|r| is_active(r) && Some(r.source) != own)
    {
        if row.keystrokes.as_ref() != keys {
            continue;
        }
        if row.context.as_deref().unwrap_or_default() == context {
            same.push(row.label.to_string());
        } else {
            elsewhere += 1;
        }
    }
    (same, elsewhere)
}

/// The active bindings of `rows`, in `context` or anywhere, the row `own` aside, that `keys`
/// start (`⌘K W (workspace: close all tabs)`), then those that start `keys`: a key bound alone
/// and a chord on it either keep the key waiting for the chord's next key, or leave the chord out
/// of reach.
pub(super) fn chord_clashes(
    rows: &[Row],
    keys: &str,
    context: &str,
    own: Option<Source>,
) -> (Vec<String>, Vec<String>) {
    if keys.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let (mut longer, mut shorter) = (Vec::new(), Vec::new());
    for row in rows
        .iter()
        .filter(|r| is_active(r) && Some(r.source) != own)
    {
        let row_context = row.context.as_deref().unwrap_or_default();
        if !(row_context == context || row_context.is_empty() || context.is_empty()) {
            continue;
        }
        let named = || format!("{} ({})", row.display, row.label);
        if row.keystrokes.len() > keys.len() && starts_with_keys(&row.keystrokes, keys) {
            longer.push(named());
        } else if keys.len() > row.keystrokes.len() && starts_with_keys(keys, &row.keystrokes) {
            shorter.push(named());
        }
    }
    (longer, shorter)
}

/// What binding `keys` in `context` among `rows` (the row `own` aside) clashes with, as the
/// binding form says it under its fields, each note with its banner's id: another binding of the
/// same keys there, chords the keys start or keys that start them, or bindings of the keys in
/// other contexts. The agent's `keymap_bind` answers with the same notes.
pub(crate) fn clash_notes(
    rows: &[Row],
    keys: &str,
    context: &str,
    own: Option<Source>,
) -> Vec<(&'static str, String)> {
    let (same, elsewhere) = clashes(rows, keys, context, own);
    let (longer, shorter) = chord_clashes(rows, keys, context, own);
    let shown = names::display_typed(&parse_keys(keys));
    let mut notes = Vec::new();
    if !same.is_empty() {
        notes.push((
            "binding-clash",
            format!(
                "{shown} is also bound to {} in this context. Both bindings stay, and this one runs: the binding saved last goes first.",
                same.join(", ")
            ),
        ));
    }
    if !longer.is_empty() {
        notes.push((
            "binding-starts-chords",
            format!(
                "{shown} also starts {} here. Bound alone, it runs at once, and those chords can no longer be typed here.",
                longer.join(", ")
            ),
        ));
    }
    if !shorter.is_empty() {
        notes.push((
            "binding-extends-keys",
            format!(
                "These keys start with {}, bound alone here: with this chord, those keys wait up to a second for the next key before they run.",
                shorter.join(", ")
            ),
        ));
    }
    if same.is_empty() && elsewhere > 0 {
        notes.push((
            "binding-elsewhere",
            format!(
                "These keys are bound in {elsewhere} other context{}: the deepest context with the focus wins.",
                if elsewhere == 1 { "" } else { "s" }
            ),
        ));
    }
    notes
}

fn parse_keys(text: &str) -> Vec<Keystroke> {
    text.split_whitespace()
        .filter_map(|k| Keystroke::parse_with_os(k, OperatingSystem::Mac).ok())
        .collect()
}
