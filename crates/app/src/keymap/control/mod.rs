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
mod tests {
    use gpui_kit::{App, Entity, KeyBinding, TestAppContext};
    use sawit_core::keymap::{BindingRef, KeySource, KeymapList, KeymapOp};
    use serde_json::{Value, json};

    use super::super::tests::{Scratch, runs};
    use super::super::{KeymapStore, init_at, store};
    use super::run;

    gpui_kit::actions!(keymap_control_test, [Close, CloseAll, Other]);

    /// The app's defaults for these tests: ⌘W closes and ⌘K W closes all, in `Workspace`.
    fn start(cx: &mut TestAppContext, scratch: &Scratch) -> Entity<KeymapStore> {
        cx.update(|cx| {
            cx.bind_keys([
                KeyBinding::new("cmd-w", Close, Some("Workspace")),
                KeyBinding::new("cmd-k w", CloseAll, Some("Workspace")),
            ]);
            init_at(scratch.file(), cx);
            store(cx)
        })
    }

    fn list(cx: &mut App, op: Value) -> KeymapList {
        let op: KeymapOp = serde_json::from_value(op).unwrap();
        serde_json::from_value(run(op, cx).unwrap()).unwrap()
    }

    fn bind(cx: &mut App, keys: &str, action: &str) -> Result<Value, String> {
        run(
            KeymapOp::Bind {
                keys: keys.into(),
                action: action.into(),
                args: None,
                context: Some("Workspace".into()),
                replace: None,
                confirm: false,
            },
            cx,
        )
    }

    fn unbind(cx: &mut App, keys: &str, restore: bool) -> Result<Value, String> {
        run(
            KeymapOp::Unbind {
                keys: keys.into(),
                context: None,
                action: None,
                args: None,
                restore,
                confirm: false,
            },
            cx,
        )
    }

    /// The rows on `keys`, by action and where each comes from.
    fn rows_on(cx: &mut App, keys: &str) -> Vec<(String, KeySource)> {
        list(cx, json!({"op": "list", "keys": keys}))
            .rows
            .into_iter()
            .filter(|row| row.keys == keys)
            .map(|row| (row.action, row.source))
            .collect()
    }

    /// A binding an agent makes is the user's, in keymap.json, and runs; the list shows it so,
    /// with the file it is in.
    #[gpui_kit::test]
    fn a_binding_an_agent_makes_lists_as_the_users(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-bind");
        start(cx, &scratch);
        cx.update(|cx| {
            let answer = bind(cx, "shift-cmd-e", "keymap_control_test::Other").unwrap();
            assert_eq!(answer["bound"]["source"], "user", "{answer}");
            assert_eq!(answer["bound"]["keys"], "cmd-shift-e", "{answer}");
            assert_eq!(answer["bound"]["display"], "⇧⌘E", "{answer}");
            assert_eq!(runs(cx, "cmd-shift-e"), ["keymap_control_test::Other"]);
            assert_eq!(
                rows_on(cx, "cmd-shift-e"),
                [("keymap_control_test::Other".to_string(), KeySource::User)]
            );
            let listed = list(cx, json!({"op": "list"}));
            assert_eq!(listed.path, scratch.file());
            assert!(listed.writable && listed.errors.is_empty());
        });
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(text.contains("keymap_control_test::Other"), "{text}");
    }

    /// A default an agent unbinds is turned off in keymap.json and lists as removed; unbinding
    /// it with restore brings it back, as the editor's remove and restore buttons do.
    #[gpui_kit::test]
    fn an_unbound_default_lists_as_removed_until_restored(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-unbind");
        start(cx, &scratch);
        cx.update(|cx| {
            let answer = unbind(cx, "cmd-k w", false).unwrap();
            assert_eq!(
                answer["turned_off"]["action"],
                "keymap_control_test::CloseAll"
            );
            assert!(runs(cx, "cmd-k w").is_empty());
            assert_eq!(
                rows_on(cx, "cmd-k w"),
                [(
                    "keymap_control_test::CloseAll".to_string(),
                    KeySource::Removed
                )]
            );
            // Turned off already: there is nothing that runs there to unbind.
            let again = unbind(cx, "cmd-k w", false).unwrap_err();
            assert!(again.contains("removed"), "{again}");

            let answer = unbind(cx, "cmd-k w", true).unwrap();
            assert_eq!(
                answer["restored"]["action"],
                "keymap_control_test::CloseAll"
            );
            assert_eq!(runs(cx, "cmd-k w"), ["keymap_control_test::CloseAll"]);
            assert_eq!(
                rows_on(cx, "cmd-k w"),
                [(
                    "keymap_control_test::CloseAll".to_string(),
                    KeySource::Default
                )]
            );
        });
    }

