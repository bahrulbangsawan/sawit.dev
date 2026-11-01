//! The New Project page's stack builder: Better-T-Stack's options, presets, default stack and
//! compatibility rules, and the `create-better-t-stack` command for a stack.
//!
//! Ported from Better-T-Stack (`AmanVarshney01/create-better-t-stack`, MIT licence), commit
//! `a9bd456ccc9d`, whose CLI is `create-better-t-stack` 3.49.0 ([`CLI_VERSION`]):
//! `apps/web/src/lib/constant.ts` (options, presets, default stack), `stack-validation.ts`
//! (`analyzeStackCompatibility`, `getDisabledReason`), `stack-compatibility.ts`
//! (`resolveStackCompatibility`, `applyStackUpdate`), `sanitize-stack-addons.ts`,
//! `stack-utils.ts` (`generateStackCommand`, `getSelectedTechs`, `getDesktopBuildNote`),
//! `stack-model.ts`, the builder's `use-stack-builder.ts` (what a click and a tag's × do) and
//! `packages/types/src/compatibility.ts`.
//!
//! Re-porting is a manual job: compare those files at a newer commit, update the data, the
//! rules, [`CLI_VERSION`] and [`UPSTREAM_COMMIT`], then regenerate `upstream.json` beside this
//! file by running upstream's own TypeScript (its `commit` field says which), so the tests
//! check the port against upstream's answers again.

mod rules;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use rules::{disabled_reason, resolve};

/// The `create-better-t-stack` version the data and rules come from. The command pins it, so
/// its flags always match what this module knows.
pub const CLI_VERSION: &str = "3.49.0";

/// The upstream commit the data and rules were ported from.
pub const UPSTREAM_COMMIT: &str = "a9bd456ccc9d";

/// The project name upstream starts with.
pub const DEFAULT_NAME: &str = "my-better-t-app";

/// An enum of upstream ids, with every value in upstream's order, its id and serde by id.
macro_rules! ids {
    ($(#[$meta:meta])* pub enum $name:ident { $($variant:ident = $id:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            /// Every value, in upstream's order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// Upstream's id, as the CLI's flags spell it.
            pub fn id(self) -> &'static str {
                match self {
                    $($name::$variant => $id),+
                }
            }

            /// The value upstream calls `id`.
            pub fn parse(id: &str) -> Option<Self> {
                match id {
                    $($id => Some($name::$variant),)+
                    _ => None,
                }
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.id())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let id = String::deserialize(d)?;
                Self::parse(&id).ok_or_else(|| {
                    let all: Vec<&str> = Self::ALL.iter().map(|v| v.id()).collect();
                    serde::de::Error::custom(format!("“{id}” isn't one of {}", all.join(", ")))
                })
            }
        }
    };
}

ids! {
    /// The web frontend (one at most, as upstream 3.49.0 keeps it).
    pub enum WebFrontend {
        TanstackRouter = "tanstack-router",
        ReactRouter = "react-router",
        TanstackStart = "tanstack-start",
        Next = "next",
        Nuxt = "nuxt",
        Svelte = "svelte",
        Solid = "solid",
        Astro = "astro",
        None = "none",
    }
}

ids! {
    /// The native (Expo) frontend (one at most).
    pub enum NativeFrontend {
        Bare = "native-bare",
        Uniwind = "native-uniwind",
        Unistyles = "native-unistyles",
        None = "none",
    }
}

ids! {
    /// Where the server code runs.
    pub enum Runtime {
        Bun = "bun",
        Node = "node",
        Workers = "workers",
        None = "none",
    }
}

ids! {
    /// The backend: a server framework, Convex, a fullstack web framework's own routes
    /// (`self-…`, the CLI's `--backend self`) or none.
    pub enum Backend {
        Hono = "hono",
        Elysia = "elysia",
        Express = "express",
        Fastify = "fastify",
        Convex = "convex",
        SelfNext = "self-next",
        SelfTanstackStart = "self-tanstack-start",
        SelfNuxt = "self-nuxt",
        SelfSvelte = "self-svelte",
        SelfSolid = "self-solid",
        SelfAstro = "self-astro",
        None = "none",
    }
}

ids! {
    /// The API layer.
    pub enum Api {
        Trpc = "trpc",
        Orpc = "orpc",
        None = "none",
    }
}

ids! {
    /// The database.
    pub enum Database {
        Sqlite = "sqlite",
        Postgres = "postgres",
        Mysql = "mysql",
        Mongodb = "mongodb",
        None = "none",
    }
}

ids! {
    /// The ORM.
    pub enum Orm {
        Drizzle = "drizzle",
        Prisma = "prisma",
        Mongoose = "mongoose",
        None = "none",
    }
}

