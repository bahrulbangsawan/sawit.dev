//! Upstream's `getDisabledReason`: why an option can't be picked with a stack as it is.

use super::compat::{
    CLERK_BACKEND_MESSAGE, CLERK_FRONTEND_MESSAGE, CLOUDFLARE_NEXT_MESSAGE,
    CONVEX_BETTER_AUTH_MESSAGE, PRISMA_COMPUTE_WEB, addon_issue, backend_disabled,
    cloudflare_next_issue, convex_ai_incompatible, convex_better_auth_compatible,
    docker_desktop_conflict, example_ai_allowed, example_todo_allowed,
    is_frontend_allowed_with_backend, prisma_desktop_conflict, setup_databases,
    supports_clerk_backend, supports_clerk_frontend, supports_db_setup, supports_db_setup_runtime,
    supports_orm_database, supports_runtime_backend, supports_runtime_database,
    supports_server_deploy_runtime, trpc_allowed, web_name,
};
use super::{core_backend, self_frontend};
use crate::stack_builder::{
    Addon, Api, Auth, Backend, Category, Database, DbSetup, Deploy, Orm, Runtime, Stack,
    WebFrontend, option,
};

/// Why option `id` of `category` can't be picked with `stack` as it is (upstream's
/// `getDisabledReason`, with its words), or `None` when it can. Options a pick would repair
/// stay enabled, as upstream leaves them.
pub fn disabled_reason(stack: &Stack, category: Category, id: &str) -> Option<String> {
    let backend = core_backend(stack.backend);
    let valid = option(category, id).is_some();

    if stack.backend == Backend::Convex {
        if backend_disabled("convex").contains(&category) && id != "none" {
            return Some(format!(
                "Convex provides its own {}",
                category.display_name().to_lowercase()
            ));
        }
        if category == Category::Auth
            && id == "better-auth"
            && !convex_better_auth_compatible(stack)
        {
            return Some(CONVEX_BETTER_AUTH_MESSAGE.into());
        }
        if category == Category::WebFrontend
            && valid
            && !is_frontend_allowed_with_backend(id, "convex", None)
        {
            let mut name = id.to_string();
            if let Some(first) = name.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            return Some(format!("{name} is not compatible with Convex"));
        }
        if category == Category::Examples
            && id == "ai"
            && convex_ai_incompatible(stack.web_frontend)
        {
            return Some(format!(
                "Convex AI example only supports React-based frontends (not {})",
                stack.web_frontend.id()
            ));
        }
    }

    if stack.backend == Backend::None
        && id != "none"
        && (category == Category::Examples || backend_disabled("none").contains(&category))
    {
        return Some("No backend selected".into());
    }

    if let Some(fullstack) = self_frontend(stack.backend) {
        let name = web_name(fullstack);
        if category == Category::Runtime && id != "none" {
            return Some(format!("{name} fullstack uses built-in server routes"));
        }
        if category == Category::WebFrontend && valid && id != fullstack.id() {
            return Some(format!("{name} fullstack requires {name} frontend"));
        }
        if category == Category::ServerDeploy && id != "none" {
            return Some("Fullstack uses frontend deployment".into());
        }
        if category == Category::Api && id == "trpc" && !trpc_allowed(fullstack) {
            return Some(format!("tRPC is not compatible with {name} (use oRPC)"));
        }
    }

    if category == Category::Backend
        && let Some(option_backend) = Backend::parse(id)
    {
        if let Some(required) = self_frontend(option_backend)
            && stack.web_frontend != required
        {
            return Some(format!("Requires {} frontend", web_name(required)));
        }
        if option_backend == Backend::Convex
            && !is_frontend_allowed_with_backend(stack.web_frontend.id(), "convex", None)
        {
            let incompatible = if stack.web_frontend == WebFrontend::Solid {
                "Solid"
            } else {
                "Astro"
            };
            return Some(format!("Convex is not compatible with {incompatible}"));
        }
        if stack.runtime == Runtime::Workers
            && id != "none"
            && !supports_runtime_backend(stack.runtime, core_backend(option_backend))
        {
            return Some("Workers runtime only works with Hono".into());
        }
    }

    if category == Category::Runtime {
        if id == "workers" && !supports_runtime_backend(Runtime::Workers, backend) {
            return Some("Workers requires Hono backend".into());
        }
        if id == "none" && !supports_runtime_backend(Runtime::None, backend) {
            return Some("Runtime 'None' only for Convex or fullstack backends".into());
        }
    }

    if category == Category::Database
        && let Some(database) = Database::parse(id)
        && !supports_runtime_database(stack.runtime, database)
    {
        return Some("MongoDB is not compatible with Workers runtime".into());
    }

    if category == Category::Orm
        && let Some(orm) = Orm::parse(id)
        && (!supports_orm_database(orm, stack.database)
            || (orm == Orm::Mongoose
                && !supports_runtime_database(stack.runtime, Database::Mongodb)))
    {
        if stack.database == Database::None && orm != Orm::None {
            return Some("Select a database first".into());
        }
        if orm == Orm::Mongoose {
            if stack.runtime == Runtime::Workers {
                return Some(
                    "Mongoose requires MongoDB, which is incompatible with Workers".into(),
                );
            }
            if stack.database != Database::Mongodb {
                return Some("Mongoose only works with MongoDB".into());
            }
        }
        if orm == Orm::Drizzle && stack.database == Database::Mongodb {
            return Some("Drizzle does not support MongoDB".into());
        }
        if orm == Orm::None && stack.database != Database::None {
            return Some("Database requires an ORM".into());
        }
        return Some(format!("{id} does not support {}", stack.database.id()));
    }

    if category == Category::DbSetup
        && let Some(setup) = DbSetup::parse(id)
        && setup != DbSetup::None
    {
        if stack.database == Database::None {
            return Some("Select a database first".into());
        }
        if !supports_db_setup(setup, stack.database) {
            let names: Vec<&str> = setup_databases(setup).iter().map(|d| d.id()).collect();
            let name = option(Category::DbSetup, id).map_or(id, |o| o.name);
            let names = if names.is_empty() {
                "a compatible database".to_string()
            } else {
                names.join(" or ")
            };
            return Some(format!("{name} requires {names}"));
        }
        if !supports_db_setup_runtime(setup, stack.runtime, backend) {
            return Some(if setup == DbSetup::D1 {
                "D1 requires Cloudflare Workers runtime or a self fullstack backend".into()
            } else {
                "Docker is incompatible with Workers".into()
            });
        }
    }

    if category == Category::Api && id == "trpc" && !trpc_allowed(stack.web_frontend) {
        return Some(format!(
            "{} requires oRPC, not tRPC",
            stack.web_frontend.id()
        ));
    }

    if category == Category::Auth && id == "clerk" {
        if !supports_clerk_backend(backend, stack.web_frontend) {
            return Some(CLERK_BACKEND_MESSAGE.into());
        }
        if !supports_clerk_frontend(stack) {
            return Some(CLERK_FRONTEND_MESSAGE.into());
        }
    }

    if category == Category::Payments && id == "polar" && stack.auth != Auth::BetterAuth {
        return Some("Polar requires Better Auth".into());
    }

    if category == Category::Addons
        && let Some(addon) = Addon::parse(id)
    {
        if let Some(issue) = addon_issue(stack, addon) {
            return Some(issue);
        }
        if stack.web_deploy == Deploy::Prisma
            && prisma_desktop_conflict(&[addon], stack.web_frontend).is_some()
        {
            return Some(format!(
                "{id} requires a static web build, but Prisma requires an executable server artifact"
            ));
        }
    }

    if category == Category::Examples {
        if id == "todo" && !example_todo_allowed(backend, stack.database, stack.api) {
            if stack.database == Database::None {
                return Some("Todo example requires a database".into());
            }
            if stack.api == Api::None {
                return Some("Todo example requires an API layer (tRPC or oRPC)".into());
            }
        }
        if id == "ai" {
            if !example_ai_allowed(stack.web_frontend) {
                return Some("AI example not compatible with Solid or Astro frontend".into());
            }
            if stack.backend == Backend::Convex && convex_ai_incompatible(stack.web_frontend) {
                return Some(format!(
                    "Convex AI example only supports React-based frontends (not {})",
                    stack.web_frontend.id()
                ));
            }
        }
    }

    if category == Category::WebDeploy && id != "none" {
        if stack.web_frontend == WebFrontend::None {
            return Some("Web deployment requires a web frontend".into());
        }
        if id == "docker"
            && let Some((frontend, addons)) = docker_desktop_conflict(stack)
        {
            return Some(format!(
                "Docker cannot serve the static output required by {} on {frontend}",
                addons.join(" and ")
            ));
        }
        if id == "prisma" && !PRISMA_COMPUTE_WEB.contains(&stack.web_frontend.id()) {
            return Some("Prisma requires TanStack Router, Next.js, Nuxt, Astro, React Router, TanStack Start, SvelteKit, or Solid".into());
        }
        if id == "prisma"
            && let Some((frontend, addons)) =
                prisma_desktop_conflict(&stack.addons, stack.web_frontend)
        {
            return Some(format!(
                "Prisma cannot deploy the static output required by {} on {frontend}",
                addons.join(" and ")
            ));
        }
        if id == "cloudflare" && cloudflare_next_issue(stack, Deploy::Cloudflare) {
            return Some(CLOUDFLARE_NEXT_MESSAGE.into());
        }
    }

    if category == Category::WebDeploy
        && stack.db_setup == DbSetup::D1
        && self_frontend(stack.backend).is_some()
        && id != "cloudflare"
    {
        return Some("D1 with a self fullstack backend requires Cloudflare web deployment".into());
    }

    if category == Category::ServerDeploy {
        let deploy = Deploy::parse(id);
        let unsupported =
            |d: Deploy| deploy == Some(d) && !supports_server_deploy_runtime(d, stack.runtime);
        if id == "cloudflare" {
            if unsupported(Deploy::Cloudflare) {
                return Some("Cloudflare requires Workers runtime".into());
            }
            if !supports_runtime_backend(Runtime::Workers, backend) {
                return Some("Cloudflare requires Hono backend".into());
            }
        }
        if unsupported(Deploy::Docker) {
            return Some("Docker server deployment requires the Bun or Node runtime".into());
        }
        if unsupported(Deploy::Vercel) {
            return Some("Vercel server deployment requires the Node runtime".into());
        }
        if unsupported(Deploy::Prisma) {
            return Some("Prisma server deployment requires the Bun or Node runtime".into());
        }
        if id != "none" && backend_disabled(backend).contains(&Category::ServerDeploy) {
            return Some("Server deployment not needed for this backend".into());
        }
        if id == "none" && stack.runtime == Runtime::Workers {
            return Some("Workers requires server deployment".into());
        }
    }

    None
}
