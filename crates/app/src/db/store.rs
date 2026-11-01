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

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::{Context, Task};
use sawit_core::db::{
    self, Conn, ConnProfile, FailureKind, Profiles, Relation, SchemaInfo, WriteRoute,
    parse_database_url, read_dotenv_database_url, resolve_password, store_password,
};
use sawit_core::instances::AppPaths;

use super::lifecycle::{ConnState, Event};
use crate::rt;

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

    /// Looks for D1 databases in `projects` (the window's project folders) when they changed.
    pub fn set_projects(&mut self, projects: Vec<PathBuf>, cx: &mut Context<Self>) {
        if projects != self.projects {
            self.projects = projects;
            self.detect(cx);
        }
    }

    /// Also looks for D1 databases in `dir` (an imported `.env`'s folder).
    pub fn add_project_dir(&mut self, dir: &Path, cx: &mut Context<Self>) {
        if !self.extra_dirs.iter().any(|d| d == dir) {
            self.extra_dirs.push(dir.to_path_buf());
        }
        self.detect(cx);
    }

    /// Finds the D1 databases of every project again (config and local folder only, nothing
    /// remote), off the UI thread. Connections of ones that went away close.
    pub fn detect(&mut self, cx: &mut Context<Self>) {
        let mut dirs = self.projects.clone();
        dirs.extend(self.extra_dirs.iter().cloned());
        let files = self.extra_files.clone();
        let rx = rt::spawn_blocking(cx, move || {
            let mut all = db::d1::Detected::default();
            for dir in dirs {
                let found = db::d1::detect(&dir);
                all.local.extend(found.local);
                all.remote.extend(found.remote);
                all.unmatched.extend(found.unmatched);
            }
            for file in files.iter().filter(|f| f.is_file()) {
                let profile = ConnProfile::sqlite_file(file);
                if !all.all().any(|p| p.name == profile.name) {
                    all.unmatched.push(profile);
                }
            }
            all
        });
        cx.spawn(async move |this, cx| {
            let Ok(found) = rx.await else { return };
            let _ = this.update(cx, |this, cx| this.set_detected(found, cx));
        })
        .detach();
    }

    fn set_detected(&mut self, found: db::d1::Detected, cx: &mut Context<Self>) {
        if found == self.detected {
            return;
        }
        let gone: Vec<String> = self
            .detected
            .all()
            .filter(|old| !found.all().any(|p| p == *old))
            .map(|p| p.name.clone())
            .collect();
        for name in &gone {
            self.close(name);
            self.schemas.remove(name);
            self.states.remove(name);
            self.notices.remove(name);
            self.routes.remove(name);
        }
        self.detected = found;
        self.schema_rev += 1;
        cx.notify();
    }

    /// Saves a SQLite file as a connection and opens it.
    pub fn add_sqlite_file(&mut self, file: &Path, cx: &mut Context<Self>) -> String {
        let profile = ConnProfile::sqlite_file(file);
        let name = profile.name.clone();
        self.replace_profile(profile, cx);
        name
    }

    /// Adds a profile from a DATABASE_URL; its password goes to the Keychain.
    pub fn add_url(&mut self, url: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let (profile, password) = parse_database_url(url).map_err(|e| e.to_string())?;
        let name = profile.name.clone();
        let Some(pw) = password else {
            self.replace_profile(profile, cx);
            return Ok(name);
        };
        // Keychain writes block (and can wait on an access prompt): do it off
        // the UI thread, then connect once the password is there.
        let key = profile.clone();
        let rx = rt::spawn_blocking(cx, move || store_password(&key, &pw));
        let shown = name.clone();
        cx.spawn(async move |this, cx| {
            let stored = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.replace_profile(profile, cx);
                if let Ok(Err(e)) = stored {
                    this.notices.insert(
                        shown,
                        format!("Unable to save the password to the Keychain. {e}"),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
        Ok(name)
    }

    pub fn import_env(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<String, String> {
        // A Workers project keeps its D1 databases in its config, not in .env: look there too.
        if let Some(dir) = path.parent() {
            self.add_project_dir(dir, cx);
        }
        match read_dotenv_database_url(&path) {
            Ok(Some(url)) => self.add_url(&url, cx),
            Ok(None) => Err(format!(
                "{} has no DATABASE_URL line. Add one, or paste the URL instead.",
                path.display()
            )),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Applies Connection Settings to `name`: a label (empty clears it), a
    /// connection string and, when given, a new password. The profile keeps
    /// its name, so open tabs follow it; the password moves to the new
    /// Keychain account when the connection details changed.
    pub fn update_profile(
        &mut self,
        name: &str,
        label: &str,
        url: &str,
        password: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(old) = self.profile(name).cloned() else {
            return Err("This connection no longer exists.".into());
        };
        if old.kind.is_lite() {
            // A file has only its label to set.
            let label = label.trim();
            self.profiles.upsert(ConnProfile {
                label: (!label.is_empty()).then(|| label.to_string()),
                ..old
            });
            self.save();
            cx.notify();
            return Ok(());
        }
        let (parsed, url_password) = parse_database_url(url).map_err(|e| e.to_string())?;
        let label = label.trim();
        let profile = ConnProfile {
            name: old.name.clone(),
            label: (!label.is_empty()).then(|| label.to_string()),
            ..parsed
        };
        let moved = profile.keychain_account() != old.keychain_account();
        let password = password.filter(|p| !p.is_empty()).or(url_password);
        if profile == old && password.is_none() {
            return Ok(());
        }
        if !moved && password.is_none() && profile.connection_string() == old.connection_string() {
            // Only the label changed: no reconnect.
            self.profiles.upsert(profile);
            self.save();
            cx.notify();
            return Ok(());
        }
        if moved {
            // Another database now: its tree loads on connect.
            self.schemas.remove(name);
            self.schema_rev += 1;
        }
        // The old entry goes unless another profile still logs in with it.
        let orphaned = moved
            && !self
                .profiles
                .items
                .iter()
                .any(|p| p.name != name && p.keychain_account() == old.keychain_account());
        let (from, to) = (old.clone(), profile.clone());
        let rx = rt::spawn_blocking(cx, move || -> sawit_core::Result<()> {
            let pw = match password {
                Some(pw) => Some(pw),
                None if moved => db::load_password(&from)?,
                None => None,
            };
            if let Some(pw) = pw {
                store_password(&to, &pw)?;
            }
            if orphaned {
                db::delete_password(&from)?;
            }
            Ok(())
        });
        let shown = name.to_string();
        cx.spawn(async move |this, cx| {
            let stored = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.replace_profile(profile, cx);
                if let Ok(Err(e)) = stored {
                    this.notices.insert(
                        shown,
                        format!("Unable to save the password to the Keychain. {e}"),
                    );
                    cx.notify();
                }
            });
        })
        .detach();
        Ok(())
    }

    pub fn add_profile(&mut self, profile: ConnProfile, cx: &mut Context<Self>) {
        self.replace_profile(profile, cx);
    }

    /// Saves `profile` and connects with it; a connection made with the old
    /// settings of the same name is closed first.
    fn replace_profile(&mut self, profile: ConnProfile, cx: &mut Context<Self>) {
        let name = profile.name.clone();
        self.close(&name);
        self.states.remove(&name);
        self.profiles.upsert(profile);
        self.save();
        self.connect(&name, cx);
    }

    /// Reads the profiles again after another process (`sawit-mcp`) changed
    /// them; connections of removed or changed profiles close.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let profiles = match Profiles::load(&self.file) {
            Ok(p) => p,
            Err(e) => {
                self.notice = Some(format!("Unable to read connections. {e}"));
                cx.notify();
                return;
            }
        };
        let stale: Vec<String> = self
            .profiles
            .items
            .iter()
            .filter(|old| !profiles.items.contains(old))
            .map(|old| old.name.clone())
            .collect();
        for name in &stale {
            self.close(name);
            self.schemas.remove(name);
            self.states.remove(name);
            self.notices.remove(name);
            self.last_used.borrow_mut().remove(name);
        }
        self.profiles = profiles;
        self.schema_rev += 1;
        cx.notify();
    }

    pub fn remove(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.profile(name).cloned() {
            drop(rt::spawn_blocking(cx, move || db::delete_password(&p)));
        }
        self.close(name);
        self.profiles.remove(name);
        self.schemas.remove(name);
        self.schema_rev += 1;
        self.states.remove(name);
        self.notices.remove(name);
        self.last_used.borrow_mut().remove(name);
        self.save();
        cx.notify();
    }

    /// Drops the store's handles; the connection closes once no running
    /// request holds it. A connect under way is abandoned.
    fn close(&mut self, name: &str) {
        self.clients.remove(name);
        self.ro_clients.remove(name);
        self.versions.remove(name);
        *self.epochs.entry(name.to_string()).or_default() += 1;
    }

    /// Feeds `event` to the profile's state, starting an attempt when that
    /// enters Connecting.
    fn apply(&mut self, name: &str, event: Event, cx: &mut Context<Self>) {
        let before = self.state(name).clone();
        let after = before.step(event, Instant::now());
        let starts = matches!(after, ConnState::Connecting { .. })
            && !matches!(before, ConnState::Connecting { .. });
        if after != before {
            self.states.insert(name.to_string(), after);
            cx.notify();
        }
        if starts {
            self.open(name, cx);
        }
    }

    /// Connects now (also from Failed, Disconnected or a retry wait).
    pub fn connect(&mut self, name: &str, cx: &mut Context<Self>) {
        if self.profile(name).is_none() {
            // A D1 or SQLite id an agent named (app_open_table) before the panel found it: kept
            // in the list, its folder looked in from now on.
            let Some(Ok(profile)) = db::d1::resolve(name) else {
                return;
            };
            match db::d1::D1Target::parse(name) {
                Some(db::d1::D1Target::Local { project, .. })
                | Some(db::d1::D1Target::Remote { project, .. }) => {
                    if !self.extra_dirs.contains(&project) {
                        self.extra_dirs.push(project);
                    }
                }
                Some(db::d1::D1Target::File(file)) if !self.extra_files.contains(&file) => {
                    self.extra_files.push(file)
                }
                _ => {}
            }
            match profile.kind {
                db::DbKind::D1Remote => self.detected.remote.push(profile),
                db::DbKind::Sqlite => self.detected.unmatched.push(profile),
                _ => self.detected.local.push(profile),
            }
        }
        // Marked connected, but the link died before the tick noticed.
        if self.state(name).is_connected() && !self.alive(name) {
            self.close(name);
            self.apply(name, Event::Dropped { active: true }, cx);
            return;
        }
        self.apply(name, Event::Connect, cx);
    }

    /// Closes the connection and keeps it closed until asked again.
    pub fn disconnect(&mut self, name: &str, cx: &mut Context<Self>) {
        self.close(name);
        self.apply(name, Event::Disconnect, cx);
    }

    /// Starts an attempt for the current Connecting state.
    fn open(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(profile) = self.profile(name).cloned() else {
            return;
        };
        self.close(name);
        let epoch = self.epochs[name];
        self.notices.remove(name);
        let rx = rt::spawn(cx, async move {
            if profile.kind.is_lite() {
                // Nothing to log in to: read the schema read-only, and see where writes go.
                let (rw, ro) = (
                    Conn::lite(profile.clone(), false),
                    Conn::lite(profile, true),
                );
                let schema = ro.load_schema().await?;
                let route = match &rw {
                    Conn::Lite(lite) => Some(lite.write_route().await),
                    Conn::Pg(_) => None,
                };
                return Ok::<_, sawit_core::CoreError>((rw, ro, schema, route, None));
            }
            // Keychain: blocking, and may wait on an access prompt.
            let lookup = profile.clone();
            let password = tokio::task::spawn_blocking(move || resolve_password(&lookup))
                .await
                .unwrap_or_default();
            let (client, ro, schema) = db::connect_all(&profile, password.as_deref()).await?;
            let version = db::server_major(&client).await.ok();
            Ok((
                Conn::Pg(client.into()),
                Conn::Pg(ro.into()),
                schema,
                None,
                version,
            ))
        });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                if this.epochs.get(&name) != Some(&epoch) {
                    return; // abandoned: disconnected or superseded
                }
                let event = match result {
                    Ok(Ok((client, ro, schema, route, version))) => {
                        this.clients.insert(name.clone(), client);
                        this.ro_clients.insert(name.clone(), ro);
                        if let Some(route) = route {
                            this.routes.insert(name.clone(), route);
                        }
                        if let Some(version) = version {
                            this.versions.insert(name.clone(), version);
                        }
                        this.schemas.insert(name.clone(), schema);
                        this.schema_rev += 1;
                        this.last_used
                            .borrow_mut()
                            .insert(name.clone(), Instant::now());
                        Event::Opened
                    }
                    Ok(Err(e)) => {
                        let kind = db::failure_kind(&e);
                        Event::OpenFailed {
                            kind,
                            error: describe(kind, &e),
                        }
                    }
                    Err(_) => Event::OpenFailed {
                        kind: FailureKind::Other,
                        error: "The database runtime stopped.".into(),
                    },
                };
                this.apply(&name, event, cx);
            });
        })
        .detach();
    }

    /// The app came to the front (`true`) or went to the background.
    /// Connections close [`BACKGROUND_GRACE`] after it leaves and stay
    /// closed until something needs them.
    fn set_foreground(&mut self, foreground: bool, cx: &mut Context<Self>) {
        match (foreground, self.background_since) {
            (true, Some(_)) => {
                self.background_since = None;
                // Time away doesn't count as idle time.
                let now = Instant::now();
                for t in self.last_used.borrow_mut().values_mut() {
                    *t = now;
                }
                cx.notify();
            }
            (false, None) => self.background_since = Some(Instant::now()),
            _ => {}
        }
    }

    /// Notices dropped connections, starts due reconnects, and closes
    /// connections that are idle or have been in the background long enough.
    fn tick(&mut self, cx: &mut Context<Self>) {
        // In front while any Sawit window is active: switching between
        // Sawit windows isn't leaving the app.
        self.set_foreground(cx.active_window().is_some(), cx);
        let now = Instant::now();
        let foreground = self.background_since.is_none();
        let away_long = self
            .background_since
            .is_some_and(|t| now.duration_since(t) >= BACKGROUND_GRACE);
        for name in self.names() {
            match self.state(&name) {
                ConnState::Connected if !self.alive(&name) => {
                    self.close(&name);
                    self.apply(&name, Event::Dropped { active: foreground }, cx);
                }
                ConnState::Connected if !self.busy(&name) => {
                    let unused = self
                        .last_used
                        .borrow()
                        .get(&name)
                        .is_none_or(|t| now.duration_since(*t) >= IDLE_TIMEOUT);
                    if away_long {
                        self.close(&name);
                        self.apply(&name, Event::Background, cx);
                    } else if foreground && unused {
                        self.close(&name);
                        self.apply(&name, Event::IdleTimeout, cx);
                    }
                }
                ConnState::Retrying { .. } if away_long => {
                    self.apply(&name, Event::Background, cx);
                }
                ConnState::Retrying { .. } => {
                    self.apply(&name, Event::Tick, cx);
                    // Keeps the "retrying in Ns" countdown current.
                    cx.notify();
                }
                _ => {}
            }
        }
    }

    /// Reloads the schema tree of a connected profile.
    pub fn refresh(&mut self, name: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(client) = self.client(name, false) else {
            self.connect(name, cx);
            return Task::ready(());
        };
        let rx = rt::spawn(cx, async move {
            let schema = client.load_schema().await;
            let route = match &client {
                Conn::Lite(lite) => Some(lite.write_route().await),
                Conn::Pg(_) => None,
            };
            schema.map(|s| (s, route))
        });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok((schema, route)) => {
                            if let Some(route) = route {
                                this.routes.insert(name.clone(), route);
                            }
                            this.schemas.insert(name.clone(), schema);
                            this.schema_rev += 1;
                            this.notices.remove(&name);
                        }
                        Err(e) => {
                            this.notices
                                .insert(name.clone(), format!("Unable to reload the schema. {e}"));
                        }
                    }
                    cx.notify();
                });
            }
        })
    }
}

/// The error line for a failed connect: what to do first (the visible
/// line), then the server's own words (in the tooltip).
fn describe(kind: FailureKind, e: &sawit_core::CoreError) -> String {
    match kind.hint() {
        "" => e.to_string(),
        hint => format!("{hint}\n{e}"),
    }
}