ids! {
    /// Where the database is hosted.
    pub enum DbSetup {
        Turso = "turso",
        D1 = "d1",
        Neon = "neon",
        PrismaPostgres = "prisma-postgres",
        MongodbAtlas = "mongodb-atlas",
        Supabase = "supabase",
        Planetscale = "planetscale",
        Docker = "docker",
        None = "none",
    }
}

ids! {
    /// A deployment target, for the web app or the server.
    pub enum Deploy {
        Cloudflare = "cloudflare",
        Prisma = "prisma",
        Docker = "docker",
        Vercel = "vercel",
        None = "none",
    }
}

ids! {
    /// Authentication.
    pub enum Auth {
        BetterAuth = "better-auth",
        Clerk = "clerk",
        None = "none",
    }
}

ids! {
    /// Payments.
    pub enum Payments {
        Polar = "polar",
        None = "none",
    }
}

ids! {
    /// The package manager the project uses, which also runs the CLI.
    pub enum Pm {
        Npm = "npm",
        Pnpm = "pnpm",
        Bun = "bun",
    }
}

ids! {
    /// An addon (any number of them).
    pub enum Addon {
        Pwa = "pwa",
        Tauri = "tauri",
        Electrobun = "electrobun",
        Starlight = "starlight",
        Fumadocs = "fumadocs",
        Lefthook = "lefthook",
        Husky = "husky",
        Biome = "biome",
        Oxlint = "oxlint",
        Turborepo = "turborepo",
        Nx = "nx",
        VitePlus = "vite-plus",
        Ultracite = "ultracite",
        Opentui = "opentui",
        Wxt = "wxt",
        Skills = "skills",
        Mcp = "mcp",
        Evlog = "evlog",
        Axiom = "axiom",
    }
}

ids! {
    /// An example (any number of them).
    pub enum Example {
        Todo = "todo",
        Ai = "ai",
    }
}

ids! {
    /// A category of the builder, in upstream's order (`CATEGORY_ORDER`); the id is upstream's
    /// key in its stack.
    pub enum Category {
        WebFrontend = "webFrontend",
        NativeFrontend = "nativeFrontend",
        Backend = "backend",
        Runtime = "runtime",
        Api = "api",
        Database = "database",
        Orm = "orm",
        DbSetup = "dbSetup",
        WebDeploy = "webDeploy",
        ServerDeploy = "serverDeploy",
        Auth = "auth",
        Payments = "payments",
        PackageManager = "packageManager",
        Addons = "addons",
        Examples = "examples",
        Git = "git",
        Install = "install",
    }
}

impl Pm {
    /// The package manager of [`crate::package_manager`] this is.
    pub fn package_manager(self) -> crate::package_manager::PackageManager {
        use crate::package_manager::PackageManager;
        match self {
            Pm::Npm => PackageManager::Npm,
            Pm::Pnpm => PackageManager::Pnpm,
            Pm::Bun => PackageManager::Bun,
        }
    }

    /// The one `pm` is, if the CLI knows it (it has no Yarn).
    pub fn from_package_manager(pm: crate::package_manager::PackageManager) -> Option<Pm> {
        Pm::parse(pm.id())
    }

    /// What starts the CLI with this package manager, pinned to [`CLI_VERSION`].
    fn runner(self) -> String {
        match self {
            Pm::Npm => format!("npx create-better-t-stack@{CLI_VERSION}"),
            Pm::Pnpm => format!("pnpm create better-t-stack@{CLI_VERSION}"),
            Pm::Bun => format!("bun create better-t-stack@{CLI_VERSION}"),
        }
    }
}

