//! The key bindings for `sawit-mcp`'s keymap tools and `sawit list keys` (see
//! `sawit_core::keymap`): the rows the keymap editor lists, and a binding added, changed, removed
//! or restored with the editor's own checks, written as the editor writes `keymap.json`.

use gpui_kit::*;
use sawit_core::keymap::{BindingRef, KeySource, KeymapList, KeymapOp, KeymapRow};
use serde_json::{Value, json};

use super::editor::{
    binding_counts, clash_notes, conflicts, is_active, matches_query, starts_with_keys,
};
use super::{KeymapStore, Row, Source, is_internal, names, store};

/// Runs `op` on the keymap, which every window shares, and answers it.
pub fn run(op: KeymapOp, cx: &mut App) -> Result<Value, String> {
    match op {
        KeymapOp::List {
            filter,
            keys,
            unbound,
        } => {
            let list = list(filter.as_deref(), keys.as_deref(), unbound, cx)?;
            serde_json::to_value(list).map_err(|e| e.to_string())
        }
        KeymapOp::Bind {
            keys,
            action,
            args,
            context,
            replace,
            confirm,
        } => bind(
            &keys,
            &action,
            args,
            context.as_deref(),
            replace.as_deref(),
            confirm,
            cx,
        ),
        KeymapOp::Unbind {
            keys,
            context,
            action,
            args,
            restore,
            confirm,
        } => {
            let named = Named {
                keys: &keys,
                context: context.as_deref(),
                action: action.as_deref(),
                args: args.as_ref(),
            };
            unbind(named, restore, confirm, cx)
        }
    }
}

/// The rows of the keymap editor that match every word of `filter` and start with the keystrokes
/// `keys`, with the actions no key runs when `unbound`, as the editor's filter, its keystroke
/// search and its Show Actions Without a Binding find them. A row conflicts as the editor marks
/// it: against every binding, not only those listed.
fn list(
    filter: Option<&str>,
    keys: Option<&str>,
    unbound: bool,
    cx: &App,
) -> Result<KeymapList, String> {
    let keys = keys
        .map(str::trim)
        .filter(|keys| !keys.is_empty())
        .map(keystrokes)
        .transpose()?;
    let query = filter.unwrap_or_default().trim().to_lowercase();
    let entity = store(cx);
    let store = entity.read(cx);
    let all = store.rows(unbound, cx);
    let counts = binding_counts(&all);
    let rows = all
        .iter()
        .filter(|row| matches_query(row, &query))
        .filter(|row| {
            keys.as_deref()
                .is_none_or(|keys| starts_with_keys(&row.keystrokes, keys))
        })
        .map(|row| wire_row(row, conflicts(row, &counts)))
        .collect();
    Ok(KeymapList {
        path: store.path().to_path_buf(),
        writable: store.can_write(),
        errors: store.errors().map(String::from).collect(),
        rows,
    })
}

