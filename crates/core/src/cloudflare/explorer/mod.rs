//! Cloudflare's Local Explorer: the page a Workers dev server (`wrangler dev`, `vite` with the
//! Cloudflare plugin, `cf dev`) serves on its own origin at `/cdn-cgi/local/explorer`, to browse
//! and edit the data of its local bindings, and the API beside it at
//! `/cdn-cgi/local/explorer/api`, whose `GET` answers with its OpenAPI document.
//!
//! Nothing announces a running one, so Sawit asks: each dev server the Ports panel lists is sent
//! `GET /cdn-cgi/local/explorer/api` on loopback, and one that answers with an OpenAPI document
//! serves an explorer. Answers are kept per port and process ([`Cache`]), so a server is asked
//! again only after a while, or once another process holds its port. Only loopback addresses are
//! ever asked or called: a dev server bound to the LAN alone is left alone, and so is every other
//! host. The docs say nothing of authentication, and the API needs none on loopback.

mod api;
mod http;
mod probe;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::time::Duration;

use crate::ports::BindAddr;

pub use api::{
    LocalWorker, Method, Response, api_path, check_confirmed, local_workers, parse_workers, request,
};
pub use probe::{Cache, Spec, find, find_with, may_serve, parse_spec, probe};

/// Where a dev server serves Local Explorer's page.
pub const PAGE_PATH: &str = "/cdn-cgi/local/explorer";
/// Where it serves Local Explorer's API.
pub const API_PATH: &str = "/cdn-cgi/local/explorer/api";

/// How long one API call gets: a D1 query or an R2 upload can take a moment.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
/// At most this much of an API answer is read; the rest is cut and said to be.
pub const BODY_LIMIT: usize = 8 << 20;

/// A running Local Explorer: a dev server that answered the probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explorer {
    pub port: u16,
    /// The loopback address it answered on.
    pub addr: SocketAddr,
    /// The dev server's process: the command it was started with, as the Ports panel groups it.
    pub pid: u32,
    /// That command (`pnpm dev`).
    pub label: String,
    /// The folder it runs in.
    pub cwd: Option<String>,
    pub spec: Spec,
}

impl Explorer {
    /// The page, as Sawit's browser opens it.
    pub fn url(&self) -> String {
        page_url(self.port)
    }

    /// The API's root.
    pub fn api_url(&self) -> String {
        format!("http://localhost:{}{API_PATH}", self.port)
    }

    /// Whether the dev server runs from inside `folder` (a project, or one of its packages).
    /// `lsof` gives the folder with its links resolved (`/private/tmp/…` for `/tmp/…`), so
    /// `folder` is taken both ways.
    pub fn runs_in(&self, folder: &Path) -> bool {
        let Some(cwd) = self.cwd.as_deref().map(Path::new) else {
            return false;
        };
        cwd.starts_with(folder) || cwd.starts_with(crate::files::resolve(folder))
    }
}

/// Local Explorer's page on the dev server at `port`.
pub fn page_url(port: u16) -> String {
    format!("http://localhost:{port}{PAGE_PATH}")
}

/// The loopback address to reach a server bound to `bind` on: 127.0.0.1 for IPv4 binds, ::1 for
/// IPv6 ones (Vite listens on `localhost`, often ::1 alone). `None` for a server bound to another
/// address only, which Sawit never asks.
pub fn loopback(bind: BindAddr) -> Option<IpAddr> {
    match bind {
        BindAddr::AnyV4 | BindAddr::LoopbackV4 => Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        BindAddr::AnyV6 | BindAddr::LoopbackV6 => Some(IpAddr::V6(Ipv6Addr::LOCALHOST)),
        BindAddr::Specific(ip) if ip.is_loopback() => Some(ip),
        BindAddr::Specific(_) => None,
    }
}

/// The explorers of `explorers` whose dev server runs inside `folder`.
pub fn running_in<'a>(explorers: &'a [Explorer], folder: &Path) -> Vec<&'a Explorer> {
    explorers.iter().filter(|e| e.runs_in(folder)).collect()
}

#[cfg(test)]
mod tests;