impl Category {
    /// The heading the page shows.
    pub fn label(self) -> &'static str {
        match self {
            Category::WebFrontend => "Web Frontend",
            Category::NativeFrontend => "Native Frontend",
            Category::Backend => "Backend",
            Category::Runtime => "Runtime",
            Category::Api => "API",
            Category::Database => "Database",
            Category::Orm => "ORM",
            Category::DbSetup => "Database Setup",
            Category::WebDeploy => "Web Deploy",
            Category::ServerDeploy => "Server Deploy",
            Category::Auth => "Auth",
            Category::Payments => "Payments",
            Category::PackageManager => "Package Manager",
            Category::Addons => "Addons",
            Category::Examples => "Examples",
            Category::Git => "Git",
            Category::Install => "Install",
        }
    }

    /// Upstream's `getCategoryDisplayName`: the id split before each capital, first letter up
    /// ("Db Setup", "Api"). Upstream's messages use it, so the port's do too.
    pub(crate) fn display_name(self) -> String {
        let mut out = String::new();
        for (i, c) in self.id().chars().enumerate() {
            if c.is_ascii_uppercase() {
                out.push(' ');
            }
            if i == 0 {
                out.push(c.to_ascii_uppercase());
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Whether any number of its options can be picked (addons, examples).
    pub fn multiple(self) -> bool {
        matches!(self, Category::Addons | Category::Examples)
    }
}

/// One option of a category, as the page shows its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StackOption {
    /// Upstream's id, as the CLI takes it.
    pub id: &'static str,
    pub name: &'static str,
    /// Upstream's one-line description.
    pub description: &'static str,
    /// Part of the default stack (upstream's "Default" tag).
    pub default: bool,
    /// Upstream marks it experimental.
    pub experimental: bool,
}

const fn opt(id: &'static str, name: &'static str, description: &'static str) -> StackOption {
    StackOption {
        id,
        name,
        description,
        default: false,
        experimental: false,
    }
}

const fn def(id: &'static str, name: &'static str, description: &'static str) -> StackOption {
    StackOption {
        default: true,
        ..opt(id, name, description)
    }
}

const fn exp(id: &'static str, name: &'static str, description: &'static str) -> StackOption {
    StackOption {
        experimental: true,
        ..opt(id, name, description)
    }
}

const WEB_FRONTEND: &[StackOption] = &[
    def(
        "tanstack-router",
        "TanStack Router",
        "Modern type-safe router for React",
    ),
    opt(
        "react-router",
        "React Router",
        "Declarative routing for React",
    ),
    opt(
        "tanstack-start",
        "TanStack Start",
        "Full-stack React framework powered by TanStack Router",
    ),
    opt("next", "Next.js", "React framework with hybrid rendering"),
    opt(
        "nuxt",
        "Nuxt",
        "Vue full-stack framework (SSR, SSG, hybrid)",
    ),
    opt("svelte", "Svelte", "Cybernetically enhanced web apps"),
    opt(
        "solid",
        "Solid",
        "Solid application with file-based routing and SSR",
    ),
    opt(
        "astro",
        "Astro",
        "The web framework for content-driven websites",
    ),
    opt("none", "No Web Frontend", "No web-based frontend"),
];

const NATIVE_FRONTEND: &[StackOption] = &[
    def(
        "native-bare",
        "Expo + Bare",
        "Expo with StyleSheet (no styling library)",
    ),
    opt(
        "native-uniwind",
        "Expo + Uniwind",
        "Fastest Tailwind bindings for React Native with HeroUI Native",
    ),
    opt(
        "native-unistyles",
        "Expo + Unistyles",
        "Expo with Unistyles (type-safe styling)",
    ),
    opt("none", "No Native Frontend", "No native mobile frontend"),
];

const RUNTIME: &[StackOption] = &[
    def("bun", "Bun", "Fast JavaScript runtime & toolkit"),
    opt("node", "Node.js", "JavaScript runtime environment"),
    opt(
        "workers",
        "Cloudflare Workers",
        "Serverless runtime for the edge",
    ),
    opt("none", "No Runtime", "No specific runtime"),
];

const BACKEND: &[StackOption] = &[
    def("hono", "Hono", "Ultrafast web framework"),
    opt("elysia", "Elysia", "TypeScript web framework"),
    opt("express", "Express", "Popular Node.js framework"),
    opt(
        "fastify",
        "Fastify",
        "Fast, low-overhead web framework for Node.js",
    ),
    opt("convex", "Convex", "Reactive backend-as-a-service"),
    opt(
        "self-next",
        "Fullstack Next.js",
        "Use Next.js built-in API routes",
    ),
    opt(
        "self-tanstack-start",
        "Fullstack TanStack Start",
        "Use TanStack Start's built-in API routes",
    ),
    opt(
        "self-nuxt",
        "Fullstack Nuxt",
        "Use Nuxt's built-in server routes",
    ),
    opt(
        "self-svelte",
        "Fullstack SvelteKit",
        "Use SvelteKit's built-in server routes",
    ),
    opt(
        "self-solid",
        "Fullstack Solid",
        "Use Solid file-based API routes",
    ),
    opt(
        "self-astro",
        "Fullstack Astro",
        "Use Astro's built-in API routes",
    ),
    opt(
        "none",
        "No Backend",
        "Skip backend integration (frontend only)",
    ),
];

const API: &[StackOption] = &[
    def("trpc", "tRPC", "End-to-end typesafe APIs"),
    opt("orpc", "oRPC", "Typesafe APIs Made Simple"),
    opt("none", "No API", "No API layer (API routes disabled)"),
];

const DATABASE: &[StackOption] = &[
    def("sqlite", "SQLite", "File-based SQL database"),
    opt("postgres", "PostgreSQL", "Advanced SQL database"),
    opt("mysql", "MySQL", "Popular relational database"),
    opt("mongodb", "MongoDB", "NoSQL document database"),
    opt("none", "No Database", "Skip database integration"),
];

const ORM: &[StackOption] = &[
    def("drizzle", "Drizzle", "TypeScript ORM"),
    opt("prisma", "Prisma", "Next-gen ORM"),
    opt("mongoose", "Mongoose", "Elegant object modeling tool"),
    opt("none", "No ORM", "Skip ORM integration"),
];

const DB_SETUP: &[StackOption] = &[
    opt(
        "turso",
        "Turso",
        "Distributed SQLite with edge replicas (libSQL)",
    ),
    opt(
        "d1",
        "Cloudflare D1",
        "Serverless SQLite-compatible database for Cloudflare Workers",
    ),
    opt(
        "neon",
        "Neon Postgres",
        "Serverless Postgres with autoscaling and branching",
    ),
    opt(
        "prisma-postgres",
        "Prisma PostgreSQL",
        "Managed Postgres via Prisma Data Platform",
    ),
    opt(
        "mongodb-atlas",
        "MongoDB Atlas",
        "Managed MongoDB clusters in the cloud",
    ),
    opt(
        "supabase",
        "Supabase",
        "Local Postgres stack via Supabase (Docker required)",
    ),
    opt(
        "planetscale",
        "PlanetScale",
        "Postgres & Vitess (MySQL) on NVMe",
    ),
    opt(
        "docker",
        "Docker",
        "Run Postgres/MySQL/MongoDB locally via Docker Compose",
    ),
    def("none", "Basic Setup", "No cloud DB integration"),
];

const DEPLOY: &[StackOption] = &[
    opt(
        "cloudflare",
        "Cloudflare",
        "Deploy to Cloudflare Workers using Alchemy",
    ),
    opt("prisma", "Prisma", "Deploy with Prisma using Alchemy"),
    opt(
        "docker",
        "Docker",
        "Self-host with a Dockerfile and docker-compose.yml",
    ),
    exp("vercel", "Vercel", "Deploy to Vercel with Services"),
    def("none", "None", "Skip deployment setup"),
];

const AUTH: &[StackOption] = &[
    def(
        "better-auth",
        "Better-Auth",
        "The most comprehensive authentication framework for TypeScript",
    ),
    opt(
        "clerk",
        "Clerk",
        "More than authentication, Complete User Management",
    ),
    opt("none", "No Auth", "Skip authentication"),
];

const PAYMENTS: &[StackOption] = &[
    opt(
        "polar",
        "Polar",
        "Turn your software into a business. 6 lines of code.",
    ),
    def("none", "No Payments", "Skip payments integration"),
];

const PACKAGE_MANAGER: &[StackOption] = &[
    opt("npm", "npm", "Default package manager"),
    opt("pnpm", "pnpm", "Fast, disk space efficient"),
    def("bun", "bun", "All-in-one toolkit"),
];

const ADDONS: &[StackOption] = &[
    opt(
        "pwa",
        "PWA (Progressive Web App)",
        "Make your app installable and work offline",
    ),
    opt(
        "tauri",
        "Tauri",
        "Package static web apps as native desktop apps",
    ),
    opt(
        "electrobun",
        "Electrobun",
        "Bundle static web apps in a lightweight desktop shell",
    ),
    opt("starlight", "Starlight", "Build stellar docs with astro"),
    opt("fumadocs", "Fumadocs", "Build excellent documentation site"),
    opt(
        "lefthook",
        "Lefthook",
        "Fast and powerful Git hooks manager",
    ),
    opt("husky", "Husky", "Modern native Git hooks made easy"),
    opt("biome", "Biome", "Format, lint, and more"),
    opt("oxlint", "Oxlint", "Oxlint + Oxfmt (linting & formatting)"),
    def("turborepo", "Turborepo", "High-performance build system"),
    opt("nx", "Nx", "Smart monorepo build system and task runner"),
    opt(
        "vite-plus",
        "Vite+",
        "Unified Vite toolchain, task runner, linting, formatting, and optional hooks",
    ),
    opt(
        "ultracite",
        "Ultracite",
        "A zero-config preset for ESLint, Biome, and Oxlint that helps AI write better code",
    ),
    opt("opentui", "OpenTUI", "Build terminal user interfaces"),
    opt("wxt", "WXT", "Build browser extensions"),
    opt(
        "skills",
        "Skills",
        "Install AI agent skills for coding assistants",
    ),
    opt("mcp", "MCP", "Install MCP servers for your agents/editors"),
    opt(
        "evlog",
        "evlog",
        "Structured request logging with Better Auth context",
    ),
    opt(
        "axiom",
        "Axiom",
        "Managed production observability through evlog and Alchemy",
    ),
];

const EXAMPLES: &[StackOption] = &[
    opt("todo", "Todo Example", "Simple todo application"),
    opt("ai", "AI Example", "AI integration example using AI SDK"),
];

const GIT: &[StackOption] = &[
    def("true", "Git", "Initialize Git repository"),
    opt("false", "No Git", "Skip Git initialization"),
];

const INSTALL: &[StackOption] = &[
    def(
        "true",
        "Install Dependencies",
        "Install packages automatically",
    ),
    opt("false", "Skip Install", "Skip dependency installation"),
];

/// The options of `category`, in upstream's order.
pub fn options(category: Category) -> &'static [StackOption] {
    match category {
        Category::WebFrontend => WEB_FRONTEND,
        Category::NativeFrontend => NATIVE_FRONTEND,
        Category::Backend => BACKEND,
        Category::Runtime => RUNTIME,
        Category::Api => API,
        Category::Database => DATABASE,
        Category::Orm => ORM,
        Category::DbSetup => DB_SETUP,
        Category::WebDeploy | Category::ServerDeploy => DEPLOY,
        Category::Auth => AUTH,
        Category::Payments => PAYMENTS,
        Category::PackageManager => PACKAGE_MANAGER,
        Category::Addons => ADDONS,
        Category::Examples => EXAMPLES,
        Category::Git => GIT,
        Category::Install => INSTALL,
    }
}

