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

mod catalog;
mod choices;
mod command;
mod picks;
mod presets;
mod rules;
mod stack;
#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use catalog::{StackOption, option, options};
pub use choices::Choices;
pub(crate) use command::frontends;
pub use command::{command, format_project_name, quiet_flags, run_command, validate_project_name};
pub use picks::{disabled, remove, select, selected};
pub use presets::{PRESETS, Preset};
pub use rules::{disabled_reason, resolve};
pub use stack::{Change, Resolved, Stack};

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
