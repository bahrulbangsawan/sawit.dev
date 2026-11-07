//! Every check the scan runs, in one table keyed by id: its layer, tier, points, which sites it
//! applies to, and how to fix it. The search and sharing checks and the agent-readiness checks
//! share entries where they test the same thing (`robots.txt`, the sitemap, `llms.txt`, JSON-LD),
//! so nothing runs twice. `docs/notes/2026-11-07-seo-agent-checks.md` lists the table.

use super::{Layer, Tier};

mod access;
mod discovery;
mod mcp;
mod payments;
mod search;
mod usability;

/// Which kinds of site a check applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applies {
    All,
    /// Content sites (and "all").
    Content,
    /// APIs and apps (and "all").
    Api,
}

/// One check.
#[derive(Clone, Copy, Debug)]
pub struct CheckDef {
    pub id: &'static str,
    pub name: &'static str,
    pub layer: Layer,
    pub tier: Tier,
    pub max: u8,
    pub applies: Applies,
    /// Listed with the search and sharing checks too.
    pub seo: bool,
    /// Needs data Sawit can't get itself (search ranking, directories): only the external scan
    /// answers it.
    pub external: bool,
    /// What it fetches and how it passes.
    pub what: &'static str,
    pub fix: &'static str,
    pub spec: &'static str,
}

const fn seo(
    id: &'static str,
    name: &'static str,
    tier: Tier,
    max: u8,
    what: &'static str,
    fix: &'static str,
    spec: &'static str,
) -> CheckDef {
    CheckDef {
        id,
        name,
        layer: Layer::Seo,
        tier,
        max,
        applies: Applies::All,
        seo: true,
        external: false,
        what,
        fix,
        spec,
    }
}

/// A check in an agent layer.
#[allow(clippy::too_many_arguments)]
const fn agent(
    id: &'static str,
    name: &'static str,
    layer: Layer,
    tier: Tier,
    max: u8,
    applies: Applies,
    what: &'static str,
    fix: &'static str,
    spec: &'static str,
) -> CheckDef {
    CheckDef {
        id,
        name,
        layer,
        tier,
        max,
        applies,
        seo: false,
        external: false,
        what,
        fix,
        spec,
    }
}

/// An agent-layer check that is also a search and sharing check.
const fn shared(def: CheckDef) -> CheckDef {
    CheckDef { seo: true, ..def }
}

/// An agent-layer check only the external scan answers.
const fn outside(def: CheckDef) -> CheckDef {
    CheckDef {
        external: true,
        ..def
    }
}

/// The table's parts, in order.
const PARTS: &[&[CheckDef]] = &[
    search::SEARCH,
    discovery::DISCOVERY,
    access::ACCESS,
    mcp::MCP,
    usability::USABILITY,
    payments::PAYMENTS,
];

/// How many checks the parts hold together.
const COUNT: usize = {
    let mut n = 0;
    let mut i = 0;
    while i < PARTS.len() {
        n += PARTS[i].len();
        i += 1;
    }
    n
};

/// The parts joined into one table, in order.
const ALL: [CheckDef; COUNT] = {
    let mut all = [PARTS[0][0]; COUNT];
    let mut n = 0;
    let mut i = 0;
    while i < PARTS.len() {
        let mut j = 0;
        while j < PARTS[i].len() {
            all[n] = PARTS[i][j];
            n += 1;
            j += 1;
        }
        i += 1;
    }
    all
};

pub const CHECKS: &[CheckDef] = &ALL;

/// The check with `id`.
pub fn def(id: &str) -> Option<&'static CheckDef> {
    CHECKS.iter().find(|c| c.id == id)
}
