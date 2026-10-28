//! State shared by the server views: the instance registry, live statuses,
//! the selected instance's log, and pending port-conflict reports.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use futures::StreamExt;
use gpui_kit::{Context, EventEmitter, Task};
use pgdesk_core::binaries::{PgBinaries, SearchRoots, discover};
use pgdesk_core::control::{self, Preflight, StartOutcome};
use pgdesk_core::download::install_managed;
use pgdesk_core::instances::{AppPaths, Instance, ListenScope, Registry, parse_conf_port};
use pgdesk_core::pgctl::PgCtlStatus;
use pgdesk_core::ports::PortVerdict;
use pgdesk_core::probe::{PortReport, check_port};

use crate::rt;

const MAX_LOG_LINES: usize = 5_000;
const STATUS_INTERVAL: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Unknown,
    Known(PgCtlStatus),
    Error(String),
}

/// Where the binaries for a new instance come from.
#[derive(Debug, Clone)]
pub enum BinaryChoice {
    Installed(PgBinaries),
    Download { major: u32 },
}

/// Asks the workspace to show (and reopen if closed) the Server tab.
pub struct ShowServerTab;

impl EventEmitter<ShowServerTab> for ServerStore {}

pub struct ServerStore {
    pub paths: AppPaths,
    pub registry: Registry,
    pub binaries: Vec<PgBinaries>,
    pub statuses: HashMap<String, Status>,
    pub selected: Option<String>,
    pub creating: bool,
    pub busy: HashSet<String>,
    pub log: Vec<String>,
    /// Port report that blocked the last start of an instance.
    pub blocked: HashMap<String, PortReport>,
    pub errors: HashMap<String, String>,
    pub notice: Option<String>,
    _log_task: Option<Task<()>>,
    _poll_task: Task<()>,
}

