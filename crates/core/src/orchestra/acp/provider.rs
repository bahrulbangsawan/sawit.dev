//! ACP agents as a provider: each agent's runtime is its own child process, started from a
//! program and its arguments (never through a shell), speaking ACP on stdin and stdout. Its
//! stderr is read all along (so it can't fill and stall the agent) and its last lines kept for
//! failure reports. The process runs in its own process group, so stopping it also ends what it
//! started (`npx` wrappers, language servers); dropping the runtime kills it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

use super::client::{AcpClient, ClientHandler};
use super::rpc::Rpc;
use crate::orchestra::model::Launch;
use crate::orchestra::provider::{
    AgentProvider, ProviderCapabilities, ProviderError, ProviderRuntime, ProviderSession,
    SessionRequest, StartRequest, TurnContext, TurnOutcome,
};

/// The provider id ACP agents are configured with.
pub const ACP: &str = "acp";

/// How long an agent gets to answer `initialize`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
/// How long a stopped agent gets to exit before it is killed.
const STOP_GRACE: Duration = Duration::from_secs(3);
/// Stderr lines kept for diagnostics.
const STDERR_LINES: usize = 40;

/// Known ACP agents, as `(preset, program and arguments, what it is)`. Claude Code and Codex speak
/// ACP through Zed's adapters (`@zed-industries/claude-code-acp`, `@zed-industries/codex-acp`).
pub fn presets() -> Vec<(&'static str, Launch, &'static str)> {
    let launch = |command: &str, args: &[&str]| Launch {
        command: command.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        env: Default::default(),
    };
    vec![
        ("opencode", launch("opencode", &["acp"]), "OpenCode"),
        (
            "gemini",
            launch("gemini", &["--experimental-acp"]),
            "Gemini CLI",
        ),
        (
            "claude-code",
            launch("claude-code-acp", &[]),
            "Claude Code (Zed's ACP adapter)",
        ),
        (
            "codex",
            launch("codex-acp", &[]),
            "Codex (Zed's ACP adapter)",
        ),
    ]
}

/// The program `command` names: an absolute path to an executable file, or a bare name found on
/// `PATH`. Anything else (a relative path, a shell line) is refused.
pub fn resolve_program(
    command: &str,
    path_var: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("no program is set".into());
    }
    let executable = |p: &Path| {
        use std::os::unix::fs::PermissionsExt;
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let as_path = Path::new(command);
    if as_path.is_absolute() {
        return if executable(as_path) {
            Ok(as_path.to_path_buf())
        } else {
            Err(format!("{command} isn't an executable file"))
        };
    }
    if command.contains('/') || command.chars().any(char::is_whitespace) {
        return Err(format!(
            "`{command}` must be a program name or an absolute path; put its arguments in args"
        ));
    }
    path_var
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .map(|dir| dir.join(command))
        .find(|p| executable(p))
        .ok_or_else(|| format!("`{command}` isn't installed (not found on PATH)"))
}

#[derive(Default)]
pub struct AcpProvider;

#[async_trait]
impl AgentProvider for AcpProvider {
    fn id(&self) -> &str {
        ACP
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            load_session: true,
            concurrent_turns: false,
            streaming: true,
            cancel: true,
            tool_calls: true,
            permission_requests: true,
            mcp_stdio: true,
            model_selection: false,
            image_prompts: false,
        }
    }

    async fn start(
        &self,
        request: StartRequest,
    ) -> Result<Arc<dyn ProviderRuntime>, ProviderError> {
        let profile = request.profile;
        let launch = profile
            .launch
            .clone()
            .ok_or_else(|| ProviderError::Start("the agent has no program to start".into()))?;
        let program = resolve_program(&launch.command, std::env::var_os("PATH").as_deref())
            .map_err(ProviderError::Start)?;
        if !profile.workdir.is_dir() {
            return Err(ProviderError::Start(format!(
                "{} isn't a folder",
                profile.workdir.display()
            )));
        }
        let mut command = Command::new(&program);
        command
            .args(&launch.args)
            .envs(&launch.env)
            .current_dir(&profile.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .process_group(0);
        let mut child = command
            .spawn()
            .map_err(|e| ProviderError::Start(format!("{}: {e}", program.display())))?;
        let pid = child.id();
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            return Err(ProviderError::Start(
                "the agent's pipes could not be opened".into(),
            ));
        };
        let tail: Arc<Mutex<VecDeque<String>>> = Arc::default();
        tokio::spawn(drain_stderr(stderr, tail.clone()));
        let (handler, turns) = ClientHandler::new();
        let rpc = Rpc::start(stdout, stdin, handler);
        let stop = CancellationToken::new();
        let exited = CancellationToken::new();
        tokio::spawn(watch(child, pid, rpc.clone(), stop.clone(), exited.clone()));
        let concurrent = profile
            .settings
            .get("concurrent_sessions")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let give_up = |why: String| {
            let stop = stop.clone();
            let exited = exited.clone();
            async move {
                stop.cancel();
                let _ = tokio::time::timeout(STOP_GRACE * 2, exited.cancelled()).await;
                Err(ProviderError::Start(why))
            }
        };
        let client = match tokio::time::timeout(CONNECT_TIMEOUT, AcpClient::connect(rpc, turns))
            .await
        {
            Ok(Ok(c)) => c,
            Ok(Err(e)) => return give_up(format!("{e}{}", tail_note(&tail))).await,
            Err(_) => return give_up("the agent didn't answer initialize in time".into()).await,
        };
        log::info!(target: "orchestra::acp", "agent {} connected (pid {pid:?}, load_session {})", profile.id, client.caps.load_session);
        Ok(Arc::new(AcpRuntime {
            client: Some(client),
            stop,
            exited,
            tail,
            concurrent,
            pid,
        }))
    }
}

