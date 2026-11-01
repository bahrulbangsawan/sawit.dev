//! The keymap as the editor lists it: a row for each binding, the defaults' and the user's, with
//! where it comes from, and on request one for each action nothing binds.

use gpui_kit::*;

use super::file::Location;
use super::{KeymapStore, Row, Source, is_internal, names};

impl KeymapStore {
    /// Where the file turns default `ix` off, if it does: a `null`, or an unbind of the default's
    /// action, on the default's keystrokes, in a context that covers the default's (none, or one
    /// GPUI reads as a superset), as GPUI decides which bindings a later one turns off.
    fn turned_off(&self, ix: usize) -> Option<Location> {
        let default = &self.defaults[ix];
        self.loaded.iter().find_map(|(at, binding)| {
            if binding.keystrokes() != default.keystrokes() {
                return None;
            }
            let covers = match (binding.predicate(), default.predicate()) {
                (None, _) => true,
                (Some(_), None) => false,
                (Some(off), Some(on)) => off.is_superset(&on),
            };
            let off = is_no_action(binding.action())
                || (is_unbind(binding.action())
                    && self.user.binding(*at).and_then(|b| b.entry.unbinds())
                        == Some(default.action().name()));
            (covers && off).then_some(*at)
        })
    }

    /// Whether `row`, as the editor read it, is still a row of the keymap: the place in the file
    /// a user's row came from may hold another binding once the file has changed. An action with
    /// no binding has no place in the file: its row only adds a binding, which stays right
    /// whatever the file holds now.
    pub fn is_current(&self, row: &Row, cx: &App) -> bool {
        if row.source == Source::Unbound {
            return true;
        }
        self.rows(false, cx).iter().any(|r| {
            r.source == row.source
                && r.action == row.action
                && r.args == row.args
                && r.keystrokes == row.keystrokes
                && r.context == row.context
        })
    }

    /// Every row of the keymap: the defaults (those the user turned off as [`Source::Removed`]),
    /// the user's bindings, then, with `unbound`, each action nothing binds. The defaults that
    /// only let keys through to a view (`zed::NoAction`) and the file's unbinds are left out: they
    /// show as the rows they change.
    pub fn rows(&self, unbound: bool, cx: &App) -> Vec<Row> {
        let mut rows = Vec::new();
        for (ix, binding) in self.defaults.iter().enumerate() {
            let action = binding.action();
            if is_no_action(action) || is_unbind(action) {
                continue;
            }
            let source = match self.turned_off(ix) {
                Some(unbind) => Source::Removed {
                    default: ix,
                    unbind,
                },
                None => Source::Default(ix),
            };
            rows.push(row(source, binding));
        }
        for (at, binding) in &self.loaded {
            if is_unbind(binding.action()) {
                continue;
            }
            rows.push(row(Source::User(*at), binding));
        }
        if unbound {
            let bound: std::collections::HashSet<&str> =
                rows.iter().map(|r| r.action.as_ref()).collect();
            let mut extra: Vec<Row> = cx
                .all_action_names()
                .iter()
                .filter(|name| !bound.contains(**name) && !is_internal(name))
                .map(|name| Row {
                    source: Source::Unbound,
                    action: (*name).into(),
                    label: names::humanize_action(name).into(),
                    args: None,
                    keystrokes: SharedString::default(),
                    display: SharedString::default(),
                    context: None,
                })
                .collect();
            extra.extend(crate::board::unbound_rows(&rows));
            rows.extend(extra);
        }
        rows.sort_by(|a, b| {
            a.label
                .cmp(&b.label)
                .then_with(|| a.context.cmp(&b.context))
                .then_with(|| a.keystrokes.cmp(&b.keystrokes))
        });
        rows
    }
}

/// The row of `binding` from `source`.
fn row(source: Source, binding: &KeyBinding) -> Row {
    let action = binding.action();
    let args = binding.action_input();
    let label = crate::board::command_label(action).unwrap_or_else(|| {
        if is_no_action(action) {
            "(keys turned off)".into()
        } else {
            names::humanize_action(action.name())
        }
    });
    Row {
        source,
        action: action.name().into(),
        label: label.into(),
        args,
        keystrokes: names::unparse(binding.keystrokes()).into(),
        display: names::display(binding.keystrokes()).into(),
        context: binding.predicate().map(|p| p.to_string().into()),
    }
}
