//! Upstream's compatibility rules: `analyzeStackCompatibility` (the auto-adjustments, branch
//! for branch, with upstream's messages), the fixed point that runs it, `getDisabledReason`,
//! the list sanitizing, and the helpers of `packages/types/src/compatibility.ts` they use.

mod analyze;
mod compat;
mod disabled;

use super::{Addon, Backend, DbSetup, Deploy, Resolved, Stack, WebFrontend};
use analyze::analyze;

pub use disabled::disabled_reason;

/// Upstream's `MAX_COMPATIBILITY_PASSES`.
const MAX_PASSES: usize = 10;

const TASK_RUNNERS: [Addon; 3] = [Addon::Turborepo, Addon::Nx, Addon::VitePlus];
const OBSERVABILITY: [Addon; 2] = [Addon::Evlog, Addon::Axiom];
const STATIC_DESKTOP: [Addon; 2] = [Addon::Tauri, Addon::Electrobun];

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
