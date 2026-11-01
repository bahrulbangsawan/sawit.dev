//! Upstream's compatibility rules: `analyzeStackCompatibility` (the auto-adjustments, branch
//! for branch, with upstream's messages), the fixed point that runs it, `getDisabledReason`,
//! the list sanitizing, and the helpers of `packages/types/src/compatibility.ts` they use.

use super::{
    Addon, Api, Auth, Backend, Category, Change, Database, DbSetup, Deploy, Example, Orm, Payments,
    Resolved, Runtime, Stack, WebFrontend, frontends, option,
};

/// Upstream's `MAX_COMPATIBILITY_PASSES`.
const MAX_PASSES: usize = 10;

const TASK_RUNNERS: [Addon; 3] = [Addon::Turborepo, Addon::Nx, Addon::VitePlus];
const OBSERVABILITY: [Addon; 2] = [Addon::Evlog, Addon::Axiom];
const STATIC_DESKTOP: [Addon; 2] = [Addon::Tauri, Addon::Electrobun];

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
const PRISMA_COMPUTE_WEB: [&str; 8] = [
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

const CLERK_BACKEND_MESSAGE: &str = "Clerk requires Convex, Hono, Express, Fastify, Elysia, or Next.js/TanStack Start fullstack backend";
const CLERK_FRONTEND_MESSAGE: &str =
    "Clerk requires React Router, TanStack Router, TanStack Start, Next.js, or React Native";
const CONVEX_BETTER_AUTH_MESSAGE: &str = "Better-Auth with Convex requires React Router, TanStack Router, TanStack Start, Next.js, or React Native";
const EVLOG_MESSAGE: &str = "The observability addons support Hono, Express, Fastify, Elysia, or backend self with Next.js, TanStack Start, Nuxt, SvelteKit, or Astro. Convex and backend none are not supported yet.";
const CLOUDFLARE_NEXT_MESSAGE: &str =
    "This Prisma PostgreSQL setup with Next.js is temporarily unavailable on Cloudflare";

/// The web frontend a fullstack backend serves its routes from (`getSelfBackendFrontend`).
pub(super) fn self_frontend(backend: Backend) -> Option<WebFrontend> {
    Some(match backend {
        Backend::SelfNext => WebFrontend::Next,
        Backend::SelfTanstackStart => WebFrontend::TanstackStart,
        Backend::SelfNuxt => WebFrontend::Nuxt,
        Backend::SelfSvelte => WebFrontend::Svelte,
        Backend::SelfSolid => WebFrontend::Solid,
        Backend::SelfAstro => WebFrontend::Astro,
        _ => return None,
    })
}

/// The fullstack backend of web frontend `web`, the CLI's `--backend self` with it.
pub(super) fn self_backend(web: WebFrontend) -> Option<Backend> {
    Backend::ALL
        .iter()
        .copied()
        .find(|b| self_frontend(*b) == Some(web))
}

/// The backend as the CLI and the rules name it (`getStackBackend`): `self` for every
/// fullstack one.
pub(super) fn core_backend(backend: Backend) -> &'static str {
    if self_frontend(backend).is_some() {
        "self"
    } else {
        backend.id()
    }
}

/// What a backend takes over (`getBackendDisabledOptions`).
fn backend_disabled(backend: &str) -> &'static [Category] {
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
fn web_name(web: WebFrontend) -> &'static str {
    option(Category::WebFrontend, web.id()).map_or(web.id(), |o| o.name)
}

/// Web and native frontend ids, none included (upstream's `[...web, ...native]`).
fn web_native(stack: &Stack) -> [&'static str; 2] {
    [stack.web_frontend.id(), stack.native_frontend.id()]
}

