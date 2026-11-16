//! ACP: the client against the mock agent over an in-memory pipe (initialize, sessions,
//! streaming, tool calls, permissions, files, cancellation, disconnection, malformed input,
//! session loading), and the provider against the mock agent run as a real child process.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::future::BoxFuture;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::support::temp;
use crate::orchestra::acp::client::{AcpClient, ClientHandler};
use crate::orchestra::acp::rpc::{Incoming, Rpc, RpcError};
use crate::orchestra::acp::{AcpProvider, mock};
use crate::orchestra::model::{AgentProfile, Launch, PermissionMode};
use crate::orchestra::provider::*;
use crate::orchestra::registry::NewAgent;

struct NoTools;
impl ToolHost for NoTools {
    fn call(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> BoxFuture<'static, Result<serde_json::Value, String>> {
        Box::pin(async { Err("none".into()) })
    }
}

struct AllowGate;
impl PermissionGate for AllowGate {
    fn ask(&self, _: PermissionRequest) -> BoxFuture<'static, PermissionDecision> {
        Box::pin(async { PermissionDecision::Allow })
    }
}

async fn pair(load_session: bool) -> (Arc<Rpc>, AcpClient) {
    let (a, b) = tokio::io::duplex(1 << 16);
    let (ar, aw) = tokio::io::split(a);
    let (br, bw) = tokio::io::split(b);
    let agent = mock::serve(ar, aw, load_session, false);
    let (handler, turns) = ClientHandler::new();
    let client = AcpClient::connect(Rpc::start(br, bw, handler), turns)
        .await
        .expect("initialize");
    (agent, client)
}

fn ctx(
    root: &Path,
    mode: PermissionMode,
    gate: Arc<dyn PermissionGate>,
) -> (TurnContext, mpsc::UnboundedReceiver<ProviderEvent>) {
    let (tx, rx) = mpsc::unbounded_channel();
    let c = TurnContext {
        agent: "a".into(),
        task: "t".into(),
        thread: "h".into(),
        root: root.to_path_buf(),
        permission: mode,
        gate,
        tools: Arc::new(NoTools),
        events: tx,
        cancel: CancellationToken::new(),
    };
    (c, rx)
}

async fn say(
    client: &AcpClient,
    root: &Path,
    mode: PermissionMode,
    gate: Arc<dyn PermissionGate>,
    text: &str,
) -> String {
    let s = client.new_session(root, &[]).await.unwrap();
    let (c, _rx) = ctx(root, mode, gate);
    let out = client.prompt(&s, text.into(), c).await.unwrap();
    assert_eq!(out.stop, StopReason::EndTurn);
    out.text
}

#[tokio::test]
async fn initialize_negotiates_and_a_turn_streams_its_output_and_tool_calls() {
    let (_d, root) = temp();
    let (_agent, client) = pair(true).await;
    assert!(client.caps.load_session);
    assert_eq!(client.caps.agent_name.as_deref(), Some("mock-agent"));
    let (_agent2, plain) = pair(false).await;
    assert!(!plain.caps.load_session);
    let s = client.new_session(&root, &[]).await.unwrap();
    let (c, mut rx) = ctx(&root, PermissionMode::Allow, Arc::new(RefuseAll));
    let out = client
        .prompt(&s, "hello tool-call".into(), c)
        .await
        .unwrap();
    assert_eq!(out.text, "echo: hello tool-call done");
    let mut seen = Vec::new();
    while let Ok(e) = rx.try_recv() {
        seen.push(e);
    }
    assert!(seen.contains(&ProviderEvent::Message("echo: hello tool-call".into())));
    assert!(
        seen.iter()
            .any(|e| matches!(e, ProviderEvent::ToolCall { kind, .. } if kind == "read"))
    );
    assert!(seen.iter().any(
        |e| matches!(e, ProviderEvent::ToolCallUpdate { status: Some(s), .. } if s == "completed")
    ));
}

#[tokio::test]
async fn permission_requests_follow_the_task_mode_and_the_gate() {
    let (_d, root) = temp();
    let (_agent, client) = pair(false).await;
    let refuse: Arc<dyn PermissionGate> = Arc::new(RefuseAll);
    for (mode, gate, want) in [
        (PermissionMode::Allow, refuse.clone(), "permission=yes"),
        (PermissionMode::Deny, refuse.clone(), "permission=no"),
        (PermissionMode::ReadOnly, refuse.clone(), "permission=no"),
        (PermissionMode::Ask, refuse.clone(), "permission=no"),
        (
            PermissionMode::Ask,
            Arc::new(AllowGate) as Arc<dyn PermissionGate>,
            "permission=yes",
        ),
    ] {
        let text = say(&client, &root, mode, gate, "ask-permission").await;
        assert!(text.contains(want), "{mode:?}: {text}");
    }
}

