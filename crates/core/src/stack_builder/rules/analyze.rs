//! Upstream's `analyzeStackCompatibility`: one pass of the auto-adjustments, branch for branch,
//! with upstream's messages.

use super::compat::{
    CLERK_BACKEND_MESSAGE, CLERK_FRONTEND_MESSAGE, CLOUDFLARE_NEXT_MESSAGE, PRISMA_COMPUTE_WEB,
    addon_issue, backend_disabled, cloudflare_next_issue, convex_ai_incompatible,
    convex_better_auth_compatible, docker_desktop_conflict, example_ai_allowed,
    example_todo_allowed, is_frontend_allowed_with_backend, prisma_desktop_conflict,
    setup_databases, supports_clerk_backend, supports_clerk_frontend, supports_db_setup,
    supports_db_setup_runtime, supports_orm_database, supports_runtime_backend,
    supports_runtime_database, supports_server_deploy_runtime, trpc_allowed,
};
use super::{core_backend, self_frontend};
use crate::stack_builder::{
    Addon, Api, Auth, Backend, Category, Change, Database, DbSetup, Deploy, Example, Orm, Payments,
    Runtime, Stack, WebFrontend,
};

/// Sets `category`'s single pick to none, if it isn't: whether it changed.
fn set_none(stack: &mut Stack, category: Category) -> bool {
    if stack.picked(category) == ["none"] {
        return false;
    }
    stack.set(category, "none")
}

