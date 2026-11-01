//! Each profile's connection: the state machine fed with events, opening and closing, closing
//! when idle or in the background, noticing drops and reconnecting, and reloading the schema.

use std::time::Instant;

use gpui_kit::{Context, Task};
use sawit_core::db::{self, Conn, FailureKind, resolve_password};

use super::{BACKGROUND_GRACE, DbStore, IDLE_TIMEOUT};
use crate::db::lifecycle::{ConnState, Event};
use crate::rt;

impl DbStore {
    /// Drops the store's handles; the connection closes once no running
    /// request holds it. A connect under way is abandoned.
    pub(super) fn close(&mut self, name: &str) {
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
    pub(super) fn tick(&mut self, cx: &mut Context<Self>) {
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
