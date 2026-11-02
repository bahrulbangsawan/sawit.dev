//! Upstream's preset stacks (`PRESET_TEMPLATES`, the CLI's `--template`).

use serde::Serialize;

use super::{
    Addon, Api, Auth, Backend, Database, DbSetup, Example, NativeFrontend, Orm, Runtime, Stack,
    WebFrontend,
};

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
