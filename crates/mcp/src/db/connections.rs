//! The targets the database tools take (`list_connections`), saved connections (`profiles.toml`,
//! passwords in the macOS Keychain), and DATABASE_URL in a project's `.env` (`write_env`).

use rmcp::model::CallToolResult;
use sawit_core::app_control::Store;
use sawit_core::db::{self, DbKind, parse_database_url, write_database_url};

use super::{SaveConnectionArgs, check_confirm};
use crate::{Sawit, blocking, fail, instance_url, ok, tilde, tri};

/// A `DATABASE_URL` as an agent may see it: without its password, or not at
/// all when it doesn't read as a PostgreSQL URL.
fn shown_url(url: &str) -> String {
    match parse_database_url(url) {
        Ok((profile, _password)) => profile.url(),
        Err(_) => "another value".into(),
    }
}

impl Sawit {
    pub async fn list_connections_tool(&self) -> CallToolResult {
        let mut lines = Vec::new();
        if let Ok(reg) = self.registry() {
            lines.extend(
                reg.instances
                    .iter()
                    .map(|i| format!("- {} (Sawit instance) · {}", i.name, instance_url(i))),
            );
        }
        if let Ok(p) = self.profiles() {
            lines.extend(p.items.iter().map(|p| match p.kind {
                DbKind::Postgres => format!("- {} · {}", p.name, p.url()),
                kind => format!("- {} ({}) · {}", p.name, kind.title(), p.location()),
            }));
        }
        if lines.is_empty() {
            ok("No connections yet.")
        } else {
            ok(lines.join("\n"))
        }
    }

    pub async fn save_connection_tool(&self, a: SaveConnectionArgs) -> CallToolResult {
        let name = a.name.trim().to_string();
        if name.is_empty() {
            return fail("Give the connection a name.");
        }
        if self.instance(&name).is_ok() {
            return fail(format!(
                "\"{name}\" is already a Sawit instance name; choose another so targets stay unambiguous."
            ));
        }
        let (mut profile, password) = tri!(parse_database_url(&a.url).map_err(|e| e.to_string()));
        profile.name = name.clone();
        let mut profiles = tri!(self.profiles());
        let replaced = profiles.items.iter().any(|p| p.name == name);
        if let Some(password) = password {
            let p = profile.clone();
            tri!(
                blocking(move || db::store_password(&p, &password))
                    .await
                    .and_then(|r| r.map_err(|e| {
                        format!("Unable to save the password in the Keychain. {e}")
                    }))
            );
        }
        profiles.upsert(profile.clone());
        tri!(
            profiles
                .save(&self.profiles_file())
                .map_err(|e| e.to_string())
        );
        self.notify(Store::Connections).await;
        ok(format!(
            "{} connection {name} · {}",
            if replaced { "Updated" } else { "Saved" },
            profile.url()
        ))
    }

    pub async fn remove_connection_tool(&self, name: &str, confirm: &str) -> CallToolResult {
        let mut profiles = tri!(self.profiles());
        let Some(profile) = profiles.items.iter().find(|p| p.name == name).cloned() else {
            return fail(format!(
                "No saved connection named \"{name}\". See list_connections."
            ));
        };
        tri!(check_confirm(name, confirm, "remove the saved connection"));
        profiles.remove(name);
        tri!(
            profiles
                .save(&self.profiles_file())
                .map_err(|e| e.to_string())
        );
        // The password is shared by profiles with the same user, host and database.
        let shared = profiles
            .items
            .iter()
            .any(|p| p.keychain_account() == profile.keychain_account());
        if !shared {
            let _ = blocking(move || db::delete_password(&profile)).await;
        }
        self.notify(Store::Connections).await;
        ok(format!("Removed connection {name}."))
    }

    pub async fn write_env_tool(
        &self,
        instance: &str,
        project_dir: &str,
        confirm: bool,
    ) -> CallToolResult {
        // Read as every folder an agent names (MCP-7): a relative one would be sawit-mcp's folder.
        let dir = tri!(crate::existing_dir(project_dir));
        let inst = tri!(self.instance(instance));
        let url = instance_url(&inst);
        let current = tri!(db::read_dotenv(&dir));
        if let Some(old) = db::dotenv_value(&current, "DATABASE_URL").filter(|old| *old != url)
            && !confirm
        {
            return fail(format!(
                "Refused: {} already sets DATABASE_URL to {}, and this would replace it with {url}. Nothing was written. Pass confirm=true to replace it.",
                tilde(&dir.join(".env")),
                shown_url(&old)
            ));
        }
        match write_database_url(&dir, &url) {
            Ok(path) => ok(format!("DATABASE_URL written to {}", tilde(&path))),
            Err(e) => fail(e),
        }
    }
}