/// Binds `keys` to `action` (with `args`) in `context`, as the binding form saves a new binding,
/// or in the place of the binding `replace` names, as it saves a changed one: the same checks
/// first (keystrokes, an action that builds with its arguments, a context that reads), and
/// nothing written when one fails. The answer is the binding as the keymap now lists it, and the
/// notes the form shows on what it clashes with.
fn bind(
    keys: &str,
    action: &str,
    args: Option<Value>,
    context: Option<&str>,
    replace: Option<&BindingRef>,
    confirm: bool,
    cx: &mut App,
) -> Result<Value, String> {
    let entity = store(cx);
    ready_to_write(entity.read(cx), confirm)?;
    let keys = keystrokes(keys)?;
    let action = action.trim();
    if action.is_empty() {
        return Err(
            "Name the action the keys run, as keymap_list shows it (workspace::CloseAllTabs)."
                .into(),
        );
    }
    if is_internal(action) {
        return Err(format!(
            "{action} only turns keys off: keymap_unbind does that."
        ));
    }
    let built = cx
        .build_action(action, args.clone())
        .map_err(|e| format!("Nothing was changed: {e}"))?;
    let written = context.map(str::trim).filter(|c| !c.is_empty());
    let shown = canonical_context(written.unwrap_or_default())?;
    let replaced = match replace {
        None => None,
        Some(old) => {
            let named = Named {
                keys: &keystrokes(&old.keys)?,
                context: old.context.as_deref(),
                action: Some(&old.action),
                args: old.args.as_ref(),
            };
            let rows = entity.read(cx).rows(false, cx);
            Some(find_row(&rows, &named, Wanted::Running)?)
        }
    };
    let file = entity.read(cx).path().to_path_buf();
    match &replaced {
        None => entity.update(cx, |store, cx| {
            store.add(written, &keys, action, args.clone(), cx)
        })?,
        Some(row) => {
            // A default built in code with arguments the keymap can't show: writing it back
            // would bind another action, as the form says too.
            if let Source::Default(ix) = row.source
                && row.args.is_none()
                && action == row.action.as_ref()
                && !entity.read(cx).defaults()[ix]
                    .action()
                    .partial_eq(built.as_ref())
            {
                return Err("This default's action has arguments the keymap can't show. Turn it off with keymap_unbind, then bind the keys with the arguments.".into());
            }
            let source = row.source;
            entity.update(cx, |store, cx| {
                store.change(source, written, &keys, action, args.clone(), cx)
            })?
        }
    }
    let store = entity.read(cx);
    let rows = store.rows(false, cx);
    let counts = binding_counts(&rows);
    let bound = rows
        .iter()
        .filter(|row| {
            is_active(row)
                && row.keystrokes.as_ref() == keys
                && row.context.as_deref() == shown.as_deref()
                && row.action.as_ref() == action
                && same_args(row, args.as_ref())
        })
        .max_by_key(|row| matches!(row.source, Source::User(_)))
        .ok_or_else(|| {
            let errors: Vec<&str> = store.errors().collect();
            format!(
                "{} was written, but its binding doesn't load: {}",
                file.display(),
                errors.join("; ")
            )
        })?;
    let mut answer = json!({
        "bound": wire_row(bound, conflicts(bound, &counts)),
        "file": file,
    });
    if let Some(row) = &replaced {
        answer["replaced"] = json!(wire_row(row, false));
    }
    let notes: Vec<String> = clash_notes(
        &rows,
        &keys,
        shown.as_deref().unwrap_or_default(),
        Some(bound.source),
    )
    .into_iter()
    .map(|(_, note)| note)
    .collect();
    if !notes.is_empty() {
        answer["clashes"] = json!(notes);
    }
    Ok(answer)
}

/// Takes the binding `named` names out, as the editor's remove button does: the user's own
/// leaves the file, a default is turned off there. With `restore`, the turned-off default it
/// names comes back.
fn unbind(named: Named, restore: bool, confirm: bool, cx: &mut App) -> Result<Value, String> {
    let entity = store(cx);
    ready_to_write(entity.read(cx), confirm)?;
    let keys = keystrokes(named.keys)?;
    let named = Named {
        keys: &keys,
        ..named
    };
    let wanted = match restore {
        true => Wanted::Removed,
        false => Wanted::Running,
    };
    let row = find_row(&entity.read(cx).rows(false, cx), &named, wanted)?;
    let source = row.source;
    entity.update(cx, |store, cx| store.remove(source, cx))?;
    let (done, note) = match source {
        Source::User(_) => ("removed", "It is out of keymap.json."),
        Source::Removed { .. } => (
            "restored",
            "The default runs again: keymap.json no longer turns it off.",
        ),
        _ => (
            "turned_off",
            "keymap.json turns the default off now; keymap_unbind with restore: true brings it back.",
        ),
    };
    Ok(json!({
        done: wire_row(&row, false),
        "note": note,
        "file": entity.read(cx).path(),
    }))
}

/// A binding as an op names it ([`BindingRef`], or an unbind's fields): its keystrokes as the file
/// writes them, and what picks it out among the bindings of those keys.
#[derive(Clone, Copy)]
struct Named<'a> {
    keys: &'a str,
    /// `None`: any context; `""`: none (anywhere).
    context: Option<&'a str>,
    action: Option<&'a str>,
    args: Option<&'a Value>,
}

/// The rows an op may take: the bindings that run, or the defaults the user turned off.
#[derive(Clone, Copy, PartialEq)]
enum Wanted {
    Running,
    Removed,
}