/// The option of `category` with upstream id `id`.
pub fn option(category: Category, id: &str) -> Option<&'static StackOption> {
    options(category).iter().find(|o| o.id == id)
}

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
    fn set(&mut self, category: Category, id: &str) -> bool {
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

/// A preset stack (upstream's `PRESET_TEMPLATES`, also the CLI's `--template`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// Upstream's presets, in its order.
pub const PRESETS: &[Preset] = &[
    Preset {
        id: "mern",
        name: "MERN Stack",
        description: "MongoDB + Express + React + Node.js - Classic MERN stack",
    },
    Preset {
        id: "pern",
        name: "PERN Stack",
        description: "PostgreSQL + Express + React + Node.js - Popular PERN stack",
    },
    Preset {
        id: "t3",
        name: "T3 Stack",
        description: "Next.js + tRPC + Prisma + PostgreSQL + Better Auth",
    },
    Preset {
        id: "uniwind",
        name: "Uniwind Native",
        description: "Expo + Uniwind native app with no backend services",
    },
];

impl Preset {
    /// The preset with id `id`.
    pub fn find(id: &str) -> Option<&'static Preset> {
        PRESETS
            .iter()
            .find(|p| p.id == id.trim().to_ascii_lowercase())
    }

    /// Its stack, with upstream's project name and package manager (bun).
    pub fn stack(&self) -> Stack {
        let base = Stack::default();
        match self.id {
            "mern" => Stack {
                web_frontend: WebFrontend::ReactRouter,
                runtime: Runtime::Node,
                backend: Backend::Express,
                database: Database::Mongodb,
                orm: Orm::Mongoose,
                db_setup: DbSetup::MongodbAtlas,
                examples: vec![Example::Todo],
                api: Api::Orpc,
                ..base
            },
            "pern" => Stack {
                runtime: Runtime::Node,
                backend: Backend::Express,
                database: Database::Postgres,
                examples: vec![Example::Todo],
                ..base
            },
            "t3" => Stack {
                web_frontend: WebFrontend::Next,
                runtime: Runtime::None,
                backend: Backend::SelfNext,
                database: Database::Postgres,
                orm: Orm::Prisma,
                addons: vec![Addon::Biome, Addon::Turborepo],
                ..base
            },
            _ => Stack {
                web_frontend: WebFrontend::None,
                native_frontend: NativeFrontend::Uniwind,
                runtime: Runtime::None,
                backend: Backend::None,
                database: Database::None,
                orm: Orm::None,
                auth: Auth::None,
                addons: Vec::new(),
                api: Api::None,
                ..base
            },
        }
    }
}