/// One pass of upstream's `analyzeStackCompatibility`: the stack it adjusts to, with its
/// changes, or `None` when nothing needed adjusting.
pub(super) fn analyze(stack: &Stack) -> Option<(Stack, Vec<Change>)> {
    let mut next = stack.clone();
    let mut changes = Vec::new();
    let mut change = |category: Category, message: String| {
        changes.push(Change { category, message });
    };

    for &key in backend_disabled(core_backend(next.backend)) {
        if set_none(&mut next, key) {
            change(
                Category::Backend,
                format!(
                    "{} set to 'none' (managed by {})",
                    key.display_name(),
                    next.backend.id()
                ),
            );
        }
    }

    if next.backend == Backend::Convex {
        if !is_frontend_allowed_with_backend(next.web_frontend.id(), "convex", None) {
            next.web_frontend = WebFrontend::None;
            change(
                Category::Backend,
                "Removed incompatible Convex frontends".into(),
            );
        }
        if next.auth == Auth::BetterAuth && !convex_better_auth_compatible(&next) {
            next.auth = Auth::None;
            change(
                Category::Auth,
                "Auth set to 'None' (Better-Auth with Convex requires compatible frontend)".into(),
            );
        }
    }

    if next.backend == Backend::None && !next.examples.is_empty() {
        next.examples.clear();
        change(Category::Backend, "Examples cleared (no backend)".into());
    }

    if let Some(frontend) = self_frontend(next.backend)
        && next.web_frontend != frontend
    {
        next.web_frontend = frontend;
        change(
            Category::Backend,
            format!(
                "Frontend set to '{}' (required for fullstack backend)",
                frontend.id()
            ),
        );
    }

    if next.runtime == Runtime::Workers
        && !supports_runtime_backend(next.runtime, core_backend(next.backend))
    {
        next.backend = Backend::Hono;
        change(
            Category::Runtime,
            "Backend set to 'Hono' (required for Workers)".into(),
        );
    }

    if next.runtime == Runtime::Workers && next.server_deploy == Deploy::None {
        next.server_deploy = Deploy::Cloudflare;
        change(
            Category::Runtime,
            "Server deploy set to 'Cloudflare' (required for Workers)".into(),
        );
    }

    if !supports_runtime_database(next.runtime, next.database) {
        next.database = Database::Sqlite;
        next.orm = Orm::Drizzle;
        next.db_setup = DbSetup::D1;
        change(
            Category::Runtime,
            "Database changed to SQLite with D1 (MongoDB incompatible with Workers)".into(),
        );
    }

    if next.runtime == Runtime::None
        && !supports_runtime_backend(next.runtime, core_backend(next.backend))
    {
        next.runtime = Stack::default().runtime;
        change(
            Category::Runtime,
            format!(
                "Runtime set to '{}' (required for this backend)",
                next.runtime.id()
            ),
        );
    }

    if next.backend != Backend::Convex && next.backend != Backend::None {
        if next.database == Database::None {
            if next.orm != Orm::None {
                next.orm = Orm::None;
                change(
                    Category::Database,
                    "ORM set to 'None' (no database selected)".into(),
                );
            }
            if next.db_setup != DbSetup::None {
                next.db_setup = DbSetup::None;
                change(
                    Category::Database,
                    "DB Setup set to 'None' (no database selected)".into(),
                );
            }
        }

        if next.database == Database::Mongodb {
            if !supports_orm_database(next.orm, next.database) {
                next.orm = Orm::Prisma;
                change(
                    Category::Database,
                    "ORM set to 'Prisma' (required for MongoDB)".into(),
                );
            }
            if !supports_db_setup(next.db_setup, next.database) {
                next.db_setup = DbSetup::None;
                change(
                    Category::Database,
                    "DB Setup set to 'None' (incompatible with MongoDB)".into(),
                );
            }
        }

        if supports_orm_database(Orm::Drizzle, next.database) {
            if next.orm == Orm::None {
                next.orm = Orm::Drizzle;
                change(
                    Category::Database,
                    "ORM set to 'Drizzle' (required for database)".into(),
                );
            }
            if next.orm == Orm::Mongoose {
                next.orm = Orm::Drizzle;
                change(
                    Category::Database,
                    "ORM set to 'Drizzle' (Mongoose only works with MongoDB)".into(),
                );
            }
        }

        if next.orm != Orm::None && next.database == Database::None {
            if next.orm == Orm::Mongoose {
                next.database = Database::Mongodb;
                change(
                    Category::Orm,
                    "Database set to 'MongoDB' (required for Mongoose)".into(),
                );
            } else {
                next.database = Database::Sqlite;
                change(
                    Category::Orm,
                    "Database set to 'SQLite' (required for ORM)".into(),
                );
            }
        }

        if next.db_setup != DbSetup::Docker
            && !supports_db_setup(next.db_setup, next.database)
            && let Some(&database) = setup_databases(next.db_setup).first()
        {
            next.database = database;
            change(
                Category::DbSetup,
                format!(
                    "Database set to '{}' (required for {})",
                    database.id(),
                    next.db_setup.id()
                ),
            );
        }

        if next.db_setup == DbSetup::D1 {
            if self_frontend(next.backend).is_some() {
                if next.web_deploy != Deploy::Cloudflare {
                    next.web_deploy = Deploy::Cloudflare;
                    change(
                        Category::DbSetup,
                        "Web deploy set to 'Cloudflare' (required for D1 with fullstack backend)"
                            .into(),
                    );
                }
            } else {
                if next.runtime != Runtime::Workers || next.backend != Backend::Hono {
                    next.runtime = Runtime::Workers;
                    next.backend = Backend::Hono;
                    change(
                        Category::DbSetup,
                        "Runtime set to 'Workers' with 'Hono' (required for D1)".into(),
                    );
                }
                if next.server_deploy != Deploy::Cloudflare {
                    next.server_deploy = Deploy::Cloudflare;
                    change(
                        Category::DbSetup,
                        "Server deploy set to 'Cloudflare' (required for D1 with Workers)".into(),
                    );
                }
            }
        }

        if next.db_setup == DbSetup::Docker {
            if !supports_db_setup(next.db_setup, next.database) {
                next.db_setup = DbSetup::None;
                change(
                    Category::DbSetup,
                    "DB Setup set to 'None' (SQLite doesn't need Docker)".into(),
                );
            }
            if !supports_db_setup_runtime(DbSetup::Docker, next.runtime, core_backend(next.backend))
            {
                next.db_setup = DbSetup::D1;
                change(
                    Category::DbSetup,
                    "DB Setup set to 'D1' (Docker incompatible with Workers)".into(),
                );
            }
        }
    }

    if next.backend != Backend::Convex
        && next.backend != Backend::None
        && !trpc_allowed(next.web_frontend)
        && next.api == Api::Trpc
    {
        next.api = Api::Orpc;
        change(
            Category::Api,
            "API set to 'oRPC' (required for this frontend)".into(),
        );
    }

    if next.auth == Auth::Clerk {
        if !supports_clerk_backend(core_backend(next.backend), next.web_frontend) {
            next.auth = Auth::None;
            change(
                Category::Auth,
                format!("Auth set to 'None' ({CLERK_BACKEND_MESSAGE})"),
            );
        } else if !supports_clerk_frontend(&next) {
            next.auth = Auth::None;
            change(
                Category::Auth,
                format!("Auth set to 'None' ({CLERK_FRONTEND_MESSAGE})"),
            );
        }
    }

    if next.payments == Payments::Polar && next.auth != Auth::BetterAuth {
        next.payments = Payments::None;
        change(
            Category::Payments,
            "Payments set to 'None' (Polar requires Better Auth)".into(),
        );
    }

    let incompatible: Vec<(Addon, String)> = next
        .addons
        .iter()
        .filter_map(|&addon| Some((addon, addon_issue(&next, addon)?)))
        .collect();
    if !incompatible.is_empty() {
        next.addons
            .retain(|addon| !incompatible.iter().any(|(a, _)| a == addon));
        for (addon, issue) in incompatible {
            change(
                Category::Addons,
                format!("{} removed ({issue})", addon.id()),
            );
        }
    }

    if next.examples.contains(&Example::Todo)
        && next.backend != Backend::Convex
        && !example_todo_allowed(core_backend(next.backend), next.database, next.api)
    {
        let reason = if next.database == Database::None {
            "requires database"
        } else {
            "requires API layer"
        };
        next.examples.retain(|e| *e != Example::Todo);
        change(Category::Examples, format!("Todo removed ({reason})"));
    }

    if next.examples.contains(&Example::Ai) {
        if !example_ai_allowed(next.web_frontend) {
            next.examples.retain(|e| *e != Example::Ai);
            change(
                Category::Examples,
                "AI removed (not compatible with Solid or Astro frontend)".into(),
            );
        }
        if next.backend == Backend::Convex && convex_ai_incompatible(next.web_frontend) {
            next.examples.retain(|e| *e != Example::Ai);
            change(
                Category::Examples,
                "AI removed (Convex AI only supports React-based frontends)".into(),
            );
        }
    }

    if next.web_deploy != Deploy::None && next.web_frontend == WebFrontend::None {
        next.web_deploy = Deploy::None;
        change(
            Category::WebDeploy,
            "Web deploy set to 'None' (no web frontend)".into(),
        );
    }

    if next.web_deploy == Deploy::Docker
        && let Some((frontend, addons)) = docker_desktop_conflict(&next)
    {
        next.web_deploy = Deploy::None;
        change(
            Category::WebDeploy,
            format!(
                "Web deploy set to 'None' ({} requires a static {frontend} build)",
                addons.join(" and ")
            ),
        );
    }

    if next.web_deploy == Deploy::Prisma && !PRISMA_COMPUTE_WEB.contains(&next.web_frontend.id()) {
        next.web_deploy = Deploy::None;
        change(
            Category::WebDeploy,
            "Web deploy set to 'None' (Prisma requires a supported web frontend)".into(),
        );
    }

    if next.web_deploy == Deploy::Prisma
        && let Some((frontend, addons)) = prisma_desktop_conflict(&next.addons, next.web_frontend)
    {
        next.web_deploy = Deploy::None;
        change(
            Category::WebDeploy,
            format!(
                "Web deploy set to 'None' ({} requires a static {frontend} build)",
                addons.join(" and ")
            ),
        );
    }

    if cloudflare_next_issue(&next, next.web_deploy) {
        next.web_deploy = Deploy::None;
        change(
            Category::WebDeploy,
            format!("Web deploy set to 'None' ({CLOUDFLARE_NEXT_MESSAGE})"),
        );
    }

    if next.server_deploy == Deploy::Cloudflare
        && (next.runtime != Runtime::Workers || next.backend != Backend::Hono)
    {
        next.server_deploy = Deploy::None;
        change(
            Category::ServerDeploy,
            "Server deploy set to 'None' (Cloudflare requires Workers + Hono)".into(),
        );
    }

    if !supports_server_deploy_runtime(next.server_deploy, next.runtime) {
        next.server_deploy = if next.runtime == Runtime::Workers {
            Deploy::Cloudflare
        } else {
            Deploy::None
        };
        change(
            Category::ServerDeploy,
            format!(
                "Server deploy set to '{}' (required for {})",
                next.server_deploy.id(),
                next.runtime.id()
            ),
        );
    }

    (!changes.is_empty()).then_some((next, changes))
}
