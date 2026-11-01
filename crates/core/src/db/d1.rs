//! Cloudflare D1 in a Workers project: where Miniflare keeps each binding's
//! local database, and how the database tools name a D1 target.
//!
//! Miniflare (inside `wrangler dev`, `@cloudflare/vite-plugin` and
//! `wrangler d1 execute --local`) stores a local D1 database as the SQLite
//! file of a Durable Object of class `D1DatabaseObject`:
//! `<persist>/v3/d1/miniflare-D1DatabaseObject/<object id>.sqlite`, with
//! `<persist>` the project's `.wrangler/state` unless `--persist-to` names
//! another folder. The object id is workerd's `idFromName` for the binding's
//! id (`preview_database_id`, else `database_id`, else the binding name), in
//! the namespace whose unique key is `miniflare-D1DatabaseObject`:
//! `key = SHA-256(unique key)`, `a = HMAC-SHA-256(key, name)[..16]`,
//! `b = HMAC-SHA-256(key, a)[..16]`, id = hex(a ‖ b). See Miniflare's
//! `durableObjectNamespaceIdFromName` and its D1 plugin, and Wrangler's
//! `d1DatabaseEntry` (https://github.com/cloudflare/workers-sdk, packages
//! `miniflare` and `wrangler`; read in miniflare 4.20260730.0 and wrangler
//! 4.156.0 as published on npm, and checked against the files Wrangler made).
//!
//! A file no binding's id names is listed as unmatched: never a guess.

use std::path::{Path, PathBuf};

use ring::{digest, hmac};

use crate::cloudflare::{Binding, BindingKind, WorkersProject};

/// The Durable Object class Miniflare keeps D1 databases in.
pub const D1_OBJECT_CLASS: &str = "D1DatabaseObject";

/// The folder (and namespace unique key) of local D1 databases.
pub fn d1_unique_key() -> String {
    format!("miniflare-{D1_OBJECT_CLASS}")
}

/// workerd's `idFromName` for a Durable Object namespace with `unique_key`:
/// 64 hex digits.
pub fn durable_object_id(unique_key: &str, name: &str) -> String {
    let key_bytes = digest::digest(&digest::SHA256, unique_key.as_bytes());
    let key = hmac::Key::new(hmac::HMAC_SHA256, key_bytes.as_ref());
    let name_mac = hmac::sign(&key, name.as_bytes());
    let first = &name_mac.as_ref()[..16];
    let mac = hmac::sign(&key, first);
    first
        .iter()
        .chain(&mac.as_ref()[..16])
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The id Miniflare keeps a binding's local data under, as Wrangler's `d1DatabaseEntry` picks
/// it: the preview database id, which `wrangler dev` uses locally, else the database id, else
/// the binding name (a database only ever used locally needs no id).
pub fn miniflare_id(binding: &Binding) -> String {
    (binding.preview_id.clone())
        .or_else(|| binding.id.clone())
        .unwrap_or_else(|| binding.binding.clone())
}

/// The folder a project's local D1 files are in.
pub fn local_d1_dir(project: &WorkersProject) -> PathBuf {
    let persist = match &project.persist_to {
        Some(dir) if dir.is_absolute() => dir.clone(),
        Some(dir) => project.root.join(dir),
        None => project.root.join(".wrangler").join("state"),
    };
    persist.join("v3").join("d1").join(d1_unique_key())
}

/// One D1 binding of a project and its local file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalD1 {
    pub project: PathBuf,
    /// The Worker's name, which the Local Explorer lists it under.
    pub worker: Option<String>,
    pub binding: String,
    /// `database_name`, the name Wrangler's remote commands take.
    pub database_name: Option<String>,
    /// The id the Local Explorer and Miniflare know it by.
    pub miniflare_id: String,
    /// Where its local data is (or will be, once the dev server first runs).
    pub file: PathBuf,
    /// The file is there.
    pub exists: bool,
}

/// What a project's local D1 folder holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocalD1Scan {
    /// One per D1 binding, in config order.
    pub databases: Vec<LocalD1>,
    /// `.sqlite` files in the folder that belong to no binding of the config (a binding since
    /// renamed, another environment's id): listed, never guessed at.
    pub unmatched: Vec<PathBuf>,
}

/// The project's D1 bindings, each with the file Miniflare keeps it in, and the D1 files no
/// binding names. Only reads the folder.
pub fn scan_local(project: &WorkersProject) -> LocalD1Scan {
    let dir = local_d1_dir(project);
    let databases: Vec<LocalD1> = project
        .bindings
        .iter()
        .filter(|b| b.kind == BindingKind::D1)
        .map(|b| {
            let id = miniflare_id(b);
            let file = dir.join(format!(
                "{}.sqlite",
                durable_object_id(&d1_unique_key(), &id)
            ));
            LocalD1 {
                project: project.root.clone(),
                worker: project.name.clone(),
                binding: b.binding.clone(),
                database_name: b.name.clone(),
                miniflare_id: id,
                exists: file.is_file(),
                file,
            }
        })
        .collect();
    let mut unmatched: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_object_file(p) && !databases.iter().any(|d| d.file == *p))
        .collect();
    unmatched.sort();
    LocalD1Scan {
        databases,
        unmatched,
    }
}

