//! The MCP servers the MCP page offers to install, besides the bundled
//! `sawit-mcp`. Names match what each service documents, so an entry added
//! earlier by hand (or by `claude mcp add`) is recognised as installed.

use std::path::{Path, PathBuf};

use crate::mcp_clients::{self, Entries, Spec, Status, Target};
use crate::package_manager::PackageManager;

mod servers;
#[cfg(test)]
mod tests;

pub use servers::SERVERS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The `sawit-mcp` next to the app.
    Bundled,
    /// A remote server. `{region}` in the URL is filled in at install time.
    Http {
        url: &'static str,
        headers: &'static [(&'static str, &'static str)],
    },
    /// A program run on this Mac. `command` is looked up in `PATH`.
    Stdio {
        command: &'static str,
        args: &'static [&'static str],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// Nothing to sign in to.
    None,
    /// The client signs in through the browser (OAuth).
    Browser,
    /// A key in this environment variable, set before starting the client.
    EnvKey(&'static str),
    /// The program keeps its own sign-in (e.g. `cap login`).
    Program,
    /// Sawit's one GitHub sign-in, handed to the client each time it connects by
    /// `sawit github token` ([`crate::mcp_clients::Handover`]): installing needs it, and no
    /// config holds the token.
    GitHub,
}

/// The scopes of Sawit's sign-in that GitHub's MCP server needs: `repo` for nearly every tool
/// (Actions included), `workflow` for its file writes that touch `.github/workflows`. A sign-in
/// without one of them says so on the server's row, with Sign In Again.
pub const GITHUB_SCOPES: &[&str] = &["repo", "workflow"];

/// The id of GitHub's server: its tools are GitHub's, its token Sawit's sign-in.
pub const GITHUB_ID: &str = "github";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Server {
    /// The name in every client's config.
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    pub source: Source,
    pub auth: Auth,
    /// A requirement or caveat shown with the server.
    pub note: Option<&'static str>,
    /// The file in `assets/agent_logos`, if there is one.
    pub logo: Option<(&'static str, bool)>,
}

/// The region the AWS MCP Server URL uses when none is given.
pub const DEFAULT_AWS_REGION: &str = "us-east-1";

impl Server {
    pub fn is_remote(&self) -> bool {
        matches!(self.source, Source::Http { .. })
    }

    /// Whether `client` reaches this server through the `mcp-remote` bridge, as
    /// [`mcp_clients::bridged`] says of its spec.
    pub fn bridged_for(&self, client: mcp_clients::Client) -> bool {
        self.is_remote()
            && match self.auth {
                Auth::GitHub => mcp_clients::handover(client) == mcp_clients::Handover::Bridge,
                _ => mcp_clients::needs_bridge(client),
            }
    }

    /// Whether the URL has a `{region}` to fill in.
    pub fn has_region(&self) -> bool {
        matches!(self.source, Source::Http { url, .. } if url.contains("{region}"))
    }

    /// The URL or command line, for display.
    pub fn address(&self, region: &str) -> String {
        self.address_with(region, PackageManager::Npm)
    }

    /// [`Self::address`], a package run with `pm` (`bunx rea-agents mcp`).
    pub fn address_with(&self, region: &str, pm: PackageManager) -> String {
        match self.source {
            Source::Bundled => "built in".into(),
            Source::Http { url, .. } => url.replace("{region}", region),
            Source::Stdio { .. } if let Some(package) = self.package() => pm.run_line(package),
            Source::Stdio { command, args } => format!("{command} {}", args.join(" ")),
        }
    }

    /// For a local server that runs a package with `npx -y`, the package and
    /// what it's given: any package manager can run it.
    pub fn package(&self) -> Option<&'static [&'static str]> {
        match self.source {
            Source::Stdio {
                command: "npx",
                args,
            } => Some(args.strip_prefix(&["-y"]).unwrap_or(args)),
            _ => None,
        }
    }

    /// The region an installed entry points at, read from its URL (or the
    /// `mcp-remote` bridge's arguments, which hold it).
    pub fn region_in(&self, signature: &str) -> Option<String> {
        let Source::Http { url, .. } = self.source else {
            return None;
        };
        let (before, after) = url.split_once("{region}")?;
        let start = signature.find(before)? + before.len();
        let len = signature[start..].find(after)?;
        let region = &signature[start..start + len];
        valid_region(region).then(|| region.to_string())
    }

    /// The program a local server needs, when it isn't found.
    pub fn missing_program(&self, find: impl Fn(&str) -> Option<PathBuf>) -> Option<&'static str> {
        self.missing_program_with(find, PackageManager::Npm)
    }

    /// [`Self::missing_program`], a package run with `pm`: its runner.
    pub fn missing_program_with(
        &self,
        find: impl Fn(&str) -> Option<PathBuf>,
        pm: PackageManager,
    ) -> Option<&'static str> {
        match self.source {
            Source::Stdio { .. } if self.package().is_some() => {
                find(pm.runner()).is_none().then_some(pm.runner())
            }
            Source::Stdio { command, .. } if find(command).is_none() => Some(command),
            _ => None,
        }
    }

    /// What to install. `find` looks a program up in `PATH`: apps such as
    /// Claude Desktop get a minimal `PATH`, so commands are written as
    /// absolute paths when found.
    pub fn spec(
        &self,
        server: &Path,
        region: &str,
        find: impl Fn(&str) -> Option<PathBuf>,
    ) -> Spec {
        self.spec_with(server, region, find, PackageManager::Npm)
    }

    /// [`Self::spec`], packages (a local server's, the `mcp-remote` bridge)
    /// run with `pm`: `npx -y pkg` becomes `bunx pkg`, `pnpm dlx pkg` or
    /// `yarn dlx pkg`.
    pub fn spec_with(
        &self,
        server: &Path,
        region: &str,
        find: impl Fn(&str) -> Option<PathBuf>,
        pm: PackageManager,
    ) -> Spec {
        let resolve = |cmd: &str| {
            find(cmd)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| cmd.to_string())
        };
        let runner = resolve(pm.runner());
        let runner_args: Vec<String> = pm.runner_args().iter().map(|a| a.to_string()).collect();
        let spec = match self.source {
            Source::Bundled => Spec::sawit(server),
            Source::Http { url, headers } => headers.iter().fold(
                Spec::http(self.id, url.replace("{region}", region)),
                |spec, (k, v)| spec.header(k, v),
            ),
            Source::Stdio { .. } if let Some(package) = self.package() => Spec::stdio(
                self.id,
                runner.clone(),
                runner_args
                    .iter()
                    .cloned()
                    .chain(package.iter().map(|a| a.to_string()))
                    .collect(),
            ),
            Source::Stdio { command, args } => Spec::stdio(
                self.id,
                resolve(command),
                args.iter().map(|a| a.to_string()).collect(),
            ),
        };
        // npx has its node beside it (nvm's, Homebrew's); the others don't.
        let node_dir = (pm != PackageManager::Npm)
            .then(|| find("node"))
            .flatten()
            .and_then(|node| Some(node.parent()?.display().to_string()));
        let spec = spec.with_runner(runner, runner_args, node_dir);
        match self.auth {
            Auth::GitHub => spec.with_token_helper(sawit_beside(server).display().to_string()),
            _ => spec,
        }
    }
}

