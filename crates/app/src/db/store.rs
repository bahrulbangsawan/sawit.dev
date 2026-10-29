//! Connection profiles and live connections shared by the database views.
//!
//! Each profile's connection follows [`ConnState`]: idle until something
//! needs it, reconnected with backoff when it drops, and closed again after
//! [`IDLE_TIMEOUT`] unused or [`BACKGROUND_GRACE`] after the app loses
//! focus (so a serverless database such as Neon can scale to zero), but
//! never while a query is using it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{Context, Task};
use sawit_core::db::{
    self, ConnProfile, FailureKind, Profiles, Relation, SchemaInfo, parse_database_url,
    read_dotenv_database_url, resolve_password, store_password,
};
use sawit_core::instances::AppPaths;
use tokio_postgres::Client;

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
    pub clients: HashMap<String, Arc<Client>>,
    /// Read-only connections for the table browser (separate from `clients`
    /// so an open transaction in the SQL editor never affects them).
    pub ro_clients: HashMap<String, Arc<Client>>,
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
            clients: HashMap::new(),
            ro_clients: HashMap::new(),
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
        let alive = |m: &HashMap<String, Arc<Client>>| m.get(name).is_some_and(|c| !c.is_closed());
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
        let held = |m: &HashMap<String, Arc<Client>>| {
            m.get(name).is_some_and(|c| Arc::strong_count(c) > 1)
        };
        held(&self.clients) || held(&self.ro_clients)
    }

    /// A live client for `name` (read-only or read-write), if connected.
    /// Counts as use for the idle timeout.
    pub fn client(&self, name: &str, read_only: bool) -> Option<Arc<Client>> {
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
        self.profiles.items.iter().find(|p| p.name == name)
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
            return;
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
            // Keychain: blocking, and may wait on an access prompt.
            let lookup = profile.clone();
            let password = tokio::task::spawn_blocking(move || resolve_password(&lookup))
                .await
                .unwrap_or_default();
            let (client, ro, schema) = db::connect_all(&profile, password.as_deref()).await?;
            Ok::<_, sawit_core::CoreError>((Arc::new(client), Arc::new(ro), schema))
        });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                if this.epochs.get(&name) != Some(&epoch) {
                    return; // abandoned: disconnected or superseded
                }
                let event = match result {
                    Ok(Ok((client, ro, schema))) => {
                        this.clients.insert(name.clone(), client);
                        this.ro_clients.insert(name.clone(), ro);
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
        let names: Vec<String> = self.profiles.items.iter().map(|p| p.name.clone()).collect();
        for name in names {
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
        let rx = rt::spawn(cx, async move { db::load_schema(&client).await });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            if let Ok(result) = rx.await {
                let _ = this.update(cx, |this, cx| {
                    match result {
                        Ok(schema) => {
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
