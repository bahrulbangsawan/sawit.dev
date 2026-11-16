//! The agent runtime's tools (`sawit_core::orchestra`). They work on the runtime's store, with
//! the app open or not; the app's scheduler runs what they queue.
//!
//! Who calls: when the runtime starts an agent's session it gives this server the agent, the
//! thread and the thread's secret in the environment ([`exec::ENV_AGENT`] and the others), and the
//! tools then act as that agent, in the task its thread is running, within that agent's rights.
//! Without them the caller is the user (a client the user runs), who may do anything: hand an
//! agent work with `delegate_task`, message it, and manage agents with `manage_runtime_agent`.

use std::path::PathBuf;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::orchestra::exec::{ENV_AGENT, ENV_THREAD, ENV_TOKEN};
use sawit_core::orchestra::registry::{self, KNOWN_PROVIDERS, NewAgent};
use sawit_core::orchestra::store::{self, Store, agents};
use sawit_core::orchestra::{Caller, Comms, Isolation, Launch, Limits, PermissionMode};
use serde_json::{Value, json};

use crate::{Sawit, blocking, fail, ok, tri};

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct NoArgs {}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SendArgs {
    #[schemars(description = "The agent's id or name (see list_agents)")]
    pub recipient: String,
    pub content: String,
    #[schemars(
        description = "The id of the message this answers, to keep the conversation together"
    )]
    pub correlation_id: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct DelegateArgs {
    #[schemars(description = "The agent's id or name (see list_agents)")]
    pub recipient: String,
    pub title: String,
    pub instructions: String,
    #[schemars(description = "The task you are working on; your current task when left out")]
    pub parent_task_id: Option<String>,
    #[schemars(description = "Repeat the call with the same key and no second task is made")]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct TaskArgs {
    pub task_id: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListTasksArgs {
    #[schemars(description = "Whose tasks: an agent's id or name; your own when left out")]
    pub agent: Option<String>,
    #[schemars(
        description = "queued, held, starting, running, waiting, completed, failed, cancelled or interrupted"
    )]
    pub status: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ManageArgs {
    #[schemars(description = "list, show, create, update, enable, disable or delete")]
    pub action: String,
    #[schemars(description = "The agent's id or name (all but list and create)")]
    pub agent: Option<String>,
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    #[schemars(
        description = "An ACP agent: opencode, gemini, claude-code or codex; or give command and args"
    )]
    pub preset: Option<String>,
    #[schemars(
        description = "The agent's program: a name on PATH or an absolute path, never a shell line"
    )]
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub instructions: Option<String>,
    #[schemars(description = "The absolute folder it works in")]
    pub workdir: Option<String>,
    #[schemars(description = "The project it belongs to (the workdir when left out)")]
    pub workspace: Option<String>,
    #[schemars(description = "deny, read_only, ask or allow")]
    pub permission: Option<String>,
    #[schemars(
        description = "shared (take turns in the folder), worktree (a git worktree per thread) or free"
    )]
    pub isolation: Option<String>,
    pub capabilities: Option<Vec<String>>,
    pub max_concurrent_tasks: Option<u32>,
    #[schemars(description = "Agents it may contact (\"*\": every agent)")]
    pub may_contact: Option<Vec<String>>,
    #[schemars(description = "Agents it takes messages and tasks from (\"*\": every agent)")]
    pub accepts_from: Option<Vec<String>>,
    pub may_delegate: Option<bool>,
    pub discoverable: Option<bool>,
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

impl Sawit {
    fn orchestra_store(&self) -> Result<Store, String> {
        Store::open(&store::default_path(&self.paths.root)).map_err(|e| e.to_string())
    }

