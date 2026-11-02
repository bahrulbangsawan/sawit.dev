//! Claude Code's and Codex's own CLIs: the calls that add and remove a server, with the
//! profile and `PATH` they run in, and running them.

use std::path::{Path, PathBuf};

use super::{Client, Spec, Target, Transport, entry};
use crate::{CoreError, Result};

/// A client CLI call, with the environment it must run in (`None` value =
/// remove the variable).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCall {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, Option<PathBuf>)>,
}

pub(crate) fn cli_call(target: &Target, cli: &Path, args: Vec<String>) -> Option<CliCall> {
    let node = crate::dev_tools::find_program("node");
    let path = std::env::var_os("PATH");
    cli_call_with(target, cli, args, node.as_deref(), path.as_deref())
}

/// [`cli_call`], given the `node` found and the app's `path`. A CLI
/// installed with npm (`#!/usr/bin/env node`) runs with its own folder on
/// `PATH`, where nvm keeps the node beside its global packages, then the
/// folder of the node found (nvm's default, say), for one installed
/// elsewhere: Claude Code's old `~/.claude/local`, `~/.npm-global/bin`.
pub(super) fn cli_call_with(
    target: &Target,
    cli: &Path,
    args: Vec<String>,
    node: Option<&Path>,
    path: Option<&std::ffi::OsStr>,
) -> Option<CliCall> {
    let var = target.client.profile_var()?;
    let mut env = vec![(var.to_string(), target.profile_dir.clone())];
    if let Some(path) = path_with(&[cli.parent(), node.and_then(Path::parent)], path) {
        env.push(("PATH".into(), Some(path)));
    }
    Some(CliCall {
        program: cli.to_path_buf(),
        args,
        env,
    })
}

/// `path` with `dirs` in front, those it doesn't have. `None` when it has
/// them all.
fn path_with(dirs: &[Option<&Path>], path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let path: Vec<PathBuf> = path
        .map(std::env::split_paths)
        .into_iter()
        .flatten()
        .collect();
    let mut front: Vec<PathBuf> = Vec::new();
    for dir in dirs.iter().flatten() {
        if dir.is_absolute() && !path.iter().chain(&front).any(|p| p == dir) {
            front.push(dir.to_path_buf());
        }
    }
    if front.is_empty() {
        return None;
    }
    std::env::join_paths(front.into_iter().chain(path))
        .ok()
        .map(PathBuf::from)
}

pub fn cli_add(target: &Target, spec: &Spec, cli: &Path) -> Option<CliCall> {
    let name = spec.name.clone();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let args: Vec<String> = match (target.client, &spec.transport) {
        (Client::ClaudeCode, Transport::Stdio { command, args, env }) => {
            let mut v = s(&["mcp", "add", "--scope", "user"]);
            v.push(name);
            for (k, val) in env {
                v.extend(["-e".into(), format!("{k}={val}")]);
            }
            v.push("--".into());
            v.push(command.clone());
            v.extend(args.iter().cloned());
            v
        }
        // `mcp add` takes no `headersHelper`; `add-json` takes the entry whole. It names the
        // helper, never the token.
        (Client::ClaudeCode, Transport::Http { .. }) if spec.token_helper.is_some() => {
            let mut v = s(&["mcp", "add-json", "--scope", "user"]);
            v.extend([name, entry(Client::ClaudeCode, spec).1.to_string()]);
            v
        }
        // `codex mcp add` takes no `http_headers_helper`: only the config file can hold it.
        (Client::Codex, Transport::Http { .. }) if spec.token_helper.is_some() => return None,
        (Client::ClaudeCode, Transport::Http { url, headers }) => {
            let mut v = s(&["mcp", "add", "--scope", "user", "--transport", "http"]);
            v.extend([name, url.clone()]);
            for (k, val) in headers {
                v.extend(["--header".into(), format!("{k}: {val}")]);
            }
            v
        }
        (Client::Codex, Transport::Stdio { command, args, env }) => {
            let mut v = s(&["mcp", "add"]);
            v.push(name);
            for (k, val) in env {
                v.extend(["--env".into(), format!("{k}={val}")]);
            }
            v.push("--".into());
            v.push(command.clone());
            v.extend(args.iter().cloned());
            v
        }
        (Client::Codex, Transport::Http { url, headers }) => {
            let mut v = s(&["mcp", "add"]);
            v.extend([name, "--url".into(), url.clone()]);
            match spec.bearer_env() {
                Some(var) => v.extend(["--bearer-token-env-var".into(), var.into()]),
                // Other headers: only the config file can hold them.
                None if !headers.is_empty() => return None,
                None => {}
            }
            v
        }
        _ => return None,
    };
    cli_call(target, cli, args)
}

pub fn cli_remove(target: &Target, cli: &Path, name: &str) -> Option<CliCall> {
    let args: Vec<&str> = match target.client {
        Client::ClaudeCode => vec!["mcp", "remove", "--scope", "user", name],
        Client::Codex => vec!["mcp", "remove", name],
        _ => return None,
    };
    cli_call(target, cli, args.into_iter().map(String::from).collect())
}

pub(crate) async fn run_cli(
    call: &CliCall,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new(&call.program);
    cmd.args(&call.args)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    for (k, v) in &call.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    let out = tokio::time::timeout(timeout, cmd.output())
        .await
        .map_err(|_| CoreError::Config(format!("The client's CLI didn't finish within {} seconds. Try again, or add the server manually.", timeout.as_secs())))??;
    Ok(out)
}

pub(super) const CLI_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) fn cli_error(target: &Target, out: &std::process::Output) -> CoreError {
    CoreError::Command {
        program: target.client.cli().unwrap_or_default().into(),
        code: out.status.code(),
        stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
    }
}