/// A Durable Object's SQLite file: 64 hex digits then `.sqlite` (not Miniflare's own
/// `metadata.sqlite`, nor `-wal`/`-shm` files).
fn is_object_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_suffix(".sqlite"))
        .is_some_and(|stem| stem.len() == 64 && stem.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// How the database tools (`detect_databases`, MCP targets, the sidebar) name a D1 database:
/// `d1-local:<binding>@<project folder>`, `d1-remote:<database name>@<project folder>`, or a
/// file no binding names, `sqlite:<path>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum D1Target {
    Local { project: PathBuf, binding: String },
    Remote { project: PathBuf, database: String },
    File(PathBuf),
}

impl D1Target {
    pub fn parse(id: &str) -> Option<D1Target> {
        if let Some(path) = id.strip_prefix("sqlite:") {
            return Some(D1Target::File(PathBuf::from(path)));
        }
        let (kind, rest) = id.split_once(':')?;
        let (name, project) = rest.split_once('@')?;
        if name.is_empty() || project.is_empty() {
            return None;
        }
        let project = PathBuf::from(project);
        match kind {
            "d1-local" => Some(D1Target::Local {
                project,
                binding: name.into(),
            }),
            "d1-remote" => Some(D1Target::Remote {
                project,
                database: name.into(),
            }),
            _ => None,
        }
    }

    pub fn id(&self) -> String {
        match self {
            D1Target::Local { project, binding } => {
                format!("d1-local:{binding}@{}", project.display())
            }
            D1Target::Remote { project, database } => {
                format!("d1-remote:{database}@{}", project.display())
            }
            D1Target::File(path) => format!("sqlite:{}", path.display()),
        }
    }
}

/// The D1 databases of the Workers project in a folder, as profiles the database tools open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    /// Each binding's local database, in config order (kind `d1-local`).
    pub local: Vec<super::ConnProfile>,
    /// Each binding that names a database on Cloudflare (kind `d1-remote`).
    pub remote: Vec<super::ConnProfile>,
    /// Local D1 files no binding names (kind `sqlite`).
    pub unmatched: Vec<super::ConnProfile>,
}

impl Detected {
    pub fn is_empty(&self) -> bool {
        self.local.is_empty() && self.remote.is_empty() && self.unmatched.is_empty()
    }

    /// Local first, then remote, then the unmatched files.
    pub fn all(&self) -> impl Iterator<Item = &super::ConnProfile> {
        self.local.iter().chain(&self.remote).chain(&self.unmatched)
    }
}

/// Finds every D1 database of the Workers project in `dir`. Only reads the config and the
/// local D1 folder; nothing remote.
pub fn detect(dir: &Path) -> Detected {
    let mut out = Detected::default();
    for project in crate::cloudflare::detect_project(dir) {
        let scan = scan_local(&project);
        out.local
            .extend(scan.databases.iter().map(super::ConnProfile::d1_local));
        out.remote.extend(
            project
                .bindings
                .iter()
                .filter(|b| b.kind == BindingKind::D1 && b.id.is_some())
                .filter_map(|b| {
                    let name = b.name.as_deref()?;
                    Some(super::ConnProfile::d1_remote(
                        &project.root,
                        &b.binding,
                        name,
                    ))
                }),
        );
        out.unmatched.extend(
            scan.unmatched
                .iter()
                .map(|f| super::ConnProfile::sqlite_file(f)),
        );
    }
    out
}

/// The profile for a target id ([`D1Target`]), read from the project's config now.
pub fn resolve(id: &str) -> Option<std::result::Result<super::ConnProfile, String>> {
    let target = D1Target::parse(id)?;
    Some(match target {
        D1Target::Local { project, binding } => find_local(&project, &binding)
            .map(|d| super::ConnProfile::d1_local(&d))
            .ok_or_else(|| {
                format!(
                    "The Wrangler config in {} has no D1 binding “{binding}”. See detect_databases.",
                    project.display()
                )
            }),
        D1Target::Remote { project, database } => {
            let binding = crate::cloudflare::detect_project(&project)
                .into_iter()
                .flat_map(|p| p.bindings)
                .find(|b| b.kind == BindingKind::D1 && b.name.as_deref() == Some(&database))
                .map(|b| b.binding)
                .unwrap_or_else(|| database.clone());
            Ok(super::ConnProfile::d1_remote(&project, &binding, &database))
        }
        D1Target::File(path) if path.is_file() => Ok(super::ConnProfile::sqlite_file(&path)),
        D1Target::File(path) => Err(format!("{} doesn't exist.", path.display())),
    })
}

