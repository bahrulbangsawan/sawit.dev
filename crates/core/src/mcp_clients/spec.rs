//! A server to install ([`Spec`]): its name in every config, and how it is reached
//! ([`Transport`]).

use std::path::Path;

use super::SERVER_NAME;

/// How a server is reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Transport {
    /// A program on this Mac.
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// A remote (streamable HTTP) server.
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
}

/// A server to install: its name in every config, and how to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub name: String,
    pub transport: Transport,
    /// The program that runs packages (`npx`, or the `pnpm`, `bunx` or `yarn`
    /// picked instead): the `mcp-remote` bridge's, which clients without
    /// remote support use, and a local server's that runs a package. An
    /// absolute path when found: apps such as Claude Desktop get a minimal
    /// `PATH`.
    pub npx: String,
    /// What goes between `npx` and the package: `-y` for npx, `dlx` for pnpm
    /// and Yarn, nothing for bunx.
    pub npx_args: Vec<String>,
    /// The folder of the `node` found, when `npx` isn't npx (nvm's node isn't
    /// beside bunx): an app started with launchd's `PATH` is told it too.
    pub node_dir: Option<String>,
    /// For a remote server that takes Sawit's GitHub sign-in (GitHub's own): the `sawit` command,
    /// by its absolute path, that hands it to the client each time it connects
    /// (`sawit github token`), so no config holds the token. See [`handover`].
    pub token_helper: Option<String>,
}

impl Spec {
    /// The bundled `sawit-mcp`.
    pub fn sawit(server: &Path) -> Spec {
        Spec::stdio(SERVER_NAME, server.display().to_string(), Vec::new())
    }

    pub fn stdio(name: &str, command: impl Into<String>, args: Vec<String>) -> Spec {
        Spec {
            name: name.into(),
            transport: Transport::Stdio {
                command: command.into(),
                args,
                env: Vec::new(),
            },
            npx: "npx".into(),
            npx_args: vec!["-y".into()],
            node_dir: None,
            token_helper: None,
        }
    }

    pub fn http(name: &str, url: impl Into<String>) -> Spec {
        Spec {
            name: name.into(),
            transport: Transport::Http {
                url: url.into(),
                headers: Vec::new(),
            },
            npx: "npx".into(),
            npx_args: vec!["-y".into()],
            node_dir: None,
            token_helper: None,
        }
    }

    /// A remote server that takes Sawit's GitHub sign-in, handed over by the `sawit` at `sawit`.
    pub fn with_token_helper(mut self, sawit: impl Into<String>) -> Spec {
        self.token_helper = Some(sawit.into());
        self
    }

    pub fn header(mut self, key: &str, value: &str) -> Spec {
        if let Transport::Http { headers, .. } = &mut self.transport {
            headers.push((key.into(), value.into()));
        }
        self
    }

    pub fn with_npx(self, npx: impl Into<String>) -> Spec {
        self.with_runner(npx, vec!["-y".into()], None)
    }

    /// Runs packages with `program` and `args` before the package (`bunx`
    /// and nothing, `pnpm` and `dlx`) instead of `npx -y`; `node_dir` is the
    /// folder of the `node` found.
    pub fn with_runner(
        mut self,
        program: impl Into<String>,
        args: Vec<String>,
        node_dir: Option<String>,
    ) -> Spec {
        self.npx = program.into();
        self.npx_args = args;
        self.node_dir = node_dir;
        self
    }

    pub fn is_remote(&self) -> bool {
        matches!(self.transport, Transport::Http { .. })
    }

    /// What identifies this server in a config entry: the URL; else the
    /// arguments, since the command may be found at another path, without
    /// the runner's own (`-y`, `dlx`), so an entry another package manager
    /// runs is the same server; else, for a server without arguments, the
    /// command itself.
    pub(super) fn key(&self) -> String {
        match &self.transport {
            Transport::Http { url, .. } => url.clone(),
            Transport::Stdio { command, args, .. } if args.is_empty() => command.clone(),
            Transport::Stdio { command, args, .. } if *command == self.npx => args
                .strip_prefix(self.npx_args.as_slice())
                .unwrap_or(args)
                .join(" "),
            Transport::Stdio { args, .. } => args.join(" "),
        }
    }

    /// `VAR` in an `Authorization: Bearer ${VAR}` header, the only header.
    pub(super) fn bearer_env(&self) -> Option<&str> {
        let Transport::Http { headers, .. } = &self.transport else {
            return None;
        };
        match headers.as_slice() {
            [(k, v)] if k.eq_ignore_ascii_case("authorization") => v
                .strip_prefix("Bearer ${")
                .and_then(|v| v.strip_suffix('}')),
            _ => None,
        }
    }

    /// The server as a program to run: itself, or for a remote server the
    /// `mcp-remote` bridge; for one that takes Sawit's GitHub sign-in, the bridge
    /// started by `sawit github token --exec`, which gives it the header.
    pub(super) fn as_stdio(&self) -> (String, Vec<String>, Vec<(String, String)>) {
        match &self.transport {
            Transport::Stdio { command, args, env } => (command.clone(), args.clone(), env.clone()),
            Transport::Http { url, .. } if let Some(sawit) = &self.token_helper => (
                sawit.clone(),
                crate::github_token::bridge_args(&self.npx, &self.npx_args, url),
                Vec::new(),
            ),
            Transport::Http { url, headers } => {
                let mut args = self.npx_args.clone();
                args.extend(["mcp-remote".to_string(), url.clone()]);
                for (k, v) in headers {
                    args.push("--header".into());
                    args.push(format!("{k}:{v}"));
                }
                (self.npx.clone(), args, Vec::new())
            }
        }
    }

    /// Whether `command`, what [`Self::as_stdio`] runs, starts a package with the runner
    /// (`npx`, `bunx`, …): itself, or behind `sawit github token --exec`.
    pub(super) fn starts_runner(&self, command: &str) -> bool {
        command == self.npx || (self.is_remote() && self.token_helper.as_deref() == Some(command))
    }
}
