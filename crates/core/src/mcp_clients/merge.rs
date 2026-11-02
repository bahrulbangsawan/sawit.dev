//! Updating an entry the config has: how the server is reached changes, the user's own keys
//! stay.

use serde_json::{Map, Value};

use super::shape::{HELPER_MARK, codex_table, own_path};
use super::{Spec, Transport};

/// The keys of a JSON entry that say how a server is reached. Sawit writes
/// them on every install. The rest of an entry (Cline's `autoApprove`,
/// `disabled`, an `env` or `headers` the user added, a timeout, …) is the
/// user's, and an update keeps it.
const TRANSPORT_KEYS: [&str; 6] = ["type", "command", "args", "url", "httpUrl", "serverUrl"];

/// What an install writes over `old`, the entry the config has. The
/// transport keys come from `fresh`, and those `fresh` lacks go. Every other
/// key stays as the user left it, `fresh` filling in only the missing ones;
/// maps (`env`, `headers`) merge the same way, key by key. When the entry
/// switches between a program and a URL, the other kind's map goes too. The
/// `PATH` Sawit worked out for the old `npx` follows `fresh` too
/// ([`follow_own_path`]).
pub(super) fn merge_entry(old: &Value, fresh: Value) -> Value {
    let (Value::Object(old), Value::Object(fresh)) = (old, &fresh) else {
        return fresh;
    };
    let program = |m: &Map<String, Value>| m.contains_key("command");
    let mut out = old.clone();
    if program(old) != program(fresh) {
        out.shift_remove(if program(fresh) { "headers" } else { "env" });
    }
    follow_own_path(old, &mut out, fresh);
    for key in TRANSPORT_KEYS {
        if !fresh.contains_key(key) {
            out.shift_remove(key);
        }
    }
    // The `headersHelper` Sawit writes is how the server is reached, like the URL: it follows
    // `fresh`, and goes when `fresh` has none. One of the user's own stays.
    let own_helper = |v: &Value| v.as_str().is_some_and(|h| h.contains(HELPER_MARK));
    if !fresh.contains_key("headersHelper") && old.get("headersHelper").is_some_and(own_helper) {
        out.shift_remove("headersHelper");
    }
    for (key, value) in fresh {
        if TRANSPORT_KEYS.contains(&key.as_str()) || key == "headersHelper" {
            // In place: the entry's keys keep their order.
            out.insert(key.clone(), value.clone());
        } else if let (Some(Value::Object(mine)), Value::Object(theirs)) = (out.get_mut(key), value)
        {
            for (k, v) in theirs {
                if !mine.contains_key(k) {
                    mine.insert(k.clone(), v.clone());
                }
            }
        } else if !out.contains_key(key) {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

/// The `PATH` in `old`'s `env` (opencode's and Kilo's `environment`) that
/// Sawit worked out for its `npx` ([`node_path`]) is Sawit's, not the user's:
/// in `out` it becomes `fresh`'s, in place, or goes when `fresh` has none
/// (after nvm moved on, the old folder may have no node left). A map only it
/// filled goes with it. A `PATH` the user wrote stays theirs.
fn follow_own_path(
    old: &Map<String, Value>,
    out: &mut Map<String, Value>,
    fresh: &Map<String, Value>,
) {
    let program = match old.get("command") {
        Some(Value::String(command)) => command.as_str(),
        Some(Value::Array(command)) => command.first().and_then(Value::as_str).unwrap_or_default(),
        _ => return,
    };
    // The bridge behind `sawit github token --exec`: the runner is what it starts.
    let args = old.get("args").and_then(Value::as_array);
    let program = args
        .filter(|args| args.first().is_some_and(|a| a == "github"))
        .and_then(|args| {
            let at = args.iter().position(|a| a == "--exec")?;
            args.get(at + 1)?.as_str()
        })
        .unwrap_or(program);
    for key in ["env", "environment"] {
        let Some(Value::Object(env)) = out.get_mut(key) else {
            continue;
        };
        if !env
            .get("PATH")
            .and_then(Value::as_str)
            .is_some_and(|path| own_path(path, program))
        {
            continue;
        }
        match fresh.get(key).and_then(|map| map.get("PATH")) {
            Some(path) => {
                env.insert("PATH".into(), path.clone());
            }
            None => {
                env.shift_remove("PATH");
                if env.is_empty() && !fresh.contains_key(key) {
                    out.shift_remove(key);
                }
            }
        }
    }
}

/// Codex's keys for a program, and for a URL: an entry switching kinds
/// drops the other kind's.
const CODEX_PROGRAM_KEYS: [&str; 5] = ["command", "args", "env", "env_vars", "cwd"];
const CODEX_URL_KEYS: [&str; 5] = [
    "url",
    "bearer_token_env_var",
    "http_headers",
    "env_http_headers",
    "http_headers_helper",
];

/// Lays `spec` over Codex's `[mcp_servers.<name>]` table in place, like
/// [`merge_entry`]: `command` and `args` (or `url`, and the header helper Sawit
/// writes) are replaced, keeping their comments; the user's other keys, their
/// order and comments stay. A `bearer_token_env_var` goes when Sawit hands over its
/// GitHub sign-in: Codex would send that token instead of the helper's.
pub(super) fn update_codex_table(table: &mut dyn toml_edit::TableLike, spec: &Spec) {
    let (other_kind, owned): (&[&str], &[&str]) = match spec.transport {
        Transport::Stdio { .. } => (&CODEX_URL_KEYS, &["command", "args"]),
        Transport::Http { .. } if spec.token_helper.is_some() => (
            &[
                "command",
                "args",
                "env",
                "env_vars",
                "cwd",
                "bearer_token_env_var",
            ],
            &["url", "http_headers_helper"],
        ),
        Transport::Http { .. } => (&CODEX_PROGRAM_KEYS, &["url"]),
    };
    for key in other_kind {
        table.remove(key);
    }
    let own_helper = table
        .get("http_headers_helper")
        .and_then(|i| i.as_str())
        .is_some_and(|h| h.contains(HELPER_MARK));
    if spec.token_helper.is_none() && own_helper {
        table.remove("http_headers_helper");
    }
    for (key, item) in codex_table(spec).iter() {
        if owned.contains(&key) {
            match (table.get_mut(key), item) {
                (Some(toml_edit::Item::Value(old)), toml_edit::Item::Value(new)) => {
                    let decor = old.decor().clone();
                    *old = new.clone();
                    *old.decor_mut() = decor;
                }
                _ => {
                    table.insert(key, item.clone());
                }
            }
        } else if let (Some(mine), Some(theirs)) = (
            table.get_mut(key).and_then(|i| i.as_table_like_mut()),
            item.as_table_like(),
        ) {
            for (k, v) in theirs.iter() {
                if !mine.contains_key(k) {
                    mine.insert(k, v.clone());
                }
            }
        } else if !table.contains_key(key) {
            table.insert(key, item.clone());
        }
    }
}