/// What picking option `id` of `category` makes of `stack`, as a click on its card does
/// upstream: nothing when it is disabled; a frontend toggles between itself and none; an addon
/// or an example toggles in its list (one task runner and one observability addon at most, the
/// list then sorted by id, as upstream sorts it); Git and Install toggle when picked again; any
/// other category takes it. The result is resolved, and its changes are what the rules did.
pub fn select(stack: &Stack, category: Category, id: &str) -> Resolved {
    let current = resolve(stack).stack;
    let mut next = current.clone();
    if disabled_reason(&current, category, id).is_none() {
        match category {
            Category::WebFrontend | Category::NativeFrontend => {
                let toggled = if current.is_picked(category, id) {
                    "none"
                } else {
                    id
                };
                next.set(category, toggled);
            }
            Category::Addons => {
                if let Some(addon) = Addon::parse(id) {
                    toggle(&mut next.addons, addon);
                    next.addons = rules::sanitize_addons(&next.addons);
                    next.addons.sort_by_key(|a| a.id());
                }
            }
            Category::Examples => {
                if let Some(example) = Example::parse(id) {
                    toggle(&mut next.examples, example);
                    next.examples = rules::dedupe(&next.examples);
                    next.examples.sort_by_key(|e| e.id());
                }
            }
            Category::Git if current.is_picked(category, id) => next.git = !current.git,
            Category::Install if current.is_picked(category, id) => next.install = !current.install,
            _ if !current.is_picked(category, id) => {
                next.set(category, id);
            }
            _ => {}
        }
    }
    resolve(&next)
}

