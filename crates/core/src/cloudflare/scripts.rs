//! The `package.json` scripts that start a Worker's dev server, and the `--persist-to` folder
//! they keep local data in.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{DevTool, WorkersProject};
use crate::scripts::read_json;

/// A `package.json` script that starts the Worker's dev server, and so Local Explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevScript {
    pub name: String,
    /// What it runs: `wrangler dev --port 8787`.
    pub body: String,
}

/// The scripts of `project`'s `package.json` that start its dev server: `dev` first, then
/// `start`, then the others in the file's order.
pub fn dev_scripts(project: &WorkersProject) -> Vec<DevScript> {
    let manifest = read_json(&project.root.join("package.json")).unwrap_or(Value::Null);
    let mut found: Vec<DevScript> = scripts_of(&manifest)
        .into_iter()
        .filter(|(_, body)| starts_dev_server(body, project.tool))
        .map(|(name, body)| DevScript { name, body })
        .collect();
    found.sort_by_key(|s| match s.name.as_str() {
        "dev" => 0,
        "start" => 1,
        _ => 2,
    });
    found
}

fn scripts_of(manifest: &Value) -> Vec<(String, String)> {
    manifest
        .get("scripts")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the command line `body` starts a Workers dev server: `wrangler dev`, `cf dev`, or
/// `vite` serving (not `vite build` or `vite preview`); with the Vite plugin also a framework's
/// own dev command that runs Vite (`react-router dev`, `astro dev`, `vinext dev`). Each command of
/// a `&&`, `;` or `|` chain counts, quoted ones (`concurrently "wrangler dev"`) too.
pub fn starts_dev_server(body: &str, tool: DevTool) -> bool {
    body.split(['&', '|', ';']).any(|segment| {
        let words: Vec<&str> = segment
            .split_whitespace()
            .map(|w| w.trim_matches(|c| c == '"' || c == '\''))
            .filter(|w| !w.is_empty())
            .map(|w| w.rsplit('/').next().unwrap_or(w))
            .collect();
        words.iter().enumerate().any(|(ix, word)| {
            let next = words.get(ix + 1).copied();
            match *word {
                "wrangler" | "cf" => next == Some("dev"),
                "vite" => next.is_none_or(|n| n == "dev" || n == "serve" || n.starts_with('-')),
                "react-router" | "astro" | "vinext" => {
                    tool == DevTool::VitePlugin && next == Some("dev")
                }
                _ => false,
            }
        })
    })
}

/// The `--persist-to` folder of the first dev script that gives one, else of any script.
pub(super) fn persist_to(dir: &Path, manifest: &Value, tool: DevTool) -> Option<PathBuf> {
    let mut scripts = scripts_of(manifest);
    scripts.sort_by_key(|(_, body)| !starts_dev_server(body, tool));
    scripts.iter().find_map(|(_, body)| {
        let words: Vec<&str> = body.split_whitespace().collect();
        let value =
            words
                .iter()
                .enumerate()
                .find_map(|(ix, w)| match w.strip_prefix("--persist-to") {
                    Some(rest) if rest.starts_with('=') => Some(rest[1..].to_string()),
                    Some("") => words.get(ix + 1).map(|v| v.to_string()),
                    _ => None,
                })?;
        let value = value.trim_matches(|c| c == '"' || c == '\'');
        (!value.is_empty()).then(|| dir.join(value))
    })
}