#[tokio::test]
async fn files_are_read_and_written_only_inside_the_agents_folder_and_as_allowed() {
    let (_d, root) = temp();
    let (_agent, client) = pair(false).await;
    let inside = root.join("notes.txt");
    std::fs::write(&inside, "secret plan").unwrap();
    let refuse: Arc<dyn PermissionGate> = Arc::new(RefuseAll);
    let read = say(
        &client,
        &root,
        PermissionMode::ReadOnly,
        refuse.clone(),
        &format!("read-file:{}", inside.display()),
    )
    .await;
    assert!(read.contains("content=secret plan"), "{read}");
    let (_e, elsewhere) = temp();
    let outside = elsewhere.join("x.txt");
    std::fs::write(&outside, "nope").unwrap();
    let escaped = say(
        &client,
        &root,
        PermissionMode::Allow,
        refuse.clone(),
        &format!("read-file:{}", outside.display()),
    )
    .await;
    assert!(
        escaped.contains("read-error=") && escaped.contains("outside"),
        "{escaped}"
    );
    let target = root.join("out.txt");
    let denied = say(
        &client,
        &root,
        PermissionMode::ReadOnly,
        refuse.clone(),
        &format!("write-file:{}", target.display()),
    )
    .await;
    assert!(denied.contains("write-error="), "{denied}");
    assert!(!target.exists());
    let written = say(
        &client,
        &root,
        PermissionMode::Allow,
        refuse,
        &format!("write-file:{}", target.display()),
    )
    .await;
    assert!(written.contains("write=ok"), "{written}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "written by mock");
}

#[tokio::test]
async fn a_cancelled_turn_ends_cancelled() {
    let (_d, root) = temp();
    let (_agent, client) = pair(false).await;
    let s = client.new_session(&root, &[]).await.unwrap();
    let (c, _rx) = ctx(&root, PermissionMode::Allow, Arc::new(RefuseAll));
    let cancel = c.cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        cancel.cancel();
    });
    let out = tokio::time::timeout(Duration::from_secs(5), client.prompt(&s, "slow".into(), c))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(out.stop, StopReason::Cancelled);
}

#[tokio::test]
async fn a_lost_agent_fails_the_turn_and_every_call_after_it() {
    let (_d, root) = temp();
    let (_agent, client) = pair(false).await;
    let s = client.new_session(&root, &[]).await.unwrap();
    let (c, _rx) = ctx(&root, PermissionMode::Allow, Arc::new(RefuseAll));
    let err = client.prompt(&s, "crash".into(), c).await.unwrap_err();
    assert!(matches!(err, ProviderError::Disconnected(_)), "{err:?}");
    assert!(client.is_closed());
    assert!(matches!(
        client.new_session(&root, &[]).await,
        Err(ProviderError::Disconnected(_))
    ));
}

#[tokio::test]
async fn sessions_load_when_the_agent_can_and_replayed_history_is_not_a_turns_output() {
    let (_d, root) = temp();
    let (_agent, client) = pair(true).await;
    client.load_session("mock-7", &root, &[]).await.unwrap();
    assert!(matches!(
        client.load_session("other", &root, &[]).await,
        Err(ProviderError::Protocol(_))
    ));
    let (c, _rx) = ctx(&root, PermissionMode::Allow, Arc::new(RefuseAll));
    let out = client.prompt("mock-7", "next".into(), c).await.unwrap();
    assert!(!out.text.contains("replayed"), "{}", out.text);
    let (_agent2, plain) = pair(false).await;
    assert!(matches!(
        plain.load_session("mock-1", &root, &[]).await,
        Err(ProviderError::Unsupported(_))
    ));
}