/// The one row of `rows` that `named` names among those `wanted`. None or several is an error
/// that lists the bindings of the keys, so that the agent can name the one it means.
fn find_row(rows: &[Row], named: &Named, wanted: Wanted) -> Result<Row, String> {
    let context = named.context.map(canonical_context).transpose()?;
    let on_keys: Vec<&Row> = rows
        .iter()
        .filter(|row| row.keystrokes.as_ref() == named.keys)
        .collect();
    let found: Vec<&Row> = on_keys
        .iter()
        .copied()
        .filter(|row| match wanted {
            Wanted::Running => is_active(row),
            Wanted::Removed => matches!(row.source, Source::Removed { .. }),
        })
        .filter(|row| {
            context
                .as_ref()
                .is_none_or(|context| row.context.as_deref() == context.as_deref())
        })
        .filter(|row| {
            named
                .action
                .is_none_or(|action| row.action.as_ref() == action)
        })
        .filter(|row| named.args.is_none() || same_args(row, named.args))
        .collect();
    let list = |rows: &[&Row]| {
        rows.iter()
            .map(|row| describe(row))
            .collect::<Vec<_>>()
            .join("; ")
    };
    match found.as_slice() {
        [row] => Ok((*row).clone()),
        [] if on_keys.is_empty() => Err(format!(
            "Nothing is bound to {}: keymap_list with keys shows what is.",
            named.keys
        )),
        [] => {
            let looked_for = match wanted {
                Wanted::Running => "binding that runs",
                Wanted::Removed => "turned-off default",
            };
            Err(format!(
                "No {looked_for} of {} matches. Its bindings: {}.",
                named.keys,
                list(&on_keys)
            ))
        }
        several => Err(format!(
            "Several bindings of {} match: {}. Name its context (\"\" for anywhere), action or args too.",
            named.keys,
            list(several)
        )),
    }
}

/// A row as an error lists it: `cmd-w in Workspace: workspace: close active tab
/// (workspace::CloseActiveTab), default`.
fn describe(row: &Row) -> String {
    let source = match row.source {
        Source::Default(_) => "default",
        Source::User(_) => "user",
        Source::Removed { .. } => "removed",
        Source::Unbound => "unbound",
    };
    let args = row
        .args
        .as_ref()
        .map(|args| format!(" {args}"))
        .unwrap_or_default();
    format!(
        "{} in {}: {} ({}{args}), {source}",
        row.keystrokes,
        row.context.as_deref().unwrap_or("any context"),
        row.label,
        row.action
    )
}

/// Refuses a change while `keymap.json` can't be read, which writing it would lose, or, unless
/// `confirm`, while it has comments of the user's, which writing it drops.
fn ready_to_write(store: &KeymapStore, confirm: bool) -> Result<(), String> {
    if !store.can_write() {
        let why = store
            .errors()
            .next()
            .unwrap_or("keymap.json can't be read.");
        return Err(format!(
            "Nothing was changed: writing keymap.json while it can't be read would lose what is in it. {why} Fix the file first; keymap_list shows its errors."
        ));
    }
    if !confirm && store.has_user_comments() {
        return Err(format!(
            "Nothing was changed: {} has comments, and a change writes the file again without them, as the keymap editor does. Tell the user, then pass confirm: true to write it anyway, or change the file by hand.",
            store.path().display()
        ));
    }
    Ok(())
}

/// `text` as the keymap file writes keystrokes (`cmd-k cmd-s`, the modifiers in GPUI's order), or
/// why it is none.
fn keystrokes(text: &str) -> Result<String, String> {
    names::normalize_keystrokes(text).ok_or_else(|| {
        format!(
            "“{}” aren't keystrokes: write them as keymap.json does, such as cmd-shift-w, ctrl-alt-left or the chord cmd-k cmd-s.",
            text.trim()
        )
    })
}

/// `text` as a binding's context reads (GPUI's own writing of it), `None` for none (anywhere), or
/// why it can't be read.
fn canonical_context(text: &str) -> Result<Option<String>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    KeyBindingContextPredicate::parse(text)
        .map(|predicate| Some(predicate.to_string()))
        .map_err(|e| format!("Nothing was changed: the context “{text}” can't be read: {e}"))
}

/// Whether `row`'s arguments are `args`, compared as JSON.
fn same_args(row: &Row, args: Option<&Value>) -> bool {
    let shown = row
        .args
        .as_deref()
        .map(|text| serde_json::from_str::<Value>(text).ok());
    match (shown, args) {
        (None, None) => true,
        (Some(Some(shown)), Some(args)) => &shown == args,
        _ => false,
    }
}

/// `row` as `keymap_list` and `sawit list keys` get it.
fn wire_row(row: &Row, conflict: bool) -> KeymapRow {
    KeymapRow {
        action: row.action.to_string(),
        label: row.label.to_string(),
        args: row.args.as_deref().map(|text| {
            serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_string()))
        }),
        keys: row.keystrokes.to_string(),
        display: row.display.to_string(),
        context: row.context.as_ref().map(ToString::to_string),
        source: match row.source {
            Source::Default(_) => KeySource::Default,
            Source::User(_) => KeySource::User,
            Source::Removed { .. } => KeySource::Removed,
            Source::Unbound => KeySource::Unbound,
        },
        conflict,
    }
}

#[cfg(test)]
mod tests;
