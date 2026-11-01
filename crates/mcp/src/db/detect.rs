//! Finding databases: every one reachable from this Mac (`detect_databases`), and the databases on
//! a server or of a D1 project (`list_databases`).

use rmcp::model::CallToolResult;
use sawit_core::db::{self, ConnProfile, DbKind};
use sawit_core::kebun::{self, Kebun};

use super::Target;
use crate::{Sawit, blocking, fail, instance_url, ok, tilde, tri};

/// One D1 or SQLite database as a `detect_databases` line.
fn lite_line(p: &ConnProfile) -> String {
    let lite = p.lite.clone().unwrap_or_default();
    let mut parts = vec![format!("- {}", p.name), format!("kind: {}", p.kind.id())];
    if let Some(binding) = &lite.binding {
        parts.push(format!("binding {binding}"));
    }
    if let Some(name) = &lite.database_name {
        parts.push(format!("database {name}"));
    }
    match (p.kind, p.file()) {
        (DbKind::D1Remote, _) => parts.push("on Cloudflare, through your Wrangler login".into()),
        (_, Some(file)) if file.is_file() => parts.push(tilde(file)),
        (_, Some(file)) => parts.push(format!(
            "{} (no local data yet: the dev server makes it)",
            tilde(file)
        )),
        _ => {}
    }
    parts.join(" · ")
}

/// One detected database as a line.
fn kebun_line(k: &Kebun) -> String {
    let p = &k.profile;
    format!(
        "- {} · kind: postgres · {}:{} · db {} · user {} · via {}",
        p.name, p.host, p.port, p.database, p.user, k.via
    )
}

/// The `via` labels of `sawit_core::kebun` in English.
pub(super) fn english_via(via: &str) -> String {
    if let Some(name) = via.strip_prefix("koneksi tersimpan ") {
        format!("saved connection {name}")
    } else if let Some(dir) = via.strip_prefix(".env di ") {
        format!(".env in {dir}")
    } else {
        via.to_string()
    }
}

impl Sawit {
    pub async fn detect_databases_tool(&self, project_dir: Option<&str>) -> CallToolResult {
        let registry = self.registry().unwrap_or_default();
        let mut out = Vec::new();
        if !registry.instances.is_empty() {
            out.push("Sawit instances:".to_string());
            for inst in &registry.instances {
                out.push(format!(
                    "- {} · localhost:{} · {}",
                    inst.name,
                    inst.port,
                    instance_url(inst)
                ));
            }
        }
        let paths = self.paths.clone();
        let mut found: Vec<Kebun> = tri!(blocking(move || kebun::saved_profiles(&paths)).await);
        let mut d1 = db::d1::Detected::default();
        if let Some(dir) = project_dir.filter(|d| !d.trim().is_empty()) {
            let dir = tri!(crate::existing_dir(dir));
            let env_dir = dir.clone();
            found.extend(tri!(blocking(move || kebun::dotenv_kebuns(&env_dir)).await));
            d1 = tri!(blocking(move || db::d1::detect(&dir)).await);
        }
        found.extend(kebun::local_kebuns(&registry).await);
        let mut unique: Vec<Kebun> = Vec::new();
        for mut k in found {
            let p = &k.profile;
            if unique.iter().any(|u| {
                u.profile.host == p.host
                    && u.profile.port == p.port
                    && u.profile.database == p.database
            }) {
                continue;
            }
            k.via = english_via(&k.via);
            unique.push(k);
        }
        if !unique.is_empty() {
            out.push("Databases (running servers list every database):".to_string());
            out.extend(unique.iter().map(kebun_line));
        }
        if !d1.local.is_empty() || !d1.remote.is_empty() {
            out.push(
                "Cloudflare D1 (pass the first field as `target`; local first, then remote):"
                    .to_string(),
            );
            out.extend(d1.local.iter().chain(&d1.remote).map(lite_line));
        }
        if !d1.unmatched.is_empty() {
            out.push("Unmatched local D1 databases (no binding of the config names them):".into());
            out.extend(d1.unmatched.iter().map(lite_line));
        }
        if out.is_empty() {
            return ok(
                "No databases found: no Sawit instances, saved connections, .env DATABASE_URL, Wrangler D1 bindings or running local Postgres.",
            );
        }
        ok(out.join("\n"))
    }

    pub async fn list_databases_tool(&self, target: &str) -> CallToolResult {
        if let Target::Lite(p) = tri!(self.resolve_target(target)) {
            let lite = p.lite.clone().unwrap_or_default();
            return match (p.kind, lite.project) {
                // The account's D1 databases, as `wrangler d1 list` shows them.
                (DbKind::D1Remote, Some(project)) => {
                    let Some(wrangler) = db::remote::wrangler(&project) else {
                        return fail("Wrangler isn't installed in this project or on this Mac.");
                    };
                    match db::remote::list(&wrangler, &project).await {
                        Ok(all) if all.is_empty() => ok("No D1 databases on this account."),
                        Ok(all) => ok(all
                            .iter()
                            .map(|d| {
                                format!("- d1-remote:{}@{} · {}", d.name, project.display(), d.uuid)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")),
                        Err(e) => fail(e.to_string()),
                    }
                }
                // The project's D1 databases.
                (DbKind::D1Local, Some(project)) => {
                    let d1 = tri!(blocking(move || db::d1::detect(&project)).await);
                    ok(d1.all().map(lite_line).collect::<Vec<_>>().join("\n"))
                }
                // A file holds one database.
                _ => ok(lite_line(&p)),
            };
        }
        let client = tri!(self.connect_admin(target, None).await);
        match db::list_databases(&client).await {
            Ok(names) if names.is_empty() => ok("No databases."),
            Ok(names) => ok(names
                .iter()
                .map(|n| format!("- {n}"))
                .collect::<Vec<_>>()
                .join("\n")),
            Err(e) => fail(e.to_string()),
        }
    }
}