    /// The caller: the runtime agent this server was started for, or the user.
    fn orchestra_caller(comms: &Comms) -> Result<Caller, String> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
        match (var(ENV_AGENT), var(ENV_THREAD), var(ENV_TOKEN)) {
            (Some(a), Some(t), Some(k)) => comms
                .caller_for_thread(&a.into(), &t.into(), &k)
                .map_err(|e| e.to_string()),
            _ => Ok(Caller::user()),
        }
    }

    async fn comms_call(&self, name: &'static str, args: Value) -> CallToolResult {
        let store = tri!(self.orchestra_store());
        let out = tri!(
            blocking(move || {
                let comms = Comms::new(store, Limits::default());
                let caller = Self::orchestra_caller(&comms)?;
                if caller.agent.is_none()
                    && name == "delegate_task"
                    && args.get("parent_task_id").is_none()
                {
                    // The user hands an agent new work: a root task.
                    let req = sawit_core::orchestra::SubmitTask {
                        agent: args["recipient"].as_str().unwrap_or_default().to_string(),
                        text: args["instructions"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        title: args["title"].as_str().map(str::to_string),
                        new_thread: true,
                        idempotency_key: args["idempotency_key"].as_str().map(str::to_string),
                        ..Default::default()
                    };
                    return comms
                        .submit_task(&req)
                        .map(|v| json!(v))
                        .map_err(|e| e.to_string());
                }
                comms.call(&caller, name, &args).map_err(|e| e.to_string())
            })
            .await
        );
        match out {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }

    async fn manage_tool(&self, a: ManageArgs) -> CallToolResult {
        let store = tri!(self.orchestra_store());
        let out = tri!(blocking(move || manage(&store, a)).await);
        match out {
            Ok(v) => ok(pretty(&v)),
            Err(e) => fail(e),
        }
    }
}

fn parse_mode(s: &str) -> Result<PermissionMode, String> {
    PermissionMode::parse(s)
        .ok_or_else(|| format!("permission `{s}` isn't one: deny, read_only, ask or allow"))
}

fn parse_isolation(s: &str) -> Result<Isolation, String> {
    serde_json::from_value(json!(s))
        .map_err(|_| format!("isolation `{s}` isn't one: shared, worktree or free"))
}

fn manage(store: &Store, a: ManageArgs) -> Result<Value, String> {
    let providers: Vec<String> = KNOWN_PROVIDERS.iter().map(|p| p.to_string()).collect();
    let key = || a.agent.clone().ok_or("`agent` is required".to_string());
    let e = |e: sawit_core::orchestra::OrchestraError| e.to_string();
    let launch = match (&a.command, &a.args) {
        (Some(c), args) => Some(Launch {
            command: c.clone(),
            args: args.clone().unwrap_or_default(),
            env: Default::default(),
        }),
        (None, _) => None,
    };
    match a.action.as_str() {
        "list" => {
            let rows = store.with(agents::list_agents).map_err(e)?;
            Ok(json!({"agents": rows.iter().map(|r| json!({
                "id": r.profile.id, "name": r.profile.name, "role": r.profile.role,
                "provider": r.profile.provider, "enabled": r.profile.enabled,
                "state": r.state, "detail": r.state_detail, "workdir": r.profile.workdir,
            })).collect::<Vec<_>>()}))
        }
        "show" => {
            let k = key()?;
            let r = store.with(|c| agents::get_agent(c, &k)).map_err(e)?;
            Ok(json!({"profile": r.profile, "state": r.state, "detail": r.state_detail}))
        }
        "create" => {
            let mut comms = sawit_core::orchestra::CommsPolicy::default();
            if let Some(v) = a.may_contact.clone() {
                comms.may_contact = v
            }
            if let Some(v) = a.accepts_from.clone() {
                comms.accepts_from = v
            }
            if let Some(v) = a.may_delegate {
                comms.may_delegate = v
            }
            if let Some(v) = a.discoverable {
                comms.discoverable = v
            }
            let new = NewAgent {
                id: None,
                name: a.name.clone().ok_or("`name` is required")?,
                role: a.role.clone().unwrap_or_default(),
                description: a.description.clone().unwrap_or_default(),
                provider: None,
                preset: a.preset.clone(),
                launch,
                model: None,
                instructions: a.instructions.clone().unwrap_or_default(),
                workdir: PathBuf::from(a.workdir.clone().ok_or("`workdir` is required")?),
                workspace: a.workspace.clone().map(PathBuf::from),
                isolation: a
                    .isolation
                    .as_deref()
                    .map(parse_isolation)
                    .transpose()?
                    .unwrap_or_default(),
                permission: a.permission.as_deref().map(parse_mode).transpose()?,
                capabilities: a.capabilities.clone().unwrap_or_default(),
                comms: Some(comms),
                max_concurrent_tasks: a.max_concurrent_tasks,
                enabled: Some(true),
                settings: None,
            };
            let p = registry::create(store, new, &providers).map_err(e)?;
            Ok(json!({"created": p}))
        }
        "update" | "enable" | "disable" => {
            let k = key()?;
            let mode = a.permission.as_deref().map(parse_mode).transpose()?;
            let isolation = a.isolation.as_deref().map(parse_isolation).transpose()?;
            let preset = match &a.preset {
                Some(p) => Some(
                    sawit_core::orchestra::acp::presets()
                        .into_iter()
                        .find(|(n, _, _)| n == p)
                        .map(|(_, l, _)| l)
                        .ok_or(format!("no preset `{p}`"))?,
                ),
                None => None,
            };
            let action = a.action.clone();
            let p = registry::update(store, &k, &providers, move |p| {
                match action.as_str() {
                    "enable" => p.enabled = true,
                    "disable" => p.enabled = false,
                    _ => {}
                }
                if let Some(v) = a.name {
                    p.name = v
                }
                if let Some(v) = a.role {
                    p.role = v
                }
                if let Some(v) = a.description {
                    p.description = v
                }
                if let Some(v) = a.instructions {
                    p.instructions = v
                }
                if let Some(v) = a.workdir {
                    p.workdir = v.into()
                }
                if let Some(v) = a.workspace {
                    p.workspace = v.into()
                }
                if let Some(v) = mode {
                    p.permission = v
                }
                if let Some(v) = isolation {
                    p.isolation = v
                }
                if let Some(v) = a.capabilities {
                    p.capabilities = v
                }
                if let Some(v) = a.max_concurrent_tasks {
                    p.max_concurrent_tasks = v
                }
                if let Some(v) = a.may_contact {
                    p.comms.may_contact = v
                }
                if let Some(v) = a.accepts_from {
                    p.comms.accepts_from = v
                }
                if let Some(v) = a.may_delegate {
                    p.comms.may_delegate = v
                }
                if let Some(v) = a.discoverable {
                    p.comms.discoverable = v
                }
                if let Some(l) = preset.or(launch) {
                    p.launch = Some(l)
                }
            })
            .map_err(e)?;
            Ok(json!({"updated": p}))
        }
        "delete" => {
            let id = registry::delete(store, &key()?).map_err(e)?;
            Ok(json!({"deleted": id}))
        }
        other => Err(format!(
            "action `{other}` isn't one: list, show, create, update, enable, disable or delete"
        )),
    }
}

#[tool_router(router = orchestra_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "Agent runtime: the agents you may work with (id, name, role, capabilities, state, and whether you may message or delegate to each)"
    )]
    async fn list_agents(&self, Parameters(_a): Parameters<NoArgs>) -> CallToolResult {
        self.comms_call("list_agents", json!({})).await
    }

    #[tool(
        description = "Agent runtime: send another agent a message. It is stored and delivered as a task of theirs; a reply comes back in the same conversation"
    )]
    async fn send_message(&self, Parameters(a): Parameters<SendArgs>) -> CallToolResult {
        self.comms_call("send_message", json!({"recipient": a.recipient, "content": a.content, "correlation_id": a.correlation_id})).await
    }

    #[tool(
        description = "Agent runtime: hand another agent a task. It runs on its own; its result comes back to the task that asked. Without an agent identity this gives the agent new work for the user"
    )]
    async fn delegate_task(&self, Parameters(a): Parameters<DelegateArgs>) -> CallToolResult {
        let mut args = json!({"recipient": a.recipient, "title": a.title, "instructions": a.instructions, "idempotency_key": a.idempotency_key});
        if let Some(p) = a.parent_task_id {
            args["parent_task_id"] = json!(p);
        }
        self.comms_call("delegate_task", args).await
    }

    #[tool(description = "Agent runtime: a task's status, result or error")]
    async fn get_task_status(&self, Parameters(a): Parameters<TaskArgs>) -> CallToolResult {
        self.comms_call("get_task_status", json!({"task_id": a.task_id}))
            .await
    }

    #[tool(
        description = "Agent runtime: an agent's tasks, newest first (another agent's: only those you asked for)"
    )]
    async fn list_agent_tasks(&self, Parameters(a): Parameters<ListTasksArgs>) -> CallToolResult {
        self.comms_call(
            "list_agent_tasks",
            json!({"agent": a.agent, "status": a.status}),
        )
        .await
    }

    #[tool(description = "Agent runtime: cancel a task and everything delegated from it")]
    async fn cancel_task(&self, Parameters(a): Parameters<TaskArgs>) -> CallToolResult {
        self.comms_call("cancel_task", json!({"task_id": a.task_id}))
            .await
    }

    #[tool(
        description = "Agent runtime: list, show, create, update, enable, disable or delete the agents it runs (ACP agents: OpenCode, Gemini CLI, Claude Code, Codex)"
    )]
    async fn manage_runtime_agent(&self, Parameters(a): Parameters<ManageArgs>) -> CallToolResult {
        self.manage_tool(a).await
    }
}