#[tokio::test]
async fn lines_that_are_not_json_rpc_are_skipped_and_unknown_methods_refused() {
    let (a, b) = tokio::io::duplex(1 << 16);
    let (ar, mut aw) = tokio::io::split(a);
    tokio::spawn(async move {
        let mut lines = BufReader::new(ar).lines();
        let first = lines.next_line().await.unwrap().unwrap();
        let req: serde_json::Value = serde_json::from_str(&first).unwrap();
        let answer = serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": {"protocolVersion": 1, "agentCapabilities": {}}});
        aw.write_all(format!("not json at all\n{{\"jsonrpc\":\"2.0\"}}\n\n{answer}\n").as_bytes())
            .await
            .unwrap();
        while lines.next_line().await.ok().flatten().is_some() {}
    });
    let (br, bw) = tokio::io::split(b);
    let (handler, turns) = ClientHandler::new();
    let rpc = Rpc::start(br, bw, handler.clone());
    let client = AcpClient::connect(rpc.clone(), turns).await.unwrap();
    assert!(!client.caps.load_session);
    assert_eq!(rpc.malformed(), 2);
    let unknown = handler
        .request(
            "terminal/create".into(),
            serde_json::json!({"sessionId": "x"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.code, RpcError::METHOD_NOT_FOUND);
    let no_turn = handler
        .request(
            "fs/read_text_file".into(),
            serde_json::json!({"sessionId": "x", "path": "/tmp"}),
        )
        .await
        .unwrap_err();
    assert_eq!(no_turn.code, RpcError::INVALID_REQUEST);
}

/// The mock agent as a child process: this test binary, run again for this one ignored test.
#[test]
#[ignore = "run by the process tests as the mock agent"]
fn mock_agent_process() {
    if std::env::var_os("SAWIT_MOCK_ACP").is_none() {
        return;
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let rpc = mock::serve(tokio::io::stdin(), tokio::io::stdout(), true, true);
        rpc.closed().cancelled().await;
    });
    std::process::exit(0);
}

fn process_profile(dir: &Path) -> AgentProfile {
    let exe = std::env::current_exe().unwrap();
    let launch = Launch {
        command: exe.to_string_lossy().into_owned(),
        args: [
            "orchestra::tests::acp_tests::mock_agent_process",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
            "-q",
        ]
        .map(String::from)
        .to_vec(),
        env: [("SAWIT_MOCK_ACP".to_string(), "1".to_string())].into(),
    };
    NewAgent {
        name: "proc".into(),
        workdir: dir.to_path_buf(),
        launch: Some(launch),
        ..Default::default()
    }
    .into_profile(&["acp".into()])
    .unwrap()
}

fn alive(pid: i32) -> bool {
    // SAFETY: signal 0 only checks that the process exists.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_provider_runs_an_acp_agent_process_and_leaves_nothing_behind() {
    let (_d, dir) = temp();
    let rt = AcpProvider
        .start(StartRequest {
            profile: process_profile(&dir),
        })
        .await
        .expect("the mock agent starts");
    assert!(rt.is_alive());
    assert!(rt.capabilities().load_session);
    let s = rt
        .open_session(SessionRequest {
            thread: "h".into(),
            cwd: dir.clone(),
            restore: None,
            mcp_servers: vec![],
        })
        .await
        .unwrap();
    let (c, _rx) = ctx(&dir, PermissionMode::Allow, Arc::new(RefuseAll));
    let out = rt.prompt(&s, "hi".into(), c).await.unwrap();
    assert_eq!(out.text, "echo: hi done");
    let restored = rt
        .open_session(SessionRequest {
            thread: "h".into(),
            cwd: dir.clone(),
            restore: Some(s.id.clone()),
            mcp_servers: vec![],
        })
        .await
        .unwrap();
    assert!(restored.restored);
    let pid = rt.process_id().map(|p| p as i32);
    rt.shutdown().await;
    assert!(!rt.is_alive());
    if let Some(pid) = pid {
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!alive(pid), "the agent process {pid} is left behind");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_crashing_agent_process_is_noticed() {
    let (_d, dir) = temp();
    let rt = AcpProvider
        .start(StartRequest {
            profile: process_profile(&dir),
        })
        .await
        .unwrap();
    let s = rt
        .open_session(SessionRequest {
            thread: "h".into(),
            cwd: dir.clone(),
            restore: None,
            mcp_servers: vec![],
        })
        .await
        .unwrap();
    let (c, _rx) = ctx(&dir, PermissionMode::Allow, Arc::new(RefuseAll));
    let err = rt.prompt(&s, "crash".into(), c).await.unwrap_err();
    assert!(matches!(err, ProviderError::Disconnected(_)), "{err:?}");
    super::support::until("the exit to be seen", async || !rt.is_alive()).await;
}

#[tokio::test]
async fn a_program_that_is_not_installed_fails_to_start_cleanly() {
    let (_d, dir) = temp();
    let mut p = process_profile(&dir);
    p.launch = Some(Launch {
        command: "sawit-no-such-agent".into(),
        ..Default::default()
    });
    let err = AcpProvider
        .start(StartRequest { profile: p })
        .await
        .err()
        .unwrap();
    assert!(
        matches!(err, ProviderError::Start(ref m) if m.contains("isn't installed")),
        "{err:?}"
    );
}