/// What removing option `id` of `category` from the selected stack makes of `stack`, as the ×
/// on its tag does upstream: out of its list, or the category set to none when it has a none.
pub fn remove(stack: &Stack, category: Category, id: &str) -> Resolved {
    let current = resolve(stack).stack;
    let mut next = current.clone();
    match category {
        Category::Addons => next.addons.retain(|a| a.id() != id),
        Category::Examples => next.examples.retain(|e| e.id() != id),
        _ if current.is_picked(category, id) && option(category, "none").is_some() => {
            next.set(category, "none");
        }
        _ => {}
    }
    resolve(&next)
}

/// Takes `value` out of `list`, or adds it at the end (upstream's `toggleMulti`).
fn toggle<T: PartialEq>(list: &mut Vec<T>, value: T) {
    if list.contains(&value) {
        list.retain(|v| *v != value);
    } else {
        list.push(value);
    }
}

/// The stack as upstream's tags show it (`getSelectedTechs`): every option picked, in category
/// order, but none of the "none"s, "false"s, and Git or Install when on.
pub fn selected(stack: &Stack) -> Vec<(Category, &'static StackOption)> {
    let mut out = Vec::new();
    for &category in Category::ALL {
        for id in stack.picked(category) {
            let hidden = id == "none"
                || id == "false"
                || (matches!(category, Category::Git | Category::Install | Category::Auth)
                    && id == "true");
            if hidden {
                continue;
            }
            if let Some(option) = option(category, id) {
                out.push((category, option));
            }
        }
    }
    out
}

/// Upstream's `formatProjectName`: whitespace runs become dashes, and an empty name is
/// [`DEFAULT_NAME`].
pub fn format_project_name(name: &str) -> String {
    if name.is_empty() {
        return DEFAULT_NAME.into();
    }
    let mut out = String::new();
    let mut in_space = false;
    for c in name.chars() {
        if c.is_whitespace() {
            if !in_space {
                out.push('-');
            }
            in_space = true;
        } else {
            out.push(c);
            in_space = false;
        }
    }
    out
}

/// What is wrong with project name `name` (as [`format_project_name`] makes it), if anything:
/// upstream's `ProjectNameSchema` on its last path part, and the control characters the CLI
/// refuses.
pub fn validate_project_name(name: &str) -> Option<String> {
    let formatted = format_project_name(name);
    if formatted.chars().any(char::is_control) {
        return Some("Project name can't contain control characters".into());
    }
    let base = formatted
        .split(['/', '\\'])
        .rfind(|p| !p.is_empty())
        .unwrap_or("");
    if base.is_empty() {
        return Some("Project name cannot be empty".into());
    }
    if base.chars().count() > 255 {
        return Some("Project name must be less than 255 characters".into());
    }
    if base != "." && base.starts_with('.') {
        return Some("Project name cannot start with a dot (except for '.')".into());
    }
    if base != "." && base.starts_with('-') {
        return Some("Project name cannot start with a dash".into());
    }
    if base.contains(['<', '>', ':', '"', '|', '?', '*']) {
        return Some("Project name contains invalid characters".into());
    }
    if base.eq_ignore_ascii_case("node_modules") {
        return Some("Project name is reserved".into());
    }
    None
}

/// Upstream's `quoteShellArgument`: as it is when it is only letters, digits and `_./-`, else
/// in single quotes.
fn quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// Whether `stack` is upstream's default in everything but its name: the command is then the
/// runner, the name and `--yes`.
fn is_default_but_name(stack: &Stack) -> bool {
    let sorted = |mut ids: Vec<&'static str>| {
        ids.sort_unstable();
        ids
    };
    let default = Stack::default();
    Category::ALL
        .iter()
        .all(|&c| sorted(stack.picked(c)) == sorted(default.picked(c)))
}