/// The `sawit` command beside `server`, the `sawit-mcp` the MCP page installs: the app's binary
/// (`sawit-mcp` is a link to it in the bundle), which hands over the GitHub sign-in.
pub fn sawit_beside(server: &Path) -> PathBuf {
    server.with_file_name("sawit")
}

pub fn get(id: &str) -> Option<&'static Server> {
    SERVERS.iter().find(|s| s.id == id)
}

/// Whether `region` names an AWS Region (`us-east-1`, `ap-southeast-2`,
/// `us-gov-west-1`): it goes into the server's host name, so nothing else
/// may.
pub fn valid_region(region: &str) -> bool {
    let parts: Vec<&str> = region.split('-').collect();
    let letters = |p: &&str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase());
    match parts.as_slice() {
        [area, middle @ .., number] => {
            area.len() == 2
                && letters(area)
                && !middle.is_empty()
                && middle.iter().all(letters)
                && !number.is_empty()
                && number.bytes().all(|b| b.is_ascii_digit())
        }
        _ => false,
    }
}

/// The MCP page's list, as `list_mcp_servers` and `sawit list mcp` show it:
/// the AI clients (and profiles) on this Mac, and every server of the
/// catalog with the clients that have it.
#[derive(Debug, Clone)]
pub struct Report {
    pub targets: Vec<Target>,
    pub servers: Vec<Listed>,
}

/// One server of a [`Report`].
#[derive(Debug, Clone)]
pub struct Listed {
    pub server: &'static Server,
    /// The program a local server runs, when it isn't installed.
    pub missing_program: Option<&'static str>,
    /// The clients that have it, by their place in [`Report::targets`], each
    /// with how: every status but [`Status::NotInstalled`].
    pub installed: Vec<(usize, Status)>,
}

/// The [`Report`] for this Mac. `sawit_mcp` is the bundled server that
/// Sawit's own entry points at.
pub fn report(sawit_mcp: &Path) -> Report {
    report_for(
        mcp_clients::targets(),
        sawit_mcp,
        crate::dev_tools::find_program,
    )
}

/// [`report`] for the clients `targets`, with `find` looking programs up.
fn report_for(
    targets: Vec<Target>,
    sawit_mcp: &Path,
    find: impl Fn(&str) -> Option<PathBuf> + Copy,
) -> Report {
    // Each config is read once for the whole catalog.
    let configs: Vec<Entries> = targets.iter().map(mcp_clients::read_entries).collect();
    let servers = SERVERS
        .iter()
        .map(|server| {
            let spec = server.spec(sawit_mcp, DEFAULT_AWS_REGION, find);
            Listed {
                server,
                missing_program: server.missing_program(find),
                installed: configs
                    .iter()
                    .enumerate()
                    .filter_map(|(ix, config)| match mcp_clients::status_in(config, &spec) {
                        Status::NotInstalled => None,
                        status => Some((ix, status)),
                    })
                    .collect(),
            }
        })
        .collect();
    Report { targets, servers }
}
