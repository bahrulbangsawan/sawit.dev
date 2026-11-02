//! Finding the database `sawit <db>` names: a Sawit instance, a saved connection, a `.env`
//! DATABASE_URL or a database on a running local Postgres.

use std::path::Path;

use crate::db::{
    self, ConnProfile, Profiles, parse_database_url, read_dotenv_database_url, resolve_password,
};
use crate::instances::{AppPaths, Registry};
use crate::ports::PortJob;
use crate::probe;

use super::ink::tilde;

/// A database `sawit <db>` can connect to, and where it was found.
#[derive(Debug, Clone)]
pub struct Kebun {
    pub profile: ConnProfile,
    pub password: Option<String>,
    /// "instance shop", "koneksi tersimpan", ".env (…)", "Postgres localhost:5432".
    pub via: String,
}

impl Kebun {
    fn same_place(&self, other: &Kebun) -> bool {
        let (a, b) = (&self.profile, &other.profile);
        a.host == b.host && a.port == b.port && a.database == b.database
    }
}

/// Finds `name`: a Sawit instance or saved connection with that name, else
/// a database called `name` in a saved connection, a `.env` DATABASE_URL in
/// `cwd` or above, or a running local Postgres. Failing that, `kyzn` finds
/// `kyzn_crm` when no other database starts with it. Only the one found has
/// its saved password read: each read asks the Keychain (`security`) twice.
pub async fn find(name: &str, paths: &AppPaths, cwd: &Path) -> Result<Option<Kebun>, String> {
    let registry = Registry::load(&paths.instances_file()).unwrap_or_default();
    if let Some(inst) = registry.get(name) {
        return Ok(Some(Kebun {
            profile: ConnProfile::new("localhost", inst.port, "postgres", "postgres"),
            password: None,
            via: format!("instance {}", inst.name),
        }));
    }
    let profiles = saved_places(paths);
    if let Some(found) = profiles.iter().find(|k| k.profile.name == name) {
        return Ok(Some(with_password(found.clone())));
    }
    let mut candidates = profiles;
    candidates.extend(dotenv_places(cwd));
    if let Some(found) = candidates.iter().find(|k| k.profile.database == name) {
        return Ok(Some(with_password(found.clone())));
    }
    // The saved and `.env` ones come first; the local ones have read theirs to list databases.
    let unread = candidates.len();
    candidates.extend(local_kebuns(&registry).await);
    let pick = |ix: usize| match ix < unread {
        true => with_password(candidates[ix].clone()),
        false => candidates[ix].clone(),
    };
    if let Some(ix) = candidates.iter().position(|k| k.profile.database == name) {
        return Ok(Some(pick(ix)));
    }
    let mut partial: Vec<usize> = Vec::new();
    for (ix, k) in candidates.iter().enumerate() {
        if k.profile.database.starts_with(name)
            && !partial.iter().any(|&p| candidates[p].same_place(k))
        {
            partial.push(ix);
        }
    }
    match partial.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(pick(*one))),
        many => Err(format!(
            "“{name}” cocok dengan {} kebun — sengketa lahan:\n{}\nSebut nama lengkapnya. Batas konsesi harus jelas, tidak seperti di peta resmi.",
            many.len(),
            many.iter()
                .map(|&ix| &candidates[ix])
                .map(|k| format!("   · {} ({})", k.profile.database, k.via))
                .collect::<Vec<_>>()
                .join("\n")
        )),
    }
}

pub fn saved_profiles(paths: &AppPaths) -> Vec<Kebun> {
    saved_places(paths).into_iter().map(with_password).collect()
}

/// [`saved_profiles`] without their passwords: where each connection goes, for what never shows
/// a password (`sawit list`) and for [`find`], which reads the password of the one it picks.
pub(super) fn saved_places(paths: &AppPaths) -> Vec<Kebun> {
    let Ok(profiles) = Profiles::load(&paths.root.join("profiles.toml")) else {
        return Vec::new();
    };
    profiles
        .items
        .into_iter()
        // SQLite and D1 profiles have no server to find.
        .filter(|profile| profile.kind.is_postgres())
        .map(|profile| Kebun {
            password: None,
            via: format!("koneksi tersimpan {}", profile.name),
            profile,
        })
        .collect()
}

/// DATABASE_URL from `.env` in `cwd` and every folder above it.
pub fn dotenv_kebuns(cwd: &Path) -> Vec<Kebun> {
    dotenv_places(cwd).into_iter().map(with_password).collect()
}

/// [`dotenv_kebuns`] with only the passwords their URLs hold, as [`saved_places`].
pub(super) fn dotenv_places(cwd: &Path) -> Vec<Kebun> {
    cwd.ancestors()
        .map(|dir| dir.join(".env"))
        .filter_map(|path| {
            let url = read_dotenv_database_url(&path).ok()??;
            let (profile, password) = parse_database_url(&url).ok()?;
            Some(Kebun {
                password,
                via: format!(".env di {}", tilde(path.parent()?)),
                profile,
            })
        })
        .collect()
}

/// `kebun` with the password the Keychain or `~/.pgpass` keeps for its profile, unless it has one.
fn with_password(mut kebun: Kebun) -> Kebun {
    if kebun.password.is_none() {
        kebun.password = resolve_password(&kebun.profile);
    }
    kebun
}

/// Every database of the Postgres servers running here: Sawit instances,
/// then any other `postgres` listening on a port (Homebrew, Postgres.app).
pub async fn local_kebuns(registry: &Registry) -> Vec<Kebun> {
    let jobs = probe::listening_jobs().await.unwrap_or_default();
    local_kebuns_in(registry, &jobs).await
}

/// [`local_kebuns`] with `jobs`, what [`probe::listening_jobs`] found listening.
pub(super) async fn local_kebuns_in(registry: &Registry, jobs: &[PortJob]) -> Vec<Kebun> {
    let mut servers: Vec<(u16, String)> = registry
        .instances
        .iter()
        .map(|i| (i.port, format!("instance {}", i.name)))
        .collect();
    for job in jobs {
        let postgres = job
            .listeners
            .iter()
            .any(|(_, c)| c.starts_with("postgres") || c.starts_with("postmaster"));
        for (port, _) in job.ports.iter().filter(|_| postgres) {
            if !servers.iter().any(|(p, _)| p == port) {
                servers.push((*port, format!("Postgres localhost:{port}")));
            }
        }
    }
    let user = std::env::var("USER").unwrap_or_default();
    let lookups = servers.into_iter().map(|(port, via)| {
        let user = user.clone();
        async move {
            for user in ["postgres", user.as_str()] {
                let mut admin = ConnProfile::new("localhost", port, "postgres", user);
                admin.connect_timeout = Some(2);
                let password = resolve_password(&admin);
                let Ok(client) = db::connect(&admin, password.as_deref()).await else {
                    continue;
                };
                let names = db::list_databases(&client).await.unwrap_or_default();
                return names
                    .into_iter()
                    .map(|database| Kebun {
                        profile: ConnProfile {
                            name: format!("{user}@localhost:{port}/{database}"),
                            database,
                            ..admin.clone()
                        },
                        password: password.clone(),
                        via: via.clone(),
                    })
                    .collect();
            }
            Vec::new()
        }
    });
    futures::future::join_all(lookups)
        .await
        .into_iter()
        .flatten()
        .collect()
}
