//! The helpers of upstream's `packages/types/src/compatibility.ts` the rules use: which
//! frontends, backends, runtimes, databases and deployments go together, and upstream's
//! messages for the ones that don't.

use super::{OBSERVABILITY, STATIC_DESKTOP, core_backend};
use crate::stack_builder::{
    Addon, Api, Auth, Category, Database, DbSetup, Deploy, Orm, Runtime, Stack, WebFrontend,
    frontends, option,
};

const CONVEX_BETTER_AUTH_INCOMPATIBLE: [&str; 4] = ["nuxt", "svelte", "solid", "astro"];
const CONVEX_BETTER_AUTH_SUPPORTED: [&str; 7] = [
    "tanstack-router",
    "react-router",
    "tanstack-start",
    "next",
    "native-bare",
    "native-uniwind",
    "native-unistyles",
];
const SERVER_BACKENDS: [&str; 4] = ["hono", "express", "fastify", "elysia"];
const EVLOG_FULLSTACK: [&str; 5] = ["next", "tanstack-start", "nuxt", "svelte", "astro"];
const TRPC_INCOMPATIBLE: [&str; 4] = ["nuxt", "svelte", "solid", "astro"];
const CONVEX_INCOMPATIBLE: [&str; 2] = ["solid", "astro"];
const AI_INCOMPATIBLE: [&str; 2] = ["solid", "astro"];
const CONVEX_AI_INCOMPATIBLE: [&str; 4] = ["solid", "astro", "svelte", "nuxt"];
const DESKTOP_STATIC_EXPORT: [&str; 4] = ["next", "svelte", "astro", "react-router"];
const TAURI_STATIC_EXPORT: [&str; 2] = ["next", "tanstack-start"];
pub(super) const PRISMA_COMPUTE_WEB: [&str; 8] = [
    "tanstack-router",
    "next",
    "nuxt",
    "astro",
    "react-router",
    "tanstack-start",
    "svelte",
    "solid",
];
const DESKTOP_WEB: [&str; 7] = [
    "tanstack-router",
    "react-router",
    "tanstack-start",
    "next",
    "nuxt",
    "svelte",
    "astro",
];
const PWA_WEB: [&str; 4] = ["tanstack-router", "react-router", "solid", "next"];

pub(super) const CLERK_BACKEND_MESSAGE: &str = "Clerk requires Convex, Hono, Express, Fastify, Elysia, or Next.js/TanStack Start fullstack backend";
pub(super) const CLERK_FRONTEND_MESSAGE: &str =
    "Clerk requires React Router, TanStack Router, TanStack Start, Next.js, or React Native";
pub(super) const CONVEX_BETTER_AUTH_MESSAGE: &str = "Better-Auth with Convex requires React Router, TanStack Router, TanStack Start, Next.js, or React Native";
const EVLOG_MESSAGE: &str = "The observability addons support Hono, Express, Fastify, Elysia, or backend self with Next.js, TanStack Start, Nuxt, SvelteKit, or Astro. Convex and backend none are not supported yet.";
pub(super) const CLOUDFLARE_NEXT_MESSAGE: &str =
    "This Prisma PostgreSQL setup with Next.js is temporarily unavailable on Cloudflare";

/// What a backend takes over (`getBackendDisabledOptions`).
pub(super) fn backend_disabled(backend: &str) -> &'static [Category] {
    match backend {
        "convex" => &[
            Category::Runtime,
            Category::Database,
            Category::Orm,
            Category::Api,
            Category::DbSetup,
            Category::ServerDeploy,
        ],
        "none" => &[
            Category::Runtime,
            Category::Database,
            Category::Orm,
            Category::Api,
            Category::Auth,
            Category::Payments,
            Category::DbSetup,
            Category::ServerDeploy,
        ],
        "self" => &[Category::Runtime, Category::ServerDeploy],
        _ => &[],
    }
}

/// The web frontend's name on its card.
pub(super) fn web_name(web: WebFrontend) -> &'static str {
    option(Category::WebFrontend, web.id()).map_or(web.id(), |o| o.name)
}

/// Web and native frontend ids, none included (upstream's `[...web, ...native]`).
fn web_native(stack: &Stack) -> [&'static str; 2] {
    [stack.web_frontend.id(), stack.native_frontend.id()]
}

