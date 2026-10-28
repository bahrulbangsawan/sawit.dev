//! Connection profiles and live connections shared by the database views.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use gpui_kit::{Context, Task};
use pgdesk_core::db::{
    self, ConnProfile, Profiles, SchemaInfo, load_password, parse_database_url,
    read_dotenv_database_url, store_password,
};
use pgdesk_core::instances::AppPaths;
use tokio_postgres::Client;

use crate::rt;

pub struct DbStore {
    file: PathBuf,
    pub profiles: Profiles,
    pub clients: HashMap<String, Arc<Client>>,
    pub schemas: HashMap<String, Vec<SchemaInfo>>,
    pub connecting: HashSet<String>,
    pub errors: HashMap<String, String>,
    pub notice: Option<String>,
}

impl DbStore {
    pub fn new(_: &mut Context<Self>) -> Self {
        let file = AppPaths::default_macos().root.join("profiles.toml");
        let profiles = Profiles::load(&file).unwrap_or_default();
        Self {
            file,
            profiles,
            clients: HashMap::new(),
            schemas: HashMap::new(),
            connecting: HashSet::new(),
            errors: HashMap::new(),
            notice: None,
        }
    }

    fn save(&mut self) {
        if let Err(e) = self.profiles.save(&self.file) {
            self.notice = Some(format!("Gagal menyimpan profil: {e}"));
        }
    }

    pub fn profile(&self, name: &str) -> Option<&ConnProfile> {
        self.profiles.items.iter().find(|p| p.name == name)
    }

    /// Adds a profile from a DATABASE_URL; its password goes to the Keychain.
    pub fn add_url(&mut self, url: &str, cx: &mut Context<Self>) -> Result<String, String> {
        let (profile, password) = parse_database_url(url).map_err(|e| e.to_string())?;
        if let Some(pw) = password {
            store_password(&profile, &pw).map_err(|e| e.to_string())?;
        }
        let name = profile.name.clone();
        self.profiles.upsert(profile);
        self.save();
        self.connect(&name, cx);
        Ok(name)
    }

    pub fn import_env(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<String, String> {
        match read_dotenv_database_url(&path) {
            Ok(Some(url)) => self.add_url(&url, cx),
            Ok(None) => Err(format!("{} tidak berisi DATABASE_URL.", path.display())),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn add_profile(&mut self, profile: ConnProfile, cx: &mut Context<Self>) {
        let name = profile.name.clone();
        self.profiles.upsert(profile);
        self.save();
        self.connect(&name, cx);
    }

    pub fn remove(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(p) = self.profile(name).cloned() {
            let _ = db::delete_password(&p);
        }
        self.profiles.remove(name);
        self.clients.remove(name);
        self.schemas.remove(name);
        self.save();
        cx.notify();
    }

    pub fn connect(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(profile) = self.profile(name).cloned() else {
            return;
        };
        self.connecting.insert(name.to_string());
        self.errors.remove(name);
        cx.notify();
        let rx = rt::spawn(cx, async move {
            let password = load_password(&profile).ok().flatten();
            let client = db::connect(&profile, password.as_deref()).await?;
            let schema = db::load_schema(&client).await?;
            Ok::<_, pgdesk_core::CoreError>((Arc::new(client), schema))
        });
        let name = name.to_string();
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.connecting.remove(&name);
                match result {
                    Ok(Ok((client, schema))) => {
                        this.clients.insert(name.clone(), client);
                        this.schemas.insert(name.clone(), schema);
                    }
                    Ok(Err(e)) => {
                        this.errors.insert(name.clone(), e.to_string());
                    }
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Reloads the schema tree of a connected profile.
    pub fn refresh(&mut self, name: &str, cx: &mut Context<Self>) -> Task<()> {
        let Some(client) = self.clients.get(name).cloned() else {
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
                        }
                        Err(e) => {
                            this.errors.insert(name.clone(), e.to_string());
                        }
                    }
                    cx.notify();
                });
            }
        })
    }
}