impl ServerStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let paths = AppPaths::default_macos();
        let registry = Registry::load(&paths.instances_file()).unwrap_or_else(|e| {
            eprintln!("registry: {e}");
            Registry::default()
        });
        let selected = registry.instances.first().map(|i| i.name.clone());
        let poll = cx.spawn(async move |this, cx| {
            loop {
                if this
                    .update(cx, |this, cx| this.refresh_statuses(cx))
                    .is_err()
                {
                    return;
                }
                cx.background_executor().timer(STATUS_INTERVAL).await;
            }
        });
        let mut store = ServerStore {
            paths,
            registry,
            binaries: vec![],
            statuses: HashMap::new(),
            selected: None,
            creating: false,
            busy: HashSet::new(),
            log: vec![],
            blocked: HashMap::new(),
            errors: HashMap::new(),
            notice: None,
            _log_task: None,
            _poll_task: poll,
        };
        store.rediscover_binaries(cx);
        if let Some(name) = selected {
            store.select(&name, cx);
        }
        store
    }

    pub fn selected_instance(&self) -> Option<&Instance> {
        self.registry.get(self.selected.as_deref()?)
    }

    pub fn status(&self, name: &str) -> &Status {
        self.statuses.get(name).unwrap_or(&Status::Unknown)
    }

    pub fn running_count(&self) -> usize {
        self.statuses
            .values()
            .filter(|s| matches!(s, Status::Known(PgCtlStatus::Running { .. })))
            .count()
    }

    /// Version of the binaries an instance uses, if they were discovered.
    pub fn version_of(&self, inst: &Instance) -> Option<String> {
        self.binaries
            .iter()
            .find(|b| b.bin_dir == inst.bin_dir)
            .map(|b| b.version.to_string())
    }

    pub fn rediscover_binaries(&mut self, cx: &mut Context<Self>) {
        let managed = self.paths.managed_bin_root();
        let rx = rt::spawn(cx, async move {
            let roots = SearchRoots::detect(managed).await;
            discover(&roots).await
        });
        cx.spawn(async move |this, cx| {
            if let Ok(found) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    this.binaries = found;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn refresh_statuses(&mut self, cx: &mut Context<Self>) {
        let instances = self.registry.instances.clone();
        if instances.is_empty() {
            return;
        }
        let rx = rt::spawn(cx, async move {
            let mut out = Vec::with_capacity(instances.len());
            for inst in instances {
                let status = match control::status(&inst).await {
                    Ok(s) => Status::Known(s),
                    Err(e) => Status::Error(e.to_string()),
                };
                out.push((inst.name, status));
            }
            out
        });
        cx.spawn(async move |this, cx| {
            if let Ok(statuses) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    let changed = statuses
                        .iter()
                        .any(|(n, s)| this.statuses.get(n) != Some(s));
                    this.statuses.extend(statuses);
                    if changed {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub fn select(&mut self, name: &str, cx: &mut Context<Self>) {
        self.creating = false;
        cx.emit(ShowServerTab);
        if self.selected.as_deref() == Some(name) && self._log_task.is_some() {
            cx.notify();
            return;
        }
        self.selected = Some(name.to_string());
        self.log.clear();
        let path = self.paths.log_file(name);
        let mut lines = cx
            .global::<rt::Rt>()
            .0
            .spawn_stream(move |tx| Box::pin(control::tail_log(path, tx)));
        self._log_task = Some(cx.spawn(async move |this, cx| {
            while let Some(first) = lines.next().await {
                // Drain whatever else is already queued to batch UI updates.
                let mut batch = vec![first];
                while let Ok(line) = lines.try_recv() {
                    batch.push(line);
                }
                if this
                    .update(cx, |this, cx| {
                        this.log.extend(batch);
                        let excess = this.log.len().saturating_sub(MAX_LOG_LINES);
                        this.log.drain(..excess);
                        cx.notify();
                    })
                    .is_err()
                {
                    return;
                }
            }
        }));
        cx.notify();
    }

    pub fn show_create(&mut self, cx: &mut Context<Self>) {
        self.creating = true;
        cx.emit(ShowServerTab);
        cx.notify();
    }

    pub fn dismiss_blocked(&mut self, name: &str, cx: &mut Context<Self>) {
        self.blocked.remove(name);
        cx.notify();
    }

    pub fn start(&mut self, name: &str, preflight: Preflight, cx: &mut Context<Self>) {
        self.run_lifecycle(name, preflight, false, cx);
    }

    pub fn restart(&mut self, name: &str, cx: &mut Context<Self>) {
        self.run_lifecycle(name, Preflight::Check, true, cx);
    }

    fn run_lifecycle(
        &mut self,
        name: &str,
        preflight: Preflight,
        restart: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(inst) = self.registry.get(name).cloned() else {
            return;
        };
        self.busy.insert(inst.name.clone());
        self.blocked.remove(&inst.name);
        self.errors.remove(&inst.name);
        cx.notify();
        let paths = self.paths.clone();
        let managed = self.registry.data_dirs();
        let rx = rt::spawn(cx, async move {
            if restart {
                control::restart(&inst, &paths, managed, preflight).await
            } else {
                control::start(&inst, &paths, managed, preflight).await
            }
        });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.busy.remove(&name);
                match result {
                    Ok(Ok(StartOutcome::Blocked(report))) => {
                        this.blocked.insert(name.clone(), report);
                    }
                    Ok(Ok(StartOutcome::AlreadyRunning { pid })) => {
                        this.notice = Some(format!("{name} sudah berjalan (PID {pid})."));
                    }
                    Ok(Ok(StartOutcome::Started)) => {}
                    Ok(Err(e)) => {
                        this.errors.insert(name.clone(), e.to_string());
                    }
                    Err(_) => {}
                }
                this.refresh_statuses(cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn stop(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(inst) = self.registry.get(name).cloned() else {
            return;
        };
        self.busy.insert(inst.name.clone());
        self.errors.remove(&inst.name);
        cx.notify();
        let rx = rt::spawn(cx, async move { control::stop(&inst).await });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.busy.remove(&name);
                if let Ok(Err(e)) = result {
                    this.errors.insert(name.clone(), e.to_string());
                }
                this.refresh_statuses(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Removes an instance from the list (its data directory is kept on disk).
    pub fn forget(&mut self, name: &str, cx: &mut Context<Self>) {
        self.registry.remove(name);
        if let Err(e) = self.registry.save(&self.paths.instances_file()) {
            self.notice = Some(format!("Gagal menyimpan daftar instance: {e}"));
        }
        self.statuses.remove(name);
        if self.selected.as_deref() == Some(name) {
            self.selected = None;
            self._log_task = None;
            self.log.clear();
        }
        cx.notify();
    }

    /// Lowest port from 5432 that no instance uses and nothing listens on.
    pub fn suggest_port(&self, cx: &mut Context<Self>) -> futures::channel::oneshot::Receiver<u16> {
        let taken: HashSet<u16> = self.registry.instances.iter().map(|i| i.port).collect();
        let addrs = ListenScope::Localhost.bind_addrs();
        rt::spawn(cx, async move {
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
        })
    }

    /// Creates a new managed instance: optional binary download, `initdb`, save.
    pub fn create(
        &mut self,
        name: String,
        port: u16,
        choice: BinaryChoice,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        if let Err(e) = pgdesk_core::instances::validate_name(&name) {
            return Task::ready(Err(e.to_string()));
        }
        if self.registry.get(&name).is_some() {
            return Task::ready(Err(format!("Instance \"{name}\" sudah ada.")));
        }
        let paths = self.paths.clone();
        let rx = rt::spawn(cx, async move {
            let bins = match choice {
                BinaryChoice::Installed(b) => b,
                BinaryChoice::Download { major } => {
                    install_managed(&format!("={major}"), &paths.managed_bin_root())
                        .await
                        .map_err(|e| e.to_string())?
                }
            };
            let inst = Instance {
                data_dir: paths.cluster_dir(&name),
                name,
                bin_dir: bins.bin_dir,
                port,
                listen: ListenScope::Localhost,
                managed: true,
            };
            if !inst.data_dir.join("PG_VERSION").exists() {
                control::init_cluster(&inst)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Ok::<_, String>(inst)
        });
        cx.spawn(async move |this, cx| {
            let inst = rx.await.map_err(|_| "dibatalkan".to_string())??;
            this.update(cx, |this, cx| {
                let name = inst.name.clone();
                this.registry.add(inst).map_err(|e| e.to_string())?;
                this.registry
                    .save(&this.paths.instances_file())
                    .map_err(|e| e.to_string())?;
                this.rediscover_binaries(cx);
                this.select(&name, cx);
                this.refresh_statuses(cx);
                Ok(())
            })
            .map_err(|_| "jendela ditutup".to_string())?
        })
    }

    /// Adds an existing data directory (e.g. a Homebrew cluster) to the list.
    pub fn adopt(
        &mut self,
        data_dir: std::path::PathBuf,
        cx: &mut Context<Self>,
    ) -> Task<Result<(), String>> {
        let Some(bins) = self.binaries.first().cloned() else {
            return Task::ready(Err("Tidak ada binary PostgreSQL yang ditemukan.".into()));
        };
        let name = data_dir
            .file_name()
            .map(|n| {
                n.to_string_lossy()
                    .to_lowercase()
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() || c == '-' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .take(40)
                    .collect::<String>()
            })
            .unwrap_or_else(|| "adopted".into());
        // Prefer binaries whose major version matches the cluster's PG_VERSION.
        let major = std::fs::read_to_string(data_dir.join("PG_VERSION"))
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        let bins = major
            .and_then(|m| self.binaries.iter().find(|b| b.version.major == m).cloned())
            .unwrap_or(bins);
        let port = std::fs::read_to_string(data_dir.join("postgresql.conf"))
            .ok()
            .and_then(|conf| parse_conf_port(&conf))
            .unwrap_or(5432);
        let inst = Instance {
            name,
            bin_dir: bins.bin_dir,
            data_dir,
            port,
            listen: ListenScope::Localhost,
            managed: false,
        };
        let check = inst.clone();
        let rx = rt::spawn(cx, async move { control::status(&check).await });
        cx.spawn(async move |this, cx| {
            match rx.await {
                Ok(Ok(PgCtlStatus::NotACluster | PgCtlStatus::MissingDir)) => {
                    return Err("Folder itu bukan data dir PostgreSQL.".to_string());
                }
                Ok(Err(e)) => return Err(e.to_string()),
                _ => {}
            }
            this.update(cx, |this, cx| {
                let name = inst.name.clone();
                this.registry.add(inst).map_err(|e| e.to_string())?;
                this.registry
                    .save(&this.paths.instances_file())
                    .map_err(|e| e.to_string())?;
                this.select(&name, cx);
                this.refresh_statuses(cx);
                Ok(())
            })
            .map_err(|_| "jendela ditutup".to_string())?
        })
    }
}