async fn drain_stderr(stderr: tokio::process::ChildStderr, tail: Arc<Mutex<VecDeque<String>>>) {
    let mut lines = BufReader::new(stderr).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut t = tail.lock().unwrap_or_else(|p| p.into_inner());
        if t.len() == STDERR_LINES {
            t.pop_front();
        }
        t.push_back(line.chars().take(500).collect());
    }
}

/// Waits for the agent to exit, or stops it when asked: closes the connection (its stdin), lets
/// it go for [`STOP_GRACE`], then kills its process group.
async fn watch(
    mut child: tokio::process::Child,
    pid: Option<u32>,
    rpc: Arc<Rpc>,
    stop: CancellationToken,
    exited: CancellationToken,
) {
    tokio::select! {
        status = child.wait() => {
            let why = match status {
                Ok(s) => format!("the agent exited ({s})"),
                Err(e) => format!("the agent could not be waited for: {e}"),
            };
            rpc.close(&why);
        }
        _ = stop.cancelled() => {
            rpc.close("the agent was stopped");
            signal_group(pid, libc::SIGTERM);
            if tokio::time::timeout(STOP_GRACE, child.wait()).await.is_err() {
                signal_group(pid, libc::SIGKILL);
                let _ = child.kill().await;
            }
        }
    }
    // Whatever it started and left behind in its group goes too.
    signal_group(pid, libc::SIGKILL);
    exited.cancel();
}

fn signal_group(pid: Option<u32>, signal: libc::c_int) {
    if let Some(pid) = pid
        .and_then(|p| libc::pid_t::try_from(p).ok())
        .filter(|p| *p > 0)
    {
        // SAFETY: kill(2) with a negative pid signals the process group the child leads
        // (process_group(0) made it the leader); it touches no memory of ours.
        unsafe {
            libc::kill(-pid, signal);
        }
    }
}

fn tail_note(tail: &Mutex<VecDeque<String>>) -> String {
    let t = tail.lock().unwrap_or_else(|p| p.into_inner());
    if t.is_empty() {
        String::new()
    } else {
        format!(
            "\nagent stderr:\n{}",
            t.iter().cloned().collect::<Vec<_>>().join("\n")
        )
    }
}

pub struct AcpRuntime {
    client: Option<AcpClient>,
    stop: CancellationToken,
    exited: CancellationToken,
    tail: Arc<Mutex<VecDeque<String>>>,
    concurrent: bool,
    pid: Option<u32>,
}

impl AcpRuntime {
    fn client(&self) -> Result<&AcpClient, ProviderError> {
        self.client
            .as_ref()
            .ok_or_else(|| ProviderError::Disconnected("not connected".into()))
    }

    fn tail_note(&self) -> String {
        tail_note(&self.tail)
    }
}

#[async_trait]
impl ProviderRuntime for AcpRuntime {
    fn capabilities(&self) -> ProviderCapabilities {
        let caps = self
            .client
            .as_ref()
            .map(|c| c.caps.clone())
            .unwrap_or_default();
        ProviderCapabilities {
            load_session: caps.load_session,
            concurrent_turns: self.concurrent,
            image_prompts: caps.image,
            ..AcpProvider.capabilities()
        }
    }

    async fn open_session(
        &self,
        request: SessionRequest,
    ) -> Result<ProviderSession, ProviderError> {
        let client = self.client()?;
        if let Some(id) = &request.restore
            && client.caps.load_session
        {
            match client
                .load_session(id, &request.cwd, &request.mcp_servers)
                .await
            {
                Ok(()) => {
                    return Ok(ProviderSession {
                        id: id.clone(),
                        restored: true,
                    });
                }
                Err(ProviderError::Disconnected(e)) => return Err(ProviderError::Disconnected(e)),
                Err(e) => {
                    log::info!(target: "orchestra::acp", "session {id} could not be restored ({e}); starting a new one")
                }
            }
        }
        let id = client
            .new_session(&request.cwd, &request.mcp_servers)
            .await?;
        Ok(ProviderSession {
            id,
            restored: false,
        })
    }

    async fn prompt(
        &self,
        session: &ProviderSession,
        text: String,
        ctx: TurnContext,
    ) -> Result<TurnOutcome, ProviderError> {
        self.client()?
            .prompt(&session.id, text, ctx)
            .await
            .map_err(|e| match e {
                ProviderError::Disconnected(why) => {
                    ProviderError::Disconnected(format!("{why}{}", self.tail_note()))
                }
                other => other,
            })
    }

    async fn cancel(&self, session: &ProviderSession) -> Result<(), ProviderError> {
        self.client()?.cancel(&session.id).await
    }

    async fn close_session(&self, _session: ProviderSession) -> Result<(), ProviderError> {
        // ACP v1 has no method to close a session: it ends with the process.
        Ok(())
    }

    async fn shutdown(&self) {
        self.stop.cancel();
        let _ = tokio::time::timeout(STOP_GRACE * 2, self.exited.cancelled()).await;
    }

    fn is_alive(&self) -> bool {
        !self.exited.is_cancelled() && self.client.as_ref().is_some_and(|c| !c.is_closed())
    }

    fn process_id(&self) -> Option<u32> {
        self.pid
    }

    fn diagnostics(&self) -> String {
        let t = self.tail.lock().unwrap_or_else(|p| p.into_inner());
        t.iter().cloned().collect::<Vec<_>>().join("\n")
    }
}

impl Drop for AcpRuntime {
    fn drop(&mut self) {
        // The watcher kills the process group when this fires; kill_on_drop covers a watcher
        // that is gone with the runtime.
        if self.client.is_some() {
            self.stop.cancel();
        }
    }
}