    /// The form's checks stand for an agent too: an action that isn't one, a context that can't
    /// be read or keys that aren't keystrokes are refused, and nothing is written.
    #[gpui_kit::test]
    fn a_binding_that_cant_be_made_writes_nothing(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-refused");
        start(cx, &scratch);
        cx.update(|cx| {
            let unknown = bind(cx, "cmd-e", "no_such::Action").unwrap_err();
            assert!(unknown.contains("Nothing was changed"), "{unknown}");
            let context = run(
                KeymapOp::Bind {
                    keys: "cmd-e".into(),
                    action: "keymap_control_test::Other".into(),
                    args: None,
                    context: Some("Workspace &&".into()),
                    replace: None,
                    confirm: false,
                },
                cx,
            )
            .unwrap_err();
            assert!(context.contains("can't be read"), "{context}");
            let keys = bind(cx, "cmd-nope-e", "keymap_control_test::Other").unwrap_err();
            assert!(keys.contains("aren't keystrokes"), "{keys}");
            let off = bind(cx, "cmd-e", "zed::NoAction").unwrap_err();
            assert!(off.contains("keymap_unbind"), "{off}");
            assert!(runs(cx, "cmd-e").is_empty());
        });
        assert!(!scratch.file().exists(), "keymap.json was written");
    }

