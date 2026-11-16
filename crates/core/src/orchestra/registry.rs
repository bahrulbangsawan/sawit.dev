//! The agent registry over the store: creating, changing and deleting agent profiles, checked
//! the same way wherever it happens (the app, or `sawit-mcp` while the app is closed).

use std::path::PathBuf;

use serde::Deserialize;

use super::acp::{self, resolve_program};
use super::ids::{AgentId, now_ms, slug};
use super::model::{AgentProfile, CommsPolicy, Isolation, Launch, PermissionMode};
use super::store::{Store, agents};
use super::{OrchestraError, Result};

/// The providers a profile may name. The app's registry can hold more (tests add their own);
/// these are the ones every process knows.
pub const KNOWN_PROVIDERS: &[&str] = &[acp::ACP];

/// Tasks one agent may be allowed to run at once, at most.
pub const MAX_CONCURRENCY: u32 = 16;

/// A new agent's settings. Unset fields take their defaults.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct NewAgent {
    /// A slug of the name when unset.
    pub id: Option<String>,
    pub name: String,
    pub role: String,
    pub description: String,
    /// `acp` when unset.
    pub provider: Option<String>,
    /// One of [`acp::presets`] (`opencode`, `gemini`, `claude-code`, `codex`), instead of `launch`.
    pub preset: Option<String>,
    pub launch: Option<Launch>,
    pub model: Option<String>,
    pub instructions: String,
    pub workdir: PathBuf,
    /// The workdir when unset.
    pub workspace: Option<PathBuf>,
    pub isolation: Isolation,
    pub permission: Option<PermissionMode>,
    pub capabilities: Vec<String>,
    pub comms: Option<CommsPolicy>,
    pub max_concurrent_tasks: Option<u32>,
    pub enabled: Option<bool>,
    pub settings: Option<serde_json::Value>,
}

impl NewAgent {
    /// The profile it makes, checked.
    pub fn into_profile(self, providers: &[String]) -> Result<AgentProfile> {
        let launch = match (&self.preset, self.launch) {
            (Some(p), _) => Some(
                acp::presets()
                    .into_iter()
                    .find(|(name, _, _)| name == p)
                    .map(|(_, l, _)| l)
                    .ok_or_else(|| OrchestraError::Invalid(format!("no preset `{p}`")))?,
            ),
            (None, l) => l,
        };
        let now = now_ms();
        let profile = AgentProfile {
            id: AgentId(
                self.id
                    .map(|i| slug(&i))
                    .unwrap_or_else(|| slug(&self.name)),
            ),
            name: self.name.trim().to_string(),
            role: self.role,
            description: self.description,
            provider: self.provider.unwrap_or_else(|| acp::ACP.into()),
            launch,
            model: self.model,
            instructions: self.instructions,
            workspace: self.workspace.unwrap_or_else(|| self.workdir.clone()),
            workdir: self.workdir,
            isolation: self.isolation,
            permission: self.permission.unwrap_or_default(),
            capabilities: self.capabilities,
            comms: self.comms.unwrap_or_default(),
            max_concurrent_tasks: self.max_concurrent_tasks.unwrap_or(1),
            enabled: self.enabled.unwrap_or(true),
            settings: self.settings.unwrap_or(serde_json::Value::Null),
            created_at: now,
            updated_at: now,
        };
        validate(&profile, providers)?;
        Ok(profile)
    }
}

/// Checks a profile before it is stored.
pub fn validate(p: &AgentProfile, providers: &[String]) -> Result<()> {
    let bad = |why: String| Err(OrchestraError::Invalid(why));
    if p.name.is_empty() || p.name.chars().count() > 80 {
        return bad("the name must be 1 to 80 characters".into());
    }
    if !providers.iter().any(|k| k == &p.provider) {
        return bad(format!(
            "no provider `{}`; there are {}",
            p.provider,
            providers.join(", ")
        ));
    }
    if !p.workdir.is_absolute() || !p.workdir.is_dir() {
        return bad(format!(
            "the working folder {} must be an absolute path to a folder",
            p.workdir.display()
        ));
    }
    if !p.workspace.is_absolute() {
        return bad("the workspace must be an absolute path".into());
    }
    if p.max_concurrent_tasks == 0 || p.max_concurrent_tasks > MAX_CONCURRENCY {
        return bad(format!(
            "max_concurrent_tasks must be 1 to {MAX_CONCURRENCY}"
        ));
    }
    if p.provider == acp::ACP {
        let Some(launch) = &p.launch else {
            return bad("an ACP agent needs a program to start (launch or preset)".into());
        };
        // A missing program is fine now (it may be installed later); a shell line never is.
        if let Err(e) = resolve_program(&launch.command, Some(std::ffi::OsStr::new("")))
            && !e.contains("isn't installed")
            && !e.contains("isn't an executable")
        {
            return bad(e);
        }
        if launch.env.keys().any(|k| k.is_empty() || k.contains('=')) {
            return bad("environment variable names can't be empty or hold `=`".into());
        }
    }
    Ok(())
}

pub fn create(store: &Store, new: NewAgent, providers: &[String]) -> Result<AgentProfile> {
    let profile = new.into_profile(providers)?;
    store.tx(|c| {
        agents::insert_agent(c, &profile)?;
        agents::main_thread(c, &profile.id)?;
        Ok(())
    })?;
    Ok(profile)
}

/// Changes the agent's profile with `change`; its id and creation time stay.
pub fn update(
    store: &Store,
    key: &str,
    providers: &[String],
    change: impl FnOnce(&mut AgentProfile),
) -> Result<AgentProfile> {
    store.tx(|c| {
        let mut p = agents::get_agent(c, key)?.profile;
        let (id, created) = (p.id.clone(), p.created_at);
        change(&mut p);
        p.id = id;
        p.created_at = created;
        p.updated_at = now_ms();
        validate(&p, providers)?;
        agents::update_agent(c, &p)?;
        Ok(p)
    })
}

pub fn delete(store: &Store, key: &str) -> Result<AgentId> {
    store.tx(|c| {
        let id = agents::get_agent(c, key)?.profile.id;
        agents::delete_agent(c, &id)?;
        Ok(id)
    })
}
