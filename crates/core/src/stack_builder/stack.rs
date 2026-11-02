//! A stack: one pick per category and the project's name, upstream's default stack, and what
//! the rules make of one.

use serde::{Deserialize, Serialize};

use super::{
    Addon, Api, Auth, Backend, Category, DEFAULT_NAME, Database, DbSetup, Deploy, Example,
    NativeFrontend, Orm, Payments, Pm, Runtime, WebFrontend,
};

/// A stack: one pick per category (one web and one native frontend, any number of addons and
/// examples), and the project's name. Missing fields read as upstream's default stack's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stack {
    pub project_name: String,
    pub web_frontend: WebFrontend,
    pub native_frontend: NativeFrontend,
    pub backend: Backend,
    pub runtime: Runtime,
    pub api: Api,
    pub database: Database,
    pub orm: Orm,
    pub db_setup: DbSetup,
    pub web_deploy: Deploy,
    pub server_deploy: Deploy,
    pub auth: Auth,
    pub payments: Payments,
    pub package_manager: Pm,
    /// In the order picked (upstream sorts them by id after a click).
    pub addons: Vec<Addon>,
    pub examples: Vec<Example>,
    pub git: bool,
    pub install: bool,
}

/// Upstream's `DEFAULT_STACK`.
impl Default for Stack {
    fn default() -> Self {
        Self {
            project_name: DEFAULT_NAME.into(),
            web_frontend: WebFrontend::TanstackRouter,
            native_frontend: NativeFrontend::None,
            backend: Backend::Hono,
            runtime: Runtime::Bun,
            api: Api::Trpc,
            database: Database::Sqlite,
            orm: Orm::Drizzle,
            db_setup: DbSetup::None,
            web_deploy: Deploy::None,
            server_deploy: Deploy::None,
            auth: Auth::BetterAuth,
            payments: Payments::None,
            package_manager: Pm::Bun,
            addons: vec![Addon::Turborepo],
            examples: Vec::new(),
            git: true,
            install: true,
        }
    }
}

fn flag(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

impl Stack {
    /// The ids picked in `category`, as upstream holds them: `["none"]` for an empty list.
    pub fn picked(&self, category: Category) -> Vec<&'static str> {
        let one = |id: &'static str| vec![id];
        let list = |ids: Vec<&'static str>| if ids.is_empty() { vec!["none"] } else { ids };
        match category {
            Category::WebFrontend => one(self.web_frontend.id()),
            Category::NativeFrontend => one(self.native_frontend.id()),
            Category::Backend => one(self.backend.id()),
            Category::Runtime => one(self.runtime.id()),
            Category::Api => one(self.api.id()),
            Category::Database => one(self.database.id()),
            Category::Orm => one(self.orm.id()),
            Category::DbSetup => one(self.db_setup.id()),
            Category::WebDeploy => one(self.web_deploy.id()),
            Category::ServerDeploy => one(self.server_deploy.id()),
            Category::Auth => one(self.auth.id()),
            Category::Payments => one(self.payments.id()),
            Category::PackageManager => one(self.package_manager.id()),
            Category::Addons => list(self.addons.iter().map(|a| a.id()).collect()),
            Category::Examples => list(self.examples.iter().map(|e| e.id()).collect()),
            Category::Git => one(flag(self.git)),
            Category::Install => one(flag(self.install)),
        }
    }

    /// Whether option `id` of `category` is picked.
    pub fn is_picked(&self, category: Category, id: &str) -> bool {
        self.picked(category).contains(&id)
    }

    /// Sets `category` to the one option `id` (a list to just that one, `none` to an empty
    /// one). False when `id` isn't one of the category's.
    pub(super) fn set(&mut self, category: Category, id: &str) -> bool {
        fn put<T>(slot: &mut T, value: Option<T>) -> bool {
            value.map(|v| *slot = v).is_some()
        }
        match category {
            Category::WebFrontend => put(&mut self.web_frontend, WebFrontend::parse(id)),
            Category::NativeFrontend => put(&mut self.native_frontend, NativeFrontend::parse(id)),
            Category::Backend => put(&mut self.backend, Backend::parse(id)),
            Category::Runtime => put(&mut self.runtime, Runtime::parse(id)),
            Category::Api => put(&mut self.api, Api::parse(id)),
            Category::Database => put(&mut self.database, Database::parse(id)),
            Category::Orm => put(&mut self.orm, Orm::parse(id)),
            Category::DbSetup => put(&mut self.db_setup, DbSetup::parse(id)),
            Category::WebDeploy => put(&mut self.web_deploy, Deploy::parse(id)),
            Category::ServerDeploy => put(&mut self.server_deploy, Deploy::parse(id)),
            Category::Auth => put(&mut self.auth, Auth::parse(id)),
            Category::Payments => put(&mut self.payments, Payments::parse(id)),
            Category::PackageManager => put(&mut self.package_manager, Pm::parse(id)),
            Category::Addons if id == "none" => {
                self.addons.clear();
                true
            }
            Category::Addons => put(&mut self.addons, Addon::parse(id).map(|a| vec![a])),
            Category::Examples if id == "none" => {
                self.examples.clear();
                true
            }
            Category::Examples => put(&mut self.examples, Example::parse(id).map(|e| vec![e])),
            Category::Git => put(&mut self.git, parse_flag(id)),
            Category::Install => put(&mut self.install, parse_flag(id)),
        }
    }
}

fn parse_flag(id: &str) -> Option<bool> {
    match id {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// One adjustment the rules made, as upstream words it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// The category whose rule made it.
    pub category: Category,
    pub message: String,
}

/// A stack the rules agree with, what they changed to get there, and upstream's desktop build
/// note when it applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved {
    pub stack: Stack,
    pub changes: Vec<Change>,
    /// Notes to show under the command: upstream's desktop build note, when it applies.
    pub notes: Vec<String>,
}