pub(super) fn is_frontend_allowed_with_backend(
    frontend: &str,
    backend: &str,
    auth: Option<Auth>,
) -> bool {
    if backend == "convex" {
        if auth == Some(Auth::BetterAuth) && CONVEX_BETTER_AUTH_INCOMPATIBLE.contains(&frontend) {
            return false;
        }
        if CONVEX_INCOMPATIBLE.contains(&frontend) {
            return false;
        }
    }
    if auth == Some(Auth::Clerk) && CONVEX_BETTER_AUTH_INCOMPATIBLE.contains(&frontend) {
        return false;
    }
    true
}

pub(super) fn convex_better_auth_compatible(stack: &Stack) -> bool {
    let both = web_native(stack);
    both.iter()
        .any(|f| CONVEX_BETTER_AUTH_SUPPORTED.contains(f))
        && both
            .iter()
            .all(|f| is_frontend_allowed_with_backend(f, "convex", Some(Auth::BetterAuth)))
}

pub(super) fn trpc_allowed(web: WebFrontend) -> bool {
    !TRPC_INCOMPATIBLE.contains(&web.id())
}

pub(super) fn supports_clerk_frontend(stack: &Stack) -> bool {
    web_native(stack)
        .iter()
        .all(|f| *f == "none" || CONVEX_BETTER_AUTH_SUPPORTED.contains(f))
}

pub(super) fn supports_clerk_backend(backend: &str, web: WebFrontend) -> bool {
    match backend {
        "self" => matches!(web, WebFrontend::Next | WebFrontend::TanstackStart),
        _ => backend == "convex" || SERVER_BACKENDS.contains(&backend),
    }
}

pub(super) fn supports_orm_database(orm: Orm, database: Database) -> bool {
    match orm {
        Orm::None => database == Database::None,
        Orm::Drizzle => matches!(
            database,
            Database::Sqlite | Database::Postgres | Database::Mysql
        ),
        Orm::Prisma => matches!(
            database,
            Database::Sqlite | Database::Postgres | Database::Mysql | Database::Mongodb
        ),
        Orm::Mongoose => database == Database::Mongodb,
    }
}

/// The databases a setup hosts (`getDatabaseSetupDatabases`).
pub(super) fn setup_databases(setup: DbSetup) -> &'static [Database] {
    match setup {
        DbSetup::Turso | DbSetup::D1 => &[Database::Sqlite],
        DbSetup::Neon | DbSetup::Supabase | DbSetup::PrismaPostgres => &[Database::Postgres],
        DbSetup::Planetscale => &[Database::Postgres, Database::Mysql],
        DbSetup::MongodbAtlas => &[Database::Mongodb],
        DbSetup::Docker => &[Database::Postgres, Database::Mysql, Database::Mongodb],
        DbSetup::None => &[],
    }
}

pub(super) fn supports_db_setup(setup: DbSetup, database: Database) -> bool {
    setup == DbSetup::None || setup_databases(setup).contains(&database)
}

pub(super) fn supports_runtime_backend(runtime: Runtime, backend: &str) -> bool {
    if backend_disabled(backend).contains(&Category::Runtime) {
        return runtime == Runtime::None;
    }
    runtime != Runtime::None && (runtime != Runtime::Workers || backend == "hono")
}

pub(super) fn supports_runtime_database(runtime: Runtime, database: Database) -> bool {
    runtime != Runtime::Workers || database != Database::Mongodb
}

pub(super) fn supports_db_setup_runtime(setup: DbSetup, runtime: Runtime, backend: &str) -> bool {
    match setup {
        DbSetup::Docker => runtime != Runtime::Workers,
        DbSetup::D1 => runtime == Runtime::Workers || backend == "self",
        _ => true,
    }
}

pub(super) fn supports_server_deploy_runtime(deploy: Deploy, runtime: Runtime) -> bool {
    match deploy {
        Deploy::None => runtime != Runtime::Workers,
        Deploy::Cloudflare => runtime == Runtime::Workers,
        Deploy::Vercel => runtime == Runtime::Node,
        Deploy::Prisma | Deploy::Docker => matches!(runtime, Runtime::Bun | Runtime::Node),
    }
}

pub(super) fn example_todo_allowed(backend: &str, database: Database, api: Api) -> bool {
    backend == "convex" || (database != Database::None && api != Api::None)
}

pub(super) fn example_ai_allowed(web: WebFrontend) -> bool {
    !AI_INCOMPATIBLE.contains(&web.id())
}