/// Finds the local D1 database `binding` of the Workers project in `project`.
pub fn find_local(project: &Path, binding: &str) -> Option<LocalD1> {
    crate::cloudflare::detect_project(project)
        .iter()
        .flat_map(|p| scan_local(p).databases)
        .find(|d| d.binding == binding)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(root: &Path, bindings: &[(&str, Option<&str>)]) -> WorkersProject {
        WorkersProject {
            root: root.to_path_buf(),
            config: root.join("wrangler.toml"),
            tool: crate::cloudflare::DevTool::Wrangler,
            version: None,
            name: Some("api".into()),
            bindings: bindings
                .iter()
                .map(|(b, id)| Binding {
                    kind: BindingKind::D1,
                    binding: b.to_string(),
                    name: Some(format!("{b}-db")),
                    id: id.map(str::to_string),
                    preview_id: None,
                })
                .collect(),
            persist_to: None,
        }
    }

    /// Miniflare keeps a binding's data under its preview database id when it has one (what
    /// `wrangler dev` uses locally), else its database id, else its binding name.
    #[test]
    fn the_preview_database_id_names_the_local_data_first() {
        let mut b = Binding {
            kind: BindingKind::D1,
            binding: "DB".into(),
            name: Some("shop".into()),
            id: Some("prod-1".into()),
            preview_id: Some("dev-1".into()),
        };
        assert_eq!(miniflare_id(&b), "dev-1");
        b.preview_id = None;
        assert_eq!(miniflare_id(&b), "prod-1");
        b.id = None;
        assert_eq!(miniflare_id(&b), "DB");
    }

    /// File names Wrangler 4.156.0 (Miniflare 4.20260730.0, workerd) actually made, with
    /// `wrangler d1 execute <binding> --local`, in a project whose `DB` has
    /// `database_id = "5d0c3fa0-9a64-4b0f-9a2a-2f5c1a8e7c11"` and whose `CACHE` has no id.
    #[test]
    fn object_ids_match_files_wrangler_made() {
        let key = d1_unique_key();
        assert_eq!(key, "miniflare-D1DatabaseObject");
        assert_eq!(
            durable_object_id(&key, "5d0c3fa0-9a64-4b0f-9a2a-2f5c1a8e7c11"),
            "b9476203485ab274a75ecb0e2aee0b3001be937781a280511b4522aaa1e0147e"
        );
        assert_eq!(
            durable_object_id(&key, "CACHE"),
            "551e99ef28a64df148b854a511d0a6d4bd76f2ded962b92a0352b84d0be0d000"
        );
    }

    #[test]
    fn finds_each_bindings_file_and_lists_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let p = project(dir.path(), &[("DB", Some("5d0c3fa0")), ("CACHE", None)]);
        let folder = local_d1_dir(&p);
        assert_eq!(
            folder,
            dir.path()
                .join(".wrangler/state/v3/d1/miniflare-D1DatabaseObject")
        );
        std::fs::create_dir_all(&folder).unwrap();
        let db = durable_object_id(&d1_unique_key(), "5d0c3fa0");
        let stray = "ab".repeat(32);
        for name in [
            format!("{db}.sqlite"),
            format!("{db}.sqlite-wal"),
            format!("{stray}.sqlite"),
            "metadata.sqlite".into(),
        ] {
            std::fs::write(folder.join(name), b"").unwrap();
        }
        let scan = scan_local(&p);
        assert_eq!(scan.databases.len(), 2);
        assert!(scan.databases[0].exists);
        assert_eq!(scan.databases[0].file, folder.join(format!("{db}.sqlite")));
        // CACHE has no id: Miniflare uses its binding name, and it hasn't run yet.
        assert_eq!(scan.databases[1].miniflare_id, "CACHE");
        assert!(!scan.databases[1].exists);
        assert_eq!(scan.unmatched, [folder.join(format!("{stray}.sqlite"))]);
    }

    #[test]
    fn persist_to_moves_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = project(&dir.path().join("app"), &[("DB", None)]);
        p.persist_to = Some(PathBuf::from("../state"));
        assert_eq!(
            local_d1_dir(&p),
            dir.path()
                .join("app/../state/v3/d1/miniflare-D1DatabaseObject")
        );
        let folder = local_d1_dir(&p);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(format!(
            "{}.sqlite",
            durable_object_id(&d1_unique_key(), "DB")
        ));
        std::fs::write(&file, b"").unwrap();
        assert!(scan_local(&p).databases[0].exists);
        p.persist_to = Some(dir.path().join("abs"));
        assert!(local_d1_dir(&p).starts_with(dir.path().join("abs/v3/d1")));
    }

    #[test]
    fn target_ids_round_trip() {
        for id in [
            "d1-local:DB@/Users/me/api",
            "d1-remote:shop@/Users/me/api",
            "sqlite:/tmp/x.sqlite",
        ] {
            assert_eq!(D1Target::parse(id).unwrap().id(), id);
        }
        assert_eq!(D1Target::parse("prod"), None);
        assert_eq!(D1Target::parse("d1-local:@/x"), None);
    }
}