fn is_frontend_allowed_with_backend(frontend: &str, backend: &str, auth: Option<Auth>) -> bool {
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

fn convex_better_auth_compatible(stack: &Stack) -> bool {
    let both = web_native(stack);
    both.iter()
        .any(|f| CONVEX_BETTER_AUTH_SUPPORTED.contains(f))
        && both
            .iter()
            .all(|f| is_frontend_allowed_with_backend(f, "convex", Some(Auth::BetterAuth)))
}

fn trpc_allowed(web: WebFrontend) -> bool {
    !TRPC_INCOMPATIBLE.contains(&web.id())
}

fn supports_clerk_frontend(stack: &Stack) -> bool {
    web_native(stack)
        .iter()
        .all(|f| *f == "none" || CONVEX_BETTER_AUTH_SUPPORTED.contains(f))
}

fn supports_clerk_backend(backend: &str, web: WebFrontend) -> bool {
    match backend {
        "self" => matches!(web, WebFrontend::Next | WebFrontend::TanstackStart),
        _ => backend == "convex" || SERVER_BACKENDS.contains(&backend),
    }
}

fn supports_orm_database(orm: Orm, database: Database) -> bool {
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
fn setup_databases(setup: DbSetup) -> &'static [Database] {
    match setup {
        DbSetup::Turso | DbSetup::D1 => &[Database::Sqlite],
        DbSetup::Neon | DbSetup::Supabase | DbSetup::PrismaPostgres => &[Database::Postgres],
        DbSetup::Planetscale => &[Database::Postgres, Database::Mysql],
        DbSetup::MongodbAtlas => &[Database::Mongodb],
        DbSetup::Docker => &[Database::Postgres, Database::Mysql, Database::Mongodb],
        DbSetup::None => &[],
    }
}

fn supports_db_setup(setup: DbSetup, database: Database) -> bool {
    setup == DbSetup::None || setup_databases(setup).contains(&database)
}

fn supports_runtime_backend(runtime: Runtime, backend: &str) -> bool {
    if backend_disabled(backend).contains(&Category::Runtime) {
        return runtime == Runtime::None;
    }
    runtime != Runtime::None && (runtime != Runtime::Workers || backend == "hono")
}

fn supports_runtime_database(runtime: Runtime, database: Database) -> bool {
    runtime != Runtime::Workers || database != Database::Mongodb
}

fn supports_db_setup_runtime(setup: DbSetup, runtime: Runtime, backend: &str) -> bool {
    match setup {
        DbSetup::Docker => runtime != Runtime::Workers,
        DbSetup::D1 => runtime == Runtime::Workers || backend == "self",
        _ => true,
    }
}

fn supports_server_deploy_runtime(deploy: Deploy, runtime: Runtime) -> bool {
    match deploy {
        Deploy::None => runtime != Runtime::Workers,
        Deploy::Cloudflare => runtime == Runtime::Workers,
        Deploy::Vercel => runtime == Runtime::Node,
        Deploy::Prisma | Deploy::Docker => matches!(runtime, Runtime::Bun | Runtime::Node),
    }
}

fn example_todo_allowed(backend: &str, database: Database, api: Api) -> bool {
    backend == "convex" || (database != Database::None && api != Api::None)
}

fn example_ai_allowed(web: WebFrontend) -> bool {
    !AI_INCOMPATIBLE.contains(&web.id())
}

fn convex_ai_incompatible(web: WebFrontend) -> bool {
    CONVEX_AI_INCOMPATIBLE.contains(&web.id())
}

/// Why `addon` doesn't fit (`validateAddonCompatibility`, with upstream's own frontends list).
fn addon_issue(stack: &Stack, addon: Addon) -> Option<String> {
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

fn docker_desktop_conflict(stack: &Stack) -> Option<(&'static str, Vec<&'static str>)> {
    desktop_deploy_conflict(
        Deploy::Docker,
        &stack.addons,
        stack.web_frontend,
        Some(core_backend(stack.backend)),
        Some(stack.auth),
    )
}

fn prisma_desktop_conflict(
    addons: &[Addon],
    web: WebFrontend,
) -> Option<(&'static str, Vec<&'static str>)> {
    desktop_deploy_conflict(Deploy::Prisma, addons, web, None, None)
}

/// `hasCloudflareNextPostgresConflict`, with the web deploy given.
fn cloudflare_next_issue(stack: &Stack, web_deploy: Deploy) -> bool {
    web_deploy == Deploy::Cloudflare
        && stack.web_frontend == WebFrontend::Next
        && stack.database == Database::Postgres
        && stack.orm == Orm::Prisma
        && !matches!(stack.db_setup, DbSetup::Neon | DbSetup::PrismaPostgres)
}

/// Whether the CLI would ask how to provision the database (automatic or manual): the setups
/// it provisions over the network, unless Alchemy manages them (`supportsAlchemyManagedDatabase`).
pub(super) fn asks_how_to_provision(stack: &Stack) -> bool {
    let remote = matches!(
        stack.db_setup,
        DbSetup::Turso
            | DbSetup::Neon
            | DbSetup::PrismaPostgres
            | DbSetup::Supabase
            | DbSetup::MongodbAtlas
    );
    let alchemy_setup = matches!(
        stack.db_setup,
        DbSetup::Neon | DbSetup::Planetscale | DbSetup::PrismaPostgres
    );
    let deploy = if core_backend(stack.backend) == "self" {
        stack.web_deploy
    } else {
        stack.server_deploy
    };
    let alchemy = alchemy_setup && matches!(deploy, Deploy::Cloudflare | Deploy::Prisma);
    remote && !alchemy
}

/// `values` without repeats, in their first order.
pub(super) fn dedupe<T: PartialEq + Copy>(values: &[T]) -> Vec<T> {
    let mut out = Vec::new();
    for v in values {
        if !out.contains(v) {
            out.push(*v);
        }
    }
    out
}

/// Upstream's `sanitizeAddons`: no repeats, and of the task runners (Turborepo, Nx, Vite+) and
/// of the observability addons (evlog, Axiom) only the last one listed, at its place.
pub(super) fn sanitize_addons(addons: &[Addon]) -> Vec<Addon> {
    let mut out: Vec<Addon> = Vec::new();
    for &addon in &dedupe(addons) {
        for group in [&TASK_RUNNERS[..], &OBSERVABILITY[..]] {
            if group.contains(&addon)
                && let Some(ix) = out.iter().position(|a| group.contains(a))
            {
                out.remove(ix);
            }
        }
        if !out.contains(&addon) {
            out.push(addon);
        }
    }
    out
}

/// Upstream's `sanitizeStackState`, on a typed stack: the lists.
fn sanitize(stack: &Stack) -> Stack {
    Stack {
        addons: sanitize_addons(&stack.addons),
        examples: dedupe(&stack.examples),
        ..stack.clone()
    }
}

/// Sets `category`'s single pick to none, if it isn't: whether it changed.
fn set_none(stack: &mut Stack, category: Category) -> bool {
    if stack.picked(category) == ["none"] {
        return false;
    }
    stack.set(category, "none")
}

/// One pass of upstream's `analyzeStackCompatibility`: the stack it adjusts to, with its
/// changes, or `None` when nothing needed adjusting.
fn analyze(stack: &Stack) -> Option<(Stack, Vec<Change>)> {
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

/// Upstream's `getDesktopBuildNote`: what a desktop addon on a static-export frontend needs.
fn desktop_note(stack: &Stack) -> Option<String> {
    let desktop: Vec<Addon> = stack
        .addons
        .iter()
        .copied()
        .filter(|a| STATIC_DESKTOP.contains(a))
        .collect();
    let first = *desktop.first()?;
    let frontend = match stack.web_frontend {
        WebFrontend::TanstackStart => "TanStack Start",
        WebFrontend::Next => "Next.js",
        WebFrontend::Nuxt => "Nuxt",
        WebFrontend::Svelte => "SvelteKit",
        WebFrontend::Astro => "Astro",
        _ => return None,
    };
    let label = if desktop.len() == 2 {
        "Tauri and Electrobun desktop builds".to_string()
    } else {
        let name = if first == Addon::Tauri {
            "Tauri"
        } else {
            "Electrobun"
        };
        format!("{name} desktop builds")
    };
    Some(if self_frontend(stack.backend).is_some() {
        format!(
            "{label} package static web assets and require a separate backend or no backend. Fullstack self backends emit server routes inside the web app, so they cannot be bundled for desktop packaging."
        )
    } else {
        format!(
            "{label} package static web assets. {frontend} needs a static/export build configuration before desktop packaging will work."
        )
    })
}

/// `stack` adjusted until the rules agree with it (upstream's `resolveStackCompatibility`:
/// sanitize, then adjust and sanitize again until nothing changes, 10 passes at most), every
/// change made on the way, and the notes for the stack it ends with.
pub fn resolve(stack: &Stack) -> Resolved {
    let mut current = sanitize(stack);
    let mut changes = Vec::new();
    for _ in 0..MAX_PASSES {
        match analyze(&current) {
            None => break,
            Some((next, made)) => {
                changes.extend(made);
                current = sanitize(&next);
            }
        }
    }
    let notes = desktop_note(&current).into_iter().collect();
    Resolved {
        stack: current,
        changes,
        notes,
    }
}

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