pub(super) fn convex_ai_incompatible(web: WebFrontend) -> bool {
    CONVEX_AI_INCOMPATIBLE.contains(&web.id())
}

/// Why `addon` doesn't fit (`validateAddonCompatibility`, with upstream's own frontends list).
pub(super) fn addon_issue(stack: &Stack, addon: Addon) -> Option<String> {
    let frontends = frontends(stack);
    let backend = core_backend(stack.backend);
    let id = addon.id();
    if OBSERVABILITY.contains(&addon) {
        let supported = SERVER_BACKENDS.contains(&backend)
            || (backend == "self" && frontends.iter().any(|f| EVLOG_FULLSTACK.contains(f)));
        if !supported {
            return Some(EVLOG_MESSAGE.into());
        }
    }
    if STATIC_DESKTOP.contains(&addon)
        && stack.auth == Auth::Clerk
        && frontends.contains(&"react-router")
    {
        return Some(format!(
            "{id} addon forces React Router into a static export, but Clerk on React Router requires SSR middleware. Remove the addon or use a different auth/frontend."
        ));
    }
    if backend == "self" && STATIC_DESKTOP.contains(&addon) {
        return Some(format!(
            "{id} addon requires a separate backend or no backend because backend 'self' emits server routes that cannot be bundled as static desktop assets."
        ));
    }
    if addon == Addon::Tauri
        && backend == "convex"
        && stack.auth == Auth::BetterAuth
        && frontends.iter().any(|f| TAURI_STATIC_EXPORT.contains(f))
    {
        return Some(
            "tauri addon is not compatible with Convex Better Auth on Next.js or TanStack Start because those templates use server auth bootstrap and cannot be exported as static desktop assets."
                .into(),
        );
    }
    let compatible: &[&str] = match addon {
        Addon::Pwa => &PWA_WEB,
        Addon::Tauri | Addon::Electrobun => &DESKTOP_WEB,
        _ => &[],
    };
    if !compatible.is_empty() && !frontends.iter().any(|f| compatible.contains(f)) {
        return Some(format!(
            "{id} addon requires one of these frontends: {}",
            compatible.join(", ")
        ));
    }
    None
}

/// A deployment that can't serve the static build a desktop addon needs
/// (`getDesktopDeployConflict`): the frontend it affects and the desktop addons picked.
fn desktop_deploy_conflict(
    deploy: Deploy,
    addons: &[Addon],
    web: WebFrontend,
    backend: Option<&str>,
    auth: Option<Auth>,
) -> Option<(&'static str, Vec<&'static str>)> {
    if !matches!(deploy, Deploy::Docker | Deploy::Prisma) {
        return None;
    }
    let desktop: Vec<&'static str> = addons
        .iter()
        .filter(|a| STATIC_DESKTOP.contains(a))
        .map(|a| a.id())
        .collect();
    let affected = Some(web.id()).filter(|f| DESKTOP_STATIC_EXPORT.contains(f))?;
    if desktop.is_empty() {
        return None;
    }
    if deploy == Deploy::Docker
        && affected == "next"
        && !desktop.contains(&"tauri")
        && backend == Some("convex")
        && auth == Some(Auth::BetterAuth)
    {
        return None;
    }
    Some((affected, desktop))
}

pub(super) fn docker_desktop_conflict(stack: &Stack) -> Option<(&'static str, Vec<&'static str>)> {
    desktop_deploy_conflict(
        Deploy::Docker,
        &stack.addons,
        stack.web_frontend,
        Some(core_backend(stack.backend)),
        Some(stack.auth),
    )
}

pub(super) fn prisma_desktop_conflict(
    addons: &[Addon],
    web: WebFrontend,
) -> Option<(&'static str, Vec<&'static str>)> {
    desktop_deploy_conflict(Deploy::Prisma, addons, web, None, None)
}

/// `hasCloudflareNextPostgresConflict`, with the web deploy given.
pub(super) fn cloudflare_next_issue(stack: &Stack, web_deploy: Deploy) -> bool {
    web_deploy == Deploy::Cloudflare
        && stack.web_frontend == WebFrontend::Next
        && stack.database == Database::Postgres
        && stack.orm == Orm::Prisma
        && !matches!(stack.db_setup, DbSetup::Neon | DbSetup::PrismaPostgres)
}
