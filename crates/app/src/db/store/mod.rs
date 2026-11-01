//! Connection profiles and live connections shared by the database views.
//!
//! Besides the saved profiles, the store holds the Cloudflare D1 databases
//! of the window's Workers projects ([`DbStore::detected`]), found again
//! whenever the Database panel is shown and never saved. Every profile is
//! opened as a [`Conn`]: a PostgreSQL client, or a SQLite file or D1
//! database that holds nothing open between operations.
//!
//! Each profile's connection follows [`ConnState`]: idle until something
//! needs it, reconnected with backoff when it drops, and closed again after
//! [`IDLE_TIMEOUT`] unused or [`BACKGROUND_GRACE`] after the app loses
//! focus (so a serverless database such as Neon can scale to zero), but
//! never while a query is using it.

mod connection;
mod profiles;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::Context;
use sawit_core::db::{self, Conn, ConnProfile, Profiles, Relation, SchemaInfo, WriteRoute};
use sawit_core::instances::AppPaths;

use super::lifecycle::ConnState;

/// A connection nothing has used for this long closes (it reopens on use).
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Connections close this long after the app goes to the background; a
/// quick switch to another app and back keeps them.
pub const BACKGROUND_GRACE: Duration = Duration::from_secs(30);
/// How often drops, idle connections and due reconnects are checked.
const TICK: Duration = Duration::from_secs(1);

pub struct DbStore {
    file: PathBuf,
    pub profiles: Profiles,
    /// The D1 databases of the window's Workers projects: local first, then remote, then the
    /// local files no binding names. Not saved; found again from the projects.
    pub detected: db::d1::Detected,
    /// The folders D1 databases are looked for in: the window's projects, and the folders of
    /// `.env` files imported.
    projects: Vec<PathBuf>,
    extra_dirs: Vec<PathBuf>,
    /// SQLite files an agent opened by id, kept in the list.
    extra_files: Vec<PathBuf>,
    pub clients: HashMap<String, Conn>,
    /// Read-only connections for the table browser (separate from `clients`
    /// so an open transaction in the SQL editor never affects them).
    pub ro_clients: HashMap<String, Conn>,
    /// Where writes to each SQLite or D1 database go, as last looked at (on connect and
    /// reload); each write looks again.
    pub routes: HashMap<String, WriteRoute>,
    /// Each PostgreSQL server's major version, once connected.
    pub versions: HashMap<String, u32>,
    /// Kept while disconnected, so the tree stays browsable.
    pub schemas: HashMap<String, Vec<SchemaInfo>>,
    /// Bumped whenever `schemas` changes, so observers can skip other notifies.
    pub schema_rev: u64,
    states: HashMap<String, ConnState>,
    /// Bumped when an attempt starts or a connection is closed; a result
    /// from an older attempt is dropped.
    epochs: HashMap<String, u64>,
    /// When each connection was last handed out.
    last_used: RefCell<HashMap<String, Instant>>,
    /// When the app went to the background; `None` while in front.
    background_since: Option<Instant>,
    /// Per-profile problems that don't affect the connection (a schema
    /// reload that failed).
    pub notices: HashMap<String, String>,
    pub notice: Option<String>,
}

impl DbStore {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let file = AppPaths::default_macos().root.join("profiles.toml");
        let profiles = Profiles::load(&file).unwrap_or_default();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if this.update(cx, |this, cx| this.tick(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self::with_profiles(file, profiles)
    }

    fn with_profiles(file: PathBuf, profiles: Profiles) -> Self {
        Self {
            file,
            profiles,
            detected: db::d1::Detected::default(),
            projects: Vec::new(),
            extra_dirs: Vec::new(),
            extra_files: Vec::new(),
            clients: HashMap::new(),
            ro_clients: HashMap::new(),
            routes: HashMap::new(),
            versions: HashMap::new(),
            schemas: HashMap::new(),
            schema_rev: 0,
            states: HashMap::new(),
            epochs: HashMap::new(),
            last_used: RefCell::default(),
            background_since: None,
            notices: HashMap::new(),
            notice: None,
        }
    }

    /// A store with no file and no connections, for unit tests.
    #[cfg(test)]
    pub fn detached(profiles: Profiles) -> Self {
        Self::with_profiles(PathBuf::new(), profiles)
    }

    fn save(&mut self) {
        if let Err(e) = self.profiles.save(&self.file) {
            self.notice = Some(format!("Unable to save connections. {e}"));
        }
    }

    pub fn state(&self, name: &str) -> &ConnState {
        const IDLE: &ConnState = &ConnState::Idle;
        self.states.get(name).unwrap_or(IDLE)
    }

    #[cfg(test)]
    pub fn set_state(&mut self, name: &str, state: ConnState) {
        self.states.insert(name.to_string(), state);
    }

    fn alive(&self, name: &str) -> bool {
        let alive = |m: &HashMap<String, Conn>| m.get(name).is_some_and(|c| !c.is_closed());
        alive(&self.clients) && alive(&self.ro_clients)
    }

    /// Connected and the connection is still alive (a dropped link or a
    /// server restart closes it; the store then reconnects).
    pub fn is_connected(&self, name: &str) -> bool {
        self.state(name).is_connected() && self.alive(name)
    }

    /// A query or page holds a connection right now (views keep a clone of
    /// the client only while a request runs).
    fn busy(&self, name: &str) -> bool {
        let held = |m: &HashMap<String, Conn>| m.get(name).is_some_and(Conn::in_use);
        held(&self.clients) || held(&self.ro_clients)
    }

    /// A live client for `name` (read-only or read-write), if connected.
    /// Counts as use for the idle timeout.
    pub fn client(&self, name: &str, read_only: bool) -> Option<Conn> {
        if !self.is_connected(name) {
            return None;
        }
        self.last_used
            .borrow_mut()
            .insert(name.to_string(), Instant::now());
        let clients = if read_only {
            &self.ro_clients
        } else {
            &self.clients
        };
        clients.get(name).cloned()
    }

    /// Connects (or reconnects) unless that is already under way. What
    /// views call when they need a connection.
    pub fn ensure_connecting(&mut self, name: &str, cx: &mut Context<Self>) {
        if !self.is_connected(name) {
            self.connect(name, cx);
        }
    }

    /// A relation from the loaded schema tree (its keys and row estimate
    /// save the table view a round trip).
    pub fn relation(&self, name: &str, schema: &str, table: &str) -> Option<&Relation> {
        self.schemas
            .get(name)?
            .iter()
            .find(|s| s.name == schema)?
            .relations
            .iter()
            .find(|r| r.name == table)
    }

    pub fn profile(&self, name: &str) -> Option<&ConnProfile> {
        self.profiles
            .items
            .iter()
            .chain(self.detected.all())
            .find(|p| p.name == name)
    }

    /// Saved profiles, then the detected D1 databases.
    fn names(&self) -> Vec<String> {
        self.profiles
            .items
            .iter()
            .chain(self.detected.all())
            .map(|p| p.name.clone())
            .collect()
    }

    /// "PostgreSQL 16 · kyzn", "Cloudflare D1 (local) · DB": the tooltip's and accessible
    /// name's first line.
    pub fn accessible_name(&self, name: &str) -> String {
        let version = self.versions.get(name).map(u32::to_string);
        self.profile(name)
            .map(|p| p.accessible_name(version.as_deref()))
            .unwrap_or_else(|| name.to_string())
    }

    /// Where writes to a SQLite or D1 profile go, as last looked at.
    pub fn route(&self, name: &str) -> Option<&WriteRoute> {
        self.routes.get(name)
    }
}
