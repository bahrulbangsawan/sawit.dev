//! The catalog: every category's options, as upstream's builder shows their cards, in its order.

use serde::Serialize;

use super::Category;

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

pub(super) const WEB_FRONTEND: &[StackOption] = &[
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

pub(super) const NATIVE_FRONTEND: &[StackOption] = &[
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
