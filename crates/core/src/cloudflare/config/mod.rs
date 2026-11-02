//! What a Worker's config file declares that Sawit shows: its name, bindings and Cron Triggers,
//! from a Wrangler config (JSON, JSONC or TOML) or a `cloudflare.config.ts`.

mod typescript;

use std::path::Path;

use serde_json::Value;

use super::{Binding, BindingKind};
use crate::scripts::read_json;

/// What a config file says that Sawit shows.
#[derive(Debug, Default)]
pub(super) struct Config {
    pub(super) name: Option<String>,
    pub(super) bindings: Vec<Binding>,
    pub(super) crons: bool,
}

pub(super) fn read_config(path: &Path) -> Config {
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match file {
        "wrangler.json" | "wrangler.jsonc" => read_json(path)
            .map(|v| from_wrangler(&v))
            .unwrap_or_default(),
        "wrangler.toml" => std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str::<toml::Table>(&text).ok())
            .and_then(|table| serde_json::to_value(table).ok())
            .map(|v| from_wrangler(&v))
            .unwrap_or_default(),
        "cloudflare.config.ts" => std::fs::read_to_string(path)
            .map(|text| typescript::read(&text))
            .unwrap_or_default(),
        _ => Config::default(),
    }
}

/// One binding list of a Wrangler config: where it lives, its kind, and which keys of an entry
/// hold the binding's name, the resource's name and its id.
struct BindingList {
    at: &'static str,
    tag: &'static str,
    binding: &'static str,
    name: Option<&'static str>,
    id: Option<&'static str>,
}

const fn list(
    at: &'static str,
    tag: &'static str,
    binding: &'static str,
    name: Option<&'static str>,
    id: Option<&'static str>,
) -> BindingList {
    BindingList {
        at,
        tag,
        binding,
        name,
        id,
    }
}

/// The binding lists Sawit reads, D1 first.
const WRANGLER_BINDINGS: &[BindingList] = &[
    list(
        "/d1_databases",
        "d1",
        "binding",
        Some("database_name"),
        Some("database_id"),
    ),
    list("/kv_namespaces", "kv", "binding", None, Some("id")),
    list("/r2_buckets", "r2", "binding", Some("bucket_name"), None),
    list(
        "/durable_objects/bindings",
        "durable_object",
        "name",
        Some("class_name"),
        None,
    ),
    list("/workflows", "workflow", "binding", Some("name"), None),
    list(
        "/send_email",
        "send_email",
        "name",
        Some("destination_address"),
        None,
    ),
    list("/services", "service", "binding", Some("service"), None),
    list("/queues/producers", "queue", "binding", Some("queue"), None),
    list("/hyperdrive", "hyperdrive", "binding", None, Some("id")),
    list(
        "/vectorize",
        "vectorize",
        "binding",
        Some("index_name"),
        None,
    ),
    list(
        "/analytics_engine_datasets",
        "analytics_engine",
        "binding",
        Some("dataset"),
        None,
    ),
];

/// The bindings that are one object, not a list.
const WRANGLER_SINGLE: &[(&str, &str)] = &[
    ("/ai", "ai"),
    ("/browser", "browser"),
    ("/images", "images"),
];

/// A Wrangler config (JSON, or TOML read as JSON): its top level, which is what `wrangler dev`
/// uses without `--env`.
fn from_wrangler(config: &Value) -> Config {
    let text = |o: &Value, key: &str| o.get(key).and_then(Value::as_str).map(str::to_string);
    let mut bindings = Vec::new();
    for list in WRANGLER_BINDINGS {
        for item in config
            .pointer(list.at)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(binding) = text(item, list.binding) {
                bindings.push(Binding {
                    kind: BindingKind::from_tag(list.tag),
                    binding,
                    name: list.name.and_then(|k| text(item, k)),
                    id: list.id.and_then(|k| text(item, k)),
                    preview_id: (list.tag == "d1")
                        .then(|| text(item, "preview_database_id"))
                        .flatten(),
                });
            }
        }
    }
    for &(at, tag) in WRANGLER_SINGLE {
        if let Some(binding) = config.pointer(at).and_then(|o| text(o, "binding")) {
            bindings.push(Binding {
                kind: BindingKind::from_tag(tag),
                binding,
                name: None,
                id: None,
                preview_id: None,
            });
        }
    }
    Config {
        name: text(config, "name"),
        bindings,
        crons: config
            .pointer("/triggers/crons")
            .and_then(Value::as_array)
            .is_some_and(|crons| !crons.is_empty()),
    }
}
