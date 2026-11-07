//! Payments: can agents pay the site? The agent payment protocols, asked only of sites that sell.

use super::super::CheckResult;
use super::super::checks::{fail, json_ld, na, pass, seen, types_of};
use super::super::site::Site;
use super::With;
use crate::web::Fetched;

pub(super) async fn payments(site: &Site) -> Vec<CheckResult> {
    let h = &site.head;
    let types: Vec<String> = json_ld(&h.json_ld).iter().flat_map(types_of).collect();
    let commerce = types.iter().any(|t| {
        matches!(
            t.as_str(),
            "Product" | "Offer" | "AggregateOffer" | "Store" | "OnlineStore"
        )
    }) || h.anchors.iter().any(|a| {
        let href = a.href.to_ascii_lowercase();
        ["/cart", "/checkout"].iter().any(|w| href.contains(w))
    });
    let (x402, mpp, ucp, acp) = futures::join!(
        site.get("/.well-known/x402"),
        site.get("/.well-known/mpp"),
        site.json("/.well-known/ucp"),
        site.json("/.well-known/acp.json"),
    );
    let real = |g: &Fetched| g.ok() && !g.mime().contains("html");
    let any = real(&x402) || real(&mpp) || ucp.1.is_some() || acp.1.is_some();
    if !commerce && !any {
        let why = "No commerce on the site: agent payments are optional here.";
        return vec![
            na("x402-support", why),
            na("mpp-support", why),
            na("ucp-support", why),
            na("acp-support", why),
        ];
    }
    let one = |id: &str, ok: bool, got: &Fetched, what: &str| {
        if ok {
            pass(id, format!("{what} answers.")).with(seen(got, ""))
        } else {
            fail(id, format!("No {what}.")).with(seen(got, ""))
        }
    };
    vec![
        one("x402-support", real(&x402), &x402, "/.well-known/x402"),
        one("mpp-support", real(&mpp), &mpp, "/.well-known/mpp"),
        one("ucp-support", ucp.1.is_some(), &ucp.0, "/.well-known/ucp"),
        one(
            "acp-support",
            acp.1.is_some(),
            &acp.0,
            "/.well-known/acp.json",
        ),
    ]
}
