//! Sawit's MCP server in the AI clients: an install adds it, as the MCP page installs it, to each
//! client with no entry named sawit, and remembers each entry it added in Sawit's data folder
//! (`mcp-entries.json`); a removal takes out exactly those, as the MCP page removes a server, and
//! leaves every entry that was there before.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::agents::label_of;
use super::files::write_whole;
use super::report::{McpDone, McpOutcome};
use crate::dev_tools::find_program;
use crate::instances::AppPaths;
use crate::mcp_catalog::{self, DEFAULT_AWS_REGION};
use crate::mcp_clients::{self, Entries, SERVER_NAME, Spec, Status, Target};

/// An entry Sawit Skills added: the client it is in, and the `sawit-mcp` it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Added {
    pub client: String,
    pub server: PathBuf,
}

/// The entries Sawit Skills added, by the config they are in.
#[derive(Debug, Clone, Default)]
pub struct McpLedger {
    pub added: BTreeMap<PathBuf, Added>,
    /// `mcp-entries.json`.
    at: PathBuf,
}

#[derive(Serialize, Deserialize, Default)]
struct Stored {
    #[serde(default)]
    added: BTreeMap<PathBuf, Added>,
}

impl McpLedger {
    /// The list in Sawit's data folder `paths` (empty when there is none yet).
    pub fn load(paths: &AppPaths) -> Result<McpLedger, String> {
        let at = paths.root.join("mcp-entries.json");
        let added = match std::fs::read_to_string(&at) {
            Ok(text) => serde_json::from_str::<Stored>(&text)
                .map_err(|e| {
                    format!(
                        "{} can't be read ({e}): Sawit won't guess which MCP entries it added. Move it away to start over.",
                        at.display()
                    )
                })?
                .added,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(e) => return Err(format!("Couldn't read {}: {e}", at.display())),
        };
        Ok(McpLedger { added, at })
    }

    fn save(&self) -> Result<(), String> {
        let stored = Stored {
            added: self.added.clone(),
        };
        let text = serde_json::to_string_pretty(&stored).map_err(|e| e.to_string())?;
        if let Some(parent) = self.at.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Couldn't make {}: {e}", parent.display()))?;
        }
        write_whole(&self.at, &format!("{text}\n"))
    }
}

/// The MCP entries Sawit Skills added, which a removal takes out: each client's name and config.
pub fn added(paths: &AppPaths) -> Vec<(String, PathBuf)> {
    McpLedger::load(paths)
        .map(|l| {
            l.added
                .into_iter()
                .map(|(config, a)| (a.client, config))
                .collect()
        })
        .unwrap_or_default()
}

/// What the MCP page installs for Sawit's own server, run from `server`.
fn spec(server: &Path) -> Option<Spec> {
    let sawit = mcp_catalog::get(SERVER_NAME)?;
    Some(sawit.spec(server, DEFAULT_AWS_REGION, find_program))
}

fn outcome(target: &Target, result: Result<McpDone, String>) -> McpOutcome {
    McpOutcome {
        label: label_of(target),
        config: target.config.clone(),
        result,
    }
}

/// Adds Sawit's MCP server to each client that has no entry named sawit, and notes each one
/// added in `ledger`; an entry there already stays, wherever it points.
pub(super) async fn ensure(server: &Path, ledger: &mut McpLedger) -> Vec<McpOutcome> {
    let Some(spec) = spec(server) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for target in mcp_clients::targets() {
        let result = match mcp_clients::status(&target, &spec) {
            Status::Installed => Ok(McpDone::Present),
            Status::Outdated(to) => Ok(McpDone::Elsewhere(to)),
            Status::Manual => Ok(McpDone::ByHand),
            Status::Unreadable(e) => Err(format!("its config can't be read safely: {e}")),
            Status::NotInstalled if !server.is_file() => Err(format!(
                "{} isn't there to run, so no entry was written",
                server.display()
            )),
            Status::NotInstalled => {
                let cli = target.client.cli().and_then(find_program);
                match mcp_clients::install(&target, &spec, cli.as_deref()).await {
                    Ok(()) => {
                        let added = Added {
                            client: label_of(&target),
                            server: server.to_path_buf(),
                        };
                        ledger.added.insert(target.config.clone(), added);
                        ledger.save().map(|()| McpDone::Added)
                    }
                    Err(e) => Err(e.to_string()),
                }
            }
        };
        out.push(outcome(&target, result));
    }
    out
}

/// Takes out of each client the entry Sawit Skills added there (`ledger`), as the MCP page removes
/// a server, while it still runs the `sawit-mcp` added; any other entry named sawit stays, and the
/// result says why.
pub(super) async fn remove(ledger: &mut McpLedger) -> Vec<McpOutcome> {
    let mut out = Vec::new();
    for target in mcp_clients::targets() {
        let has_entry = matches!(
            mcp_clients::read_entries(&target),
            Entries::Servers(servers) if servers.contains_key(SERVER_NAME)
        );
        let Some(added) = ledger.added.get(&target.config).cloned() else {
            if has_entry {
                out.push(outcome(
                    &target,
                    Ok(McpDone::Kept("it was there before Install Skills".into())),
                ));
            }
            continue;
        };
        let Some(spec) = spec(&added.server) else {
            continue;
        };
        // What to say of it: `None` when it was gone already.
        let result = match mcp_clients::status(&target, &spec) {
            Status::Installed => {
                let cli = target.client.cli().and_then(find_program);
                match mcp_clients::uninstall(&target, SERVER_NAME, cli.as_deref()).await {
                    Ok(()) => Some(Ok(McpDone::Removed)),
                    Err(e) => {
                        out.push(outcome(&target, Err(e.to_string())));
                        continue;
                    }
                }
            }
            Status::NotInstalled => None,
            Status::Outdated(to) => Some(Ok(McpDone::Kept(format!(
                "it now runs {to}, not the sawit-mcp Install Skills added"
            )))),
            Status::Manual => continue,
            Status::Unreadable(e) => {
                let e = format!("its config can't be read safely: {e}");
                out.push(outcome(&target, Err(e)));
                continue;
            }
        };
        ledger.added.remove(&target.config);
        if let Err(e) = ledger.save() {
            out.push(outcome(&target, Err(e)));
            continue;
        }
        if let Some(result) = result {
            out.push(outcome(&target, result));
        }
    }
    out
}
