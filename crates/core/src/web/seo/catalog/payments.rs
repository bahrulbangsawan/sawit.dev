//! Agent readiness: payments (count when any one protocol is supported).

use super::super::{Layer, Tier};
use super::{Applies, CheckDef, agent, outside};
use Applies::All;
use Layer::Payments;
use Tier::Bonus;

pub(super) const PAYMENTS: &[CheckDef] = &[
    // Agent readiness: payments (count when any one protocol is supported).
    agent(
        "x402-support",
        "x402 payments",
        Payments,
        Bonus,
        2,
        All,
        "A paid resource answers 402 with x402 payment requirements, or /.well-known/x402 answers.",
        "Support x402 for agent payments where you sell to agents.",
        "https://www.x402.org/",
    ),
    agent(
        "mpp-support",
        "Machine payments",
        Payments,
        Bonus,
        2,
        All,
        "/.well-known/mpp answers, or a 402 asks for a Payment authorization.",
        "Support machine payments where you sell to agents.",
        "",
    ),
    agent(
        "ucp-support",
        "Universal commerce",
        Payments,
        Bonus,
        3,
        All,
        "/.well-known/ucp answers with the store's commerce profile.",
        "Publish a UCP profile at /.well-known/ucp.",
        "https://ucp.dev/",
    ),
    agent(
        "acp-support",
        "Agentic commerce",
        Payments,
        Bonus,
        3,
        All,
        "/.well-known/acp.json answers with the merchant's checkout.",
        "Publish your agentic checkout at /.well-known/acp.json.",
        "https://www.agenticcommerce.dev/",
    ),
    outside(agent(
        "ap2-support",
        "Agent payments protocol",
        Payments,
        Bonus,
        3,
        All,
        "Whether the site supports the agent payments protocol.",
        "Support it where you sell to agents.",
        "",
    )),
];
