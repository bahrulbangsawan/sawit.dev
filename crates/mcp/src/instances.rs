//! Local PostgreSQL instances managed by Sawit: create, start, stop,
//! restart, remove, logs, port checks and the installed binaries.

use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{schemars, tool, tool_router};
use sawit_core::app_control::Store;
use sawit_core::binaries::{PgBinaries, SearchRoots, discover};
use sawit_core::control::{self, Preflight, StartOutcome};
use sawit_core::download::install_managed;
use sawit_core::instances::{Instance, ListenScope, validate_name};
use sawit_core::pgctl::PgCtlStatus;
use sawit_core::ports::PortVerdict;
use sawit_core::probe::check_port;

use crate::db::check_confirm;
use crate::{Sawit, blocking, fail, instance_url, ok, tilde, tri};

/// Majors Sawit can download (newest first; the first is the default).
const DOWNLOADABLE: [u32; 3] = [18, 17, 16];

/// Log lines returned by default and at most.
const LOG_LINES: usize = 100;
const MAX_LOG_LINES: usize = 2000;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct InstanceArgs {
    #[schemars(description = "Sawit instance name (see list_instances)")]
    pub name: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct CreateArgs {
    #[schemars(
        description = "Name: lowercase letters, digits and dashes (it is also the data folder name)"
    )]
    pub name: String,
    #[schemars(description = "Port (default: the first free port from 5432)")]
    pub port: Option<u16>,
    #[schemars(
        description = "PostgreSQL major version: 16, 17 or 18. Installed binaries of that version are used, else Sawit downloads them. Default: the newest installed, else 18"
    )]
    pub version: Option<u32>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct RemoveArgs {
    #[schemars(description = "Sawit instance name")]
    pub name: String,
    #[schemars(
        description = "Also delete the data folder (all databases) of a Sawit-created instance. Requires confirm = name. Default false: the data is kept on disk"
    )]
    #[serde(default)]
    pub delete_data: bool,
    #[schemars(description = "Must equal `name` when delete_data is true")]
    pub confirm: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct LogArgs {
    #[schemars(description = "Sawit instance name")]
    pub name: String,
    #[schemars(description = "Lines from the end of the log (default 100, max 2000)")]
    pub lines: Option<usize>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct PortArgs {
    #[schemars(description = "TCP port, e.g. 5432")]
    pub port: u16,
}

/// Whether removing `inst` may go ahead with these options.
pub(crate) fn check_remove(
    inst: &Instance,
    delete_data: bool,
    confirm: Option<&str>,
) -> Result<(), String> {
    if !delete_data {
        return Ok(());
    }
    if !inst.managed {
        return Err(format!(
            "Refused: {} uses a data folder Sawit didn't create ({}); Sawit never deletes it. Remove it with delete_data=false, then delete the folder yourself if you mean to.",
            inst.name,
            inst.data_dir.display()
        ));
    }
    check_confirm(
        &inst.name,
        confirm.unwrap_or_default(),
        "delete all data of",
    )
}

/// Binaries for `version` (or the newest) among `installed`.
pub(crate) fn pick_binaries(installed: &[PgBinaries], version: Option<u32>) -> Option<PgBinaries> {
    match version {
        Some(major) => installed.iter().find(|b| b.version.major == major).cloned(),
        None => installed.first().cloned(),
    }
}

/// The last `n` lines of `text`.
pub(crate) fn last_lines(text: &str, n: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// The end of a log file: at most `n` lines from its last 512 KiB.
fn tail_file(path: &Path, n: usize) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(512 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    let text = String::from_utf8_lossy(&bytes);
    // A cut first line is dropped.
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Ok(last_lines(text, n))
}

fn status_text(status: &sawit_core::Result<PgCtlStatus>) -> String {
    match status {
        Ok(PgCtlStatus::Running { pid }) => format!("running (PID {pid})"),
        Ok(PgCtlStatus::Stopped) => "stopped".into(),
        Ok(other) => format!("{other:?}"),
        Err(e) => format!("error: {e}"),
    }
}

impl Sawit {
    pub async fn list_instances_tool(&self) -> CallToolResult {
        let reg = tri!(self.registry());
        if reg.instances.is_empty() {
            return ok("No Sawit instances yet. Create one with create_instance.");
        }
        let mut lines = Vec::new();
        for inst in &reg.instances {
            lines.push(format!(
                "- {} · port {} · {} · {}",
                inst.name,
                inst.port,
                status_text(&control::status(inst).await),
                instance_url(inst)
            ));
        }
        ok(lines.join("\n"))
    }

    fn start_text(
        name: &str,
        inst: &Instance,
        outcome: sawit_core::Result<StartOutcome>,
    ) -> CallToolResult {
        match outcome {
            Ok(StartOutcome::Started) => ok(format!(
                "{name} is running. DATABASE_URL={}",
                instance_url(inst)
            )),
            Ok(StartOutcome::AlreadyRunning { pid }) => {
                ok(format!("{name} is already running (PID {pid})."))
            }
            Ok(StartOutcome::Blocked(report)) => fail(format!(
                "Start held back because of a port conflict. Resolve it first (see list_ports / stop_processes), or ask the user to use \"Start Anyway\" in Sawit.\n{}",
                report.explain()
            )),
            Err(e) => fail(e.to_string()),
        }
    }

    pub async fn start_instance_tool(&self, name: &str) -> CallToolResult {
        let inst = tri!(self.instance(name));
        let managed = self.registry().map(|r| r.data_dirs()).unwrap_or_default();
        let outcome = control::start(&inst, &self.paths, managed, Preflight::Check).await;
        Self::start_text(name, &inst, outcome)
    }

    pub async fn stop_instance_tool(&self, name: &str) -> CallToolResult {
        let inst = tri!(self.instance(name));
        match control::stop(&inst).await {
            Ok(()) => ok(format!("{name} stopped.")),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn restart_instance_tool(&self, name: &str) -> CallToolResult {
        let inst = tri!(self.instance(name));
        let managed = self.registry().map(|r| r.data_dirs()).unwrap_or_default();
        let outcome = control::restart(&inst, &self.paths, managed, Preflight::Check).await;
        Self::start_text(name, &inst, outcome)
    }

    /// Lowest port from 5432 that no instance uses and nothing listens on.
    async fn suggest_port(&self, taken: &HashSet<u16>) -> u16 {
        let addrs = ListenScope::Localhost.bind_addrs();
        for port in 5432..5532 {
            if taken.contains(&port) {
                continue;
            }
            if let Ok(report) = check_port(port, &addrs, vec![]).await
                && report.verdict == PortVerdict::Free
            {
                return port;
            }
        }
        5432
    }

    async fn create_instance_tool(&self, a: CreateArgs) -> CallToolResult {
        let name = a.name.trim().to_string();
        tri!(validate_name(&name).map_err(|e| e.to_string()));
        if let Some(v) = a.version.filter(|v| !DOWNLOADABLE.contains(v)) {
            return fail(format!(
                "PostgreSQL {v} isn't offered; choose 16, 17 or 18."
            ));
        }
        let registry = tri!(self.registry());
        if registry.get(&name).is_some() {
            return fail(format!(
                "An instance named \"{name}\" already exists. Choose another name."
            ));
        }
        let taken: HashSet<u16> = registry.instances.iter().map(|i| i.port).collect();
        let port = match a.port {
            Some(p) if taken.contains(&p) => {
                return fail(format!(
                    "Port {p} is already used by another Sawit instance."
                ));
            }
            Some(p) => p,
            None => self.suggest_port(&taken).await,
        };

        let roots = SearchRoots::detect(self.paths.managed_bin_root()).await;
        let installed = discover(&roots).await;
        let (bins, downloaded) = match pick_binaries(&installed, a.version) {
            Some(b) => (b, false),
            None => {
                let major = a.version.unwrap_or(DOWNLOADABLE[0]);
                match install_managed(&format!("={major}"), &self.paths.managed_bin_root()).await {
                    Ok(b) => (b, true),
                    Err(e) => return fail(format!("Unable to download PostgreSQL {major}. {e}")),
                }
            }
        };
        let inst = Instance {
            data_dir: self.paths.cluster_dir(&name),
            name: name.clone(),
            bin_dir: bins.bin_dir.clone(),
            port,
            listen: ListenScope::Localhost,
            managed: true,
        };
        if !inst.data_dir.join("PG_VERSION").exists() {
            tri!(
                control::init_cluster(&inst)
                    .await
                    .map_err(|e| e.to_string())
            );
        }
        // Re-read: the app may have changed the list meanwhile.
        let mut registry = tri!(self.registry());
        tri!(registry.add(inst.clone()).map_err(|e| e.to_string()));
        tri!(
            registry
                .save(&self.paths.instances_file())
                .map_err(|e| e.to_string())
        );
        self.notify(Store::Instances).await;
        ok(format!(
            "Created {name}: PostgreSQL {} ({}{}) on port {port}. It is stopped; start it with start_instance.\nDATABASE_URL={}",
            bins.version,
            bins.source,
            if downloaded { ", downloaded" } else { "" },
            instance_url(&inst)
        ))
    }

    async fn remove_instance_tool(&self, a: RemoveArgs) -> CallToolResult {
        let inst = tri!(self.instance(&a.name));
        tri!(check_remove(&inst, a.delete_data, a.confirm.as_deref()));
        if let Err(e) = control::stop(&inst).await {
            return fail(format!(
                "Unable to stop {} first, so it was kept. {e}",
                a.name
            ));
        }
        let mut registry = tri!(self.registry());
        registry.remove(&inst.name);
        tri!(
            registry
                .save(&self.paths.instances_file())
                .map_err(|e| e.to_string())
        );
        self.notify(Store::Instances).await;
        if !a.delete_data {
            return ok(format!(
                "Removed {} from Sawit. Its data is kept in {}.",
                inst.name,
                tilde(&inst.data_dir)
            ));
        }
        // Only ever inside Sawit's own clusters folder.
        let clusters = self.paths.root.join("clusters");
        if !inst.data_dir.starts_with(&clusters) {
            return fail(format!(
                "Removed {} from Sawit, but its data folder {} is outside {}, so it was not deleted.",
                inst.name,
                inst.data_dir.display(),
                clusters.display()
            ));
        }
        let dir = inst.data_dir.clone();
        match blocking(move || std::fs::remove_dir_all(&dir)).await {
            Ok(Ok(())) => ok(format!(
                "Removed {} and deleted its data ({}).",
                inst.name,
                tilde(&inst.data_dir)
            )),
            Ok(Err(e)) => fail(format!(
                "Removed {} from Sawit, but deleting {} failed: {e}",
                inst.name,
                inst.data_dir.display()
            )),
            Err(e) => fail(e),
        }
    }

    async fn instance_logs_tool(&self, a: LogArgs) -> CallToolResult {
        let inst = tri!(self.instance(&a.name));
        let n = a.lines.unwrap_or(LOG_LINES).clamp(1, MAX_LOG_LINES);
        let path = self.paths.log_file(&inst.name);
        let shown = tilde(&path);
        match blocking(move || tail_file(&path, n)).await {
            Ok(Ok(text)) if text.trim().is_empty() => ok(format!("{shown} is empty.")),
            Ok(Ok(text)) => ok(format!("{shown} (last {n} lines):\n{text}")),
            Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => ok(format!(
                "No log yet ({shown}); the instance hasn't been started by Sawit."
            )),
            Ok(Err(e)) => fail(format!("Unable to read {shown}: {e}")),
            Err(e) => fail(e),
        }
    }

    async fn check_port_tool(&self, port: u16) -> CallToolResult {
        let managed = self.registry().map(|r| r.data_dirs()).unwrap_or_default();
        match check_port(port, &ListenScope::Localhost.bind_addrs(), managed).await {
            // Starts with the verdict ("Port 5432 is free.").
            Ok(report) => ok(report.explain()),
            Err(e) => fail(e.to_string()),
        }
    }

    async fn list_binaries_tool(&self) -> CallToolResult {
        let roots = SearchRoots::detect(self.paths.managed_bin_root()).await;
        let found = discover(&roots).await;
        let registry = self.registry().unwrap_or_default();
        if found.is_empty() {
            return ok(
                "No PostgreSQL installation found. create_instance downloads one (16, 17 or 18).",
            );
        }
        let lines: Vec<String> = found
            .iter()
            .map(|b| {
                let users: Vec<&str> = registry
                    .instances
                    .iter()
                    .filter(|i| i.bin_dir == b.bin_dir)
                    .map(|i| i.name.as_str())
                    .collect();
                format!(
                    "- PostgreSQL {} · {} · {}{}",
                    b.version,
                    b.source,
                    tilde(&b.bin_dir),
                    if users.is_empty() {
                        String::new()
                    } else {
                        format!(" · used by {}", users.join(", "))
                    }
                )
            })
            .collect();
        ok(lines.join("\n"))
    }
}

#[tool_router(router = instances_router, vis = "pub(crate)")]
impl Sawit {
    #[tool(
        description = "List local PostgreSQL instances managed by Sawit with status, port and DATABASE_URL"
    )]
    async fn list_instances(&self) -> CallToolResult {
        self.list_instances_tool().await
    }

    #[tool(
        description = "Create a new local PostgreSQL instance (initdb, superuser postgres, trust auth on localhost). Uses installed binaries of the requested version or downloads them (may take a minute). Does not start it"
    )]
    async fn create_instance(&self, Parameters(a): Parameters<CreateArgs>) -> CallToolResult {
        self.create_instance_tool(a).await
    }

    #[tool(
        description = "Start a Sawit instance. Refuses (and explains) when another process already listens on its port, e.g. a Docker container"
    )]
    async fn start_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        self.start_instance_tool(&a.name).await
    }

    #[tool(description = "Stop a Sawit instance (fast shutdown)")]
    async fn stop_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        self.stop_instance_tool(&a.name).await
    }

    #[tool(description = "Restart a Sawit instance (stop, then start with the port check)")]
    async fn restart_instance(&self, Parameters(a): Parameters<InstanceArgs>) -> CallToolResult {
        self.restart_instance_tool(&a.name).await
    }

    #[tool(
        description = "Stop an instance and remove it from Sawit. Data is kept unless delete_data=true AND confirm equals the name (only for instances Sawit created)"
    )]
    async fn remove_instance(&self, Parameters(a): Parameters<RemoveArgs>) -> CallToolResult {
        self.remove_instance_tool(a).await
    }

    #[tool(description = "Show the last lines of an instance's PostgreSQL server log")]
    async fn instance_logs(&self, Parameters(a): Parameters<LogArgs>) -> CallToolResult {
        self.instance_logs_tool(a).await
    }

    #[tool(
        description = "Check whether a TCP port is free on localhost, and if not which process (Docker container, Postgres, dev server) holds it"
    )]
    async fn check_port(&self, Parameters(a): Parameters<PortArgs>) -> CallToolResult {
        self.check_port_tool(a.port).await
    }

    #[tool(
        description = "List PostgreSQL installations found on this Mac (Sawit-downloaded, Homebrew, Postgres.app, PATH) and which instances use them"
    )]
    async fn list_postgres_binaries(&self) -> CallToolResult {
        self.list_binaries_tool().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sawit_core::binaries::{BinarySource, PgVersion};
    use std::path::PathBuf;

    fn inst(managed: bool) -> Instance {
        Instance {
            name: "shop".into(),
            bin_dir: PathBuf::from("/bin"),
            data_dir: PathBuf::from("/data/shop"),
            port: 5432,
            listen: ListenScope::Localhost,
            managed,
        }
    }

    #[test]
    fn removing_keeps_data_unless_confirmed() {
        assert!(check_remove(&inst(true), false, None).is_ok());
        assert!(check_remove(&inst(false), false, None).is_ok());
        assert!(check_remove(&inst(true), true, None).is_err());
        assert!(check_remove(&inst(true), true, Some("nope")).is_err());
        assert!(check_remove(&inst(true), true, Some("shop")).is_ok());
        let e = check_remove(&inst(false), true, Some("shop")).unwrap_err();
        assert!(e.contains("didn't create"), "{e}");
    }

    #[test]
    fn binaries_by_version() {
        let b = |major| PgBinaries {
            bin_dir: PathBuf::from(format!("/pg{major}")),
            version: PgVersion { major, minor: 1 },
            source: BinarySource::Homebrew,
        };
        let installed = vec![b(17), b(16)];
        assert_eq!(pick_binaries(&installed, None).unwrap().version.major, 17);
        assert_eq!(
            pick_binaries(&installed, Some(16)).unwrap().version.major,
            16
        );
        assert!(pick_binaries(&installed, Some(18)).is_none());
        assert!(pick_binaries(&[], None).is_none());
    }

    #[test]
    fn log_tail() {
        assert_eq!(last_lines("a\nb\nc\n", 2), "b\nc");
        assert_eq!(last_lines("a", 5), "a");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.log");
        std::fs::write(
            &path,
            (1..=10).map(|i| format!("line {i}\n")).collect::<String>(),
        )
        .unwrap();
        assert_eq!(tail_file(&path, 2).unwrap(), "line 9\nline 10");
    }
}
