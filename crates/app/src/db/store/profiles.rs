//! The profiles: adding them (a URL, a .env file, a SQLite file), changing and removing them,
//! reading them again after another process changed them, and finding the D1 databases of the
//! window's Workers projects.

use std::path::{Path, PathBuf};

use gpui_kit::Context;
use sawit_core::db::{
    self, ConnProfile, Profiles, parse_database_url, read_dotenv_database_url, store_password,
};

use super::DbStore;
use crate::rt;

impl DbStore {
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
}