/// The frontends the CLI's `--frontend` takes: the web and the native one that aren't none, or
/// `none` alone (upstream's `getStackFrontends`).
pub(crate) fn frontends(stack: &Stack) -> Vec<&'static str> {
    let picked: Vec<&'static str> = [stack.web_frontend.id(), stack.native_frontend.id()]
        .into_iter()
        .filter(|id| *id != "none")
        .collect();
    if picked.is_empty() {
        vec!["none"]
    } else {
        picked
    }
}

/// The command that creates `stack`'s project, exactly as upstream's builder writes it
/// (`generateStackCommand`), with the CLI pinned to [`CLI_VERSION`] instead of `@latest`. The
/// stack's package manager picks the runner: `bun create`, `pnpm create` or `npx`.
pub fn command(stack: &Stack) -> String {
    let base = stack.package_manager.runner();
    let name = quote(&format_project_name(&stack.project_name));
    if is_default_but_name(stack) {
        return format!("{base} {name} --yes");
    }
    let list = |ids: Vec<&'static str>| match ids.join(" ") {
        joined if joined.is_empty() => "none".to_string(),
        joined => joined,
    };
    let flags = [
        format!("--frontend {}", frontends(stack).join(" ")),
        format!("--backend {}", rules::core_backend(stack.backend)),
        format!("--runtime {}", stack.runtime.id()),
        format!("--api {}", stack.api.id()),
        format!("--auth {}", stack.auth.id()),
        format!("--payments {}", stack.payments.id()),
        format!("--database {}", stack.database.id()),
        format!("--orm {}", stack.orm.id()),
        format!("--db-setup {}", stack.db_setup.id()),
        format!("--package-manager {}", stack.package_manager.id()),
        if stack.git { "--git" } else { "--no-git" }.to_string(),
        format!("--web-deploy {}", stack.web_deploy.id()),
        format!("--server-deploy {}", stack.server_deploy.id()),
        if stack.install {
            "--install"
        } else {
            "--no-install"
        }
        .to_string(),
        format!(
            "--addons {}",
            list(stack.addons.iter().map(|a| a.id()).collect())
        ),
        format!(
            "--examples {}",
            list(stack.examples.iter().map(|e| e.id()).collect())
        ),
    ];
    format!("{base} {name} {}", flags.join(" "))
}

/// The flags [`run_command`] adds to [`command`] so the CLI never stops to ask: the folder
/// conflict (Sawit checks there is none first) and the "Open project with?" picker, and, for
/// the database setups that ask how to provision (and that Alchemy doesn't manage), manual
/// setup: the CLI then writes the `.env` and prints the steps.
pub fn quiet_flags(stack: &Stack) -> Vec<&'static str> {
    let mut flags = vec!["--directory-conflict", "error", "--open", "none"];
    if rules::asks_how_to_provision(stack) {
        flags.push("--manual-db");
    }
    flags
}

/// What Create Project runs for `stack`: [`command`], with npx told not to ask before it
/// downloads the CLI (`npx -y`), then [`quiet_flags`].
pub fn run_command(stack: &Stack) -> String {
    let command = command(stack);
    let command = match stack.package_manager {
        Pm::Npm => command.replacen("npx ", "npx -y ", 1),
        _ => command,
    };
    format!("{command} {}", quiet_flags(stack).join(" "))
}

/// A stack given by upstream's CLI flag names, any of them left out: what `sawit new` and the
/// MCP tools take. Unknown ids are refused with the ones there are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Choices {
    /// A preset (`mern`, `pern`, `t3`, `uniwind`) to start from instead of the default stack.
    pub template: Option<String>,
    /// The project name (or a path relative to where it is created).
    pub name: Option<String>,
    /// Web and native frontends, as `--frontend` takes them (one of each at most), or `none`.
    pub frontend: Option<Vec<String>>,
    /// A backend id; `self` is the fullstack backend of the web frontend.
    pub backend: Option<String>,
    pub runtime: Option<String>,
    pub api: Option<String>,
    pub database: Option<String>,
    pub orm: Option<String>,
    pub db_setup: Option<String>,
    pub web_deploy: Option<String>,
    pub server_deploy: Option<String>,
    pub auth: Option<String>,
    pub payments: Option<String>,
    pub addons: Option<Vec<String>>,
    pub examples: Option<Vec<String>>,
    pub git: Option<bool>,
    pub install: Option<bool>,
    pub package_manager: Option<String>,
}