    /// keymap.json's comments go when it is written again, so a change to a file that has some
    /// is refused until the agent confirms it, and the file stays as it was. The lines a new file
    /// starts with are Sawit's, and don't stop it.
    #[gpui_kit::test]
    fn a_keymap_with_comments_is_written_only_when_confirmed(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-comments");
        let store = start(cx, &scratch);
        std::fs::create_dir_all(&scratch.0).unwrap();
        let mine = "// My bindings.\n[{\"context\": \"Workspace\", \"bindings\": {\"ctrl-alt-h\": \"keymap_control_test::Other\"}}]\n";
        std::fs::write(scratch.file(), mine).unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
            assert!(refused.contains("comments"), "{refused}");
            assert!(refused.contains("confirm: true"), "{refused}");
            let refused = unbind(cx, "cmd-w", false).unwrap_err();
            assert!(refused.contains("comments"), "{refused}");
        });
        assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), mine);
        cx.update(|cx| {
            run(
                KeymapOp::Bind {
                    keys: "cmd-e".into(),
                    action: "keymap_control_test::Other".into(),
                    args: None,
                    context: Some("Workspace".into()),
                    replace: None,
                    confirm: true,
                },
                cx,
            )
            .unwrap();
            assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Other"]);
            assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_control_test::Other"]);
        });
        let text = std::fs::read_to_string(scratch.file()).unwrap();
        assert!(!text.contains("My bindings"), "{text}");
    }

    /// The lines a new keymap.json starts with, on how to write it, are Sawit's: a change goes
    /// through without asking, as the editor's does.
    #[gpui_kit::test]
    fn a_new_keymap_files_own_lines_dont_stop_a_change(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-new-file");
        let store = start(cx, &scratch);
        cx.update(|cx| {
            store.read(cx).ensure_file().unwrap();
            store.update(cx, |store, cx| store.reload(cx));
            bind(cx, "cmd-e", "keymap_control_test::Other").unwrap();
            assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Other"]);
        });
    }

    /// The keystroke search finds the chords that start with the keys, and the filter finds a
    /// binding by the words of its name, as the editor's do.
    #[gpui_kit::test]
    fn the_list_finds_bindings_by_keys_and_by_name(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-list");
        start(cx, &scratch);
        cx.update(|cx| {
            let chords = list(cx, json!({"op": "list", "keys": "cmd-k"}));
            let keys: Vec<&str> = chords.rows.iter().map(|row| row.keys.as_str()).collect();
            assert_eq!(keys, ["cmd-k w"]);
            let named = list(cx, json!({"op": "list", "filter": "Close ALL"}));
            let actions: Vec<&str> = named.rows.iter().map(|row| row.action.as_str()).collect();
            assert_eq!(actions, ["keymap_control_test::CloseAll"]);
            assert_eq!(named.rows[0].context.as_deref(), Some("Workspace"));
            // Actions no key runs show only when asked.
            let unbound = list(
                cx,
                json!({"op": "list", "filter": "keymap_control_test other", "unbound": true}),
            );
            assert_eq!(unbound.rows.len(), 1, "{:?}", unbound.rows);
            assert_eq!(unbound.rows[0].source, KeySource::Unbound);
            let bad = run(
                serde_json::from_value(json!({"op": "list", "keys": "cmd-nope-k"})).unwrap(),
                cx,
            )
            .unwrap_err();
            assert!(bad.contains("aren't keystrokes"), "{bad}");
        });
    }

    /// A binding of keys another binding has in the same context stays beside it, runs first,
    /// and the answer says so, as the form does; the list marks both as conflicting.
    #[gpui_kit::test]
    fn a_bind_says_what_it_clashes_with(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-clash");
        start(cx, &scratch);
        cx.update(|cx| {
            let answer = bind(cx, "cmd-w", "keymap_control_test::Other").unwrap();
            let clashes = answer["clashes"].to_string();
            assert!(clashes.contains("also bound to"), "{answer}");
            assert!(clashes.contains("keymap control test: close"), "{answer}");
            assert_eq!(answer["bound"]["conflict"], true, "{answer}");
            assert_eq!(
                runs(cx, "cmd-w"),
                ["keymap_control_test::Other", "keymap_control_test::Close"]
            );
            let chord = bind(cx, "cmd-k", "keymap_control_test::Other").unwrap();
            assert!(
                chord["clashes"].to_string().contains("also starts"),
                "{chord}"
            );
        });
    }

    /// `replace` changes the binding it names, as the form's Edit does: a default moves to the
    /// new keys (turned off where it was), and the answer names both.
    #[gpui_kit::test]
    fn replace_moves_a_binding_to_new_keys(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-replace");
        start(cx, &scratch);
        cx.update(|cx| {
            let answer = run(
                KeymapOp::Bind {
                    keys: "cmd-e".into(),
                    action: "keymap_control_test::Close".into(),
                    args: None,
                    context: Some("Workspace".into()),
                    replace: Some(Box::new(BindingRef {
                        keys: "cmd-w".into(),
                        context: Some("Workspace".into()),
                        action: "keymap_control_test::Close".into(),
                        args: None,
                    })),
                    confirm: false,
                },
                cx,
            )
            .unwrap();
            assert_eq!(answer["replaced"]["keys"], "cmd-w", "{answer}");
            assert_eq!(answer["bound"]["keys"], "cmd-e", "{answer}");
            assert_eq!(runs(cx, "cmd-e"), ["keymap_control_test::Close"]);
            assert!(runs(cx, "cmd-w").is_empty());
            assert_eq!(
                rows_on(cx, "cmd-w"),
                [("keymap_control_test::Close".to_string(), KeySource::Removed)]
            );
        });
    }

    /// Keys several bindings have are named with their context or action: unnamed, the unbind
    /// is refused and lists them; named, it takes the one meant.
    #[gpui_kit::test]
    fn an_unbind_of_keys_bound_twice_needs_the_one_meant(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-twice");
        start(cx, &scratch);
        cx.update(|cx| {
            run(
                KeymapOp::Bind {
                    keys: "cmd-w".into(),
                    action: "keymap_control_test::Other".into(),
                    args: None,
                    context: Some("Editor".into()),
                    replace: None,
                    confirm: false,
                },
                cx,
            )
            .unwrap();
            let vague = unbind(cx, "cmd-w", false).unwrap_err();
            assert!(vague.contains("Several bindings"), "{vague}");
            assert!(
                vague.contains("Workspace") && vague.contains("Editor"),
                "{vague}"
            );
            let answer = run(
                KeymapOp::Unbind {
                    keys: "cmd-w".into(),
                    context: Some("Editor".into()),
                    action: None,
                    args: None,
                    restore: false,
                    confirm: false,
                },
                cx,
            )
            .unwrap();
            assert_eq!(answer["removed"]["source"], "user", "{answer}");
            assert_eq!(runs(cx, "cmd-w"), ["keymap_control_test::Close"]);
            let nothing = unbind(cx, "ctrl-alt-q", false).unwrap_err();
            assert!(nothing.contains("Nothing is bound"), "{nothing}");
        });
    }

    /// While keymap.json can't be read, a change would write over what the user is in the middle
    /// of writing: it is refused, and the file stays as it is. A file another program changed
    /// since the keymap last read it is read again, and the change must be asked for again.
    #[gpui_kit::test]
    fn a_change_never_writes_over_what_it_hasnt_read(cx: &mut TestAppContext) {
        let scratch = Scratch::new("agent-unreadable");
        let store = start(cx, &scratch);
        std::fs::create_dir_all(&scratch.0).unwrap();
        std::fs::write(scratch.file(), "[{").unwrap();
        cx.update(|cx| {
            store.update(cx, |store, cx| store.reload(cx));
            let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
            assert!(refused.contains("can't be read"), "{refused}");
            let listed = list(cx, json!({"op": "list"}));
            assert!(!listed.writable && listed.errors.len() == 1, "{listed:?}");
        });
        assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), "[{");

        std::fs::write(scratch.file(), "[]").unwrap();
        cx.update(|cx| store.update(cx, |store, cx| store.reload(cx)));
        let by_hand = r#"[{"context": "Workspace", "bindings": {"ctrl-alt-h": "keymap_control_test::Other"}}]"#;
        std::fs::write(scratch.file(), by_hand).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options()
            .write(true)
            .open(scratch.file())
            .unwrap()
            .set_modified(later)
            .unwrap();
        cx.update(|cx| {
            let refused = bind(cx, "cmd-e", "keymap_control_test::Other").unwrap_err();
            assert!(refused.contains("changed outside"), "{refused}");
            assert_eq!(runs(cx, "ctrl-alt-h"), ["keymap_control_test::Other"]);
        });
        assert_eq!(std::fs::read_to_string(scratch.file()).unwrap(), by_hand);
    }
}