/// `id` as one of `category`'s options, or why not.
fn pick<T>(category: Category, id: &str, parse: impl Fn(&str) -> Option<T>) -> Result<T, String> {
    let id = id.trim().to_ascii_lowercase();
    parse(&id).ok_or_else(|| {
        let ids: Vec<&str> = options(category).iter().map(|o| o.id).collect();
        format!(
            "“{id}” isn't one of the {} options: use one of {}.",
            category.label(),
            ids.join(", ")
        )
    })
}

/// A list's ids, `none` meaning an empty list.
fn pick_list<T: PartialEq>(
    category: Category,
    ids: &[String],
    parse: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, String> {
    let mut out = Vec::new();
    for id in ids.iter().flat_map(|i| i.split([',', ' '])) {
        let id = id.trim();
        if id.is_empty() || id.eq_ignore_ascii_case("none") {
            continue;
        }
        let value = pick(category, id, &parse)?;
        if !out.contains(&value) {
            out.push(value);
        }
    }
    Ok(out)
}

impl Choices {
    /// The stack these choices give: the preset's (or the default) with each choice given in
    /// its place, not yet resolved.
    pub fn stack(&self) -> Result<Stack, String> {
        let mut stack = match self.template.as_deref().map(str::trim) {
            None | Some("") | Some("none") => Stack::default(),
            Some(id) => Preset::find(id)
                .ok_or_else(|| {
                    format!("“{id}” isn't a preset: use one of mern, pern, t3, uniwind.")
                })?
                .stack(),
        };
        if let Some(name) = &self.name {
            stack.project_name = name.clone();
        }
        if let Some(frontends) = &self.frontend {
            let (mut web, mut native) = (Vec::new(), Vec::new());
            for id in frontends.iter().flat_map(|i| i.split([',', ' '])) {
                let id = id.trim().to_ascii_lowercase();
                if id.is_empty() || id == "none" {
                    continue;
                }
                if let Some(w) = WebFrontend::parse(&id) {
                    web.push(w);
                } else if let Some(n) = NativeFrontend::parse(&id) {
                    native.push(n);
                } else {
                    let ids: Vec<&str> = WEB_FRONTEND
                        .iter()
                        .chain(NATIVE_FRONTEND)
                        .map(|o| o.id)
                        .filter(|id| *id != "none")
                        .collect();
                    return Err(format!(
                        "“{id}” isn't a frontend: use one of {} or none.",
                        ids.join(", ")
                    ));
                }
            }
            if web.len() > 1 || native.len() > 1 {
                return Err(
                    "Pick one web frontend and one native frontend at most, as create-better-t-stack takes them."
                        .into(),
                );
            }
            stack.web_frontend = web.first().copied().unwrap_or(WebFrontend::None);
            stack.native_frontend = native.first().copied().unwrap_or(NativeFrontend::None);
        }
        if let Some(id) = &self.backend {
            stack.backend = if id.trim().eq_ignore_ascii_case("self") {
                rules::self_backend(stack.web_frontend).ok_or_else(|| {
                    "Backend self uses a fullstack web frontend's own routes: pick next, tanstack-start, nuxt, svelte, solid or astro as the frontend."
                        .to_string()
                })?
            } else {
                pick(Category::Backend, id, Backend::parse)?
            };
        }
        macro_rules! single {
            ($field:ident, $category:expr, $parse:expr) => {
                if let Some(id) = &self.$field {
                    stack.$field = pick($category, id, $parse)?;
                }
            };
        }
        single!(runtime, Category::Runtime, Runtime::parse);
        single!(api, Category::Api, Api::parse);
        single!(database, Category::Database, Database::parse);
        single!(orm, Category::Orm, Orm::parse);
        single!(db_setup, Category::DbSetup, DbSetup::parse);
        single!(web_deploy, Category::WebDeploy, Deploy::parse);
        single!(server_deploy, Category::ServerDeploy, Deploy::parse);
        single!(auth, Category::Auth, Auth::parse);
        single!(payments, Category::Payments, Payments::parse);
        single!(package_manager, Category::PackageManager, Pm::parse);
        if let Some(ids) = &self.addons {
            stack.addons = pick_list(Category::Addons, ids, Addon::parse)?;
        }
        if let Some(ids) = &self.examples {
            stack.examples = pick_list(Category::Examples, ids, Example::parse)?;
        }
        if let Some(git) = self.git {
            stack.git = git;
        }
        if let Some(install) = self.install {
            stack.install = install;
        }
        Ok(stack)
    }
}

/// Every reason an option of `stack` is disabled, by category: what the page greys and
/// `stack_check` reports.
pub fn disabled(stack: &Stack) -> Vec<(Category, &'static str, String)> {
    let mut out = Vec::new();
    for &category in Category::ALL {
        for option in options(category) {
            if let Some(reason) = disabled_reason(stack, category, option.id) {
                out.push((category, option.id, reason));
            }
        }
    }
    out
}
