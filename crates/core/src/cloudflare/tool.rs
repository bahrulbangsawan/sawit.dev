//! What serves a Worker in development, and the versions of it that Local Explorer needs.

use std::cmp::Ordering;
use std::fmt;

use super::VITE_PLUGIN;

/// What serves a Worker in development, and so Local Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DevTool {
    /// `wrangler dev`.
    Wrangler,
    /// `vite` with `@cloudflare/vite-plugin`.
    VitePlugin,
    /// `cf dev` for a `cloudflare.config.ts` project that declares neither of the others, which
    /// then starts one of them itself.
    CfCli,
}

impl DevTool {
    /// Its npm package, whose `package.json` in `node_modules` says the installed version.
    pub fn package(self) -> &'static str {
        match self {
            DevTool::Wrangler => "wrangler",
            DevTool::VitePlugin => VITE_PLUGIN,
            DevTool::CfCli => "cf",
        }
    }

    /// Its name in a sentence.
    pub fn name(self) -> &'static str {
        match self {
            DevTool::Wrangler => "Wrangler",
            DevTool::VitePlugin => "the Cloudflare Vite plugin",
            DevTool::CfCli => "cf",
        }
    }

    /// The first version whose dev server serves Local Explorer. `cf` has none of its own: the
    /// tool it starts needs this.
    pub fn explorer_since(self) -> Option<Version> {
        match self {
            DevTool::Wrangler => Some(Version::new(4, 118, 0)),
            DevTool::VitePlugin => Some(Version::new(1, 50, 0)),
            DevTool::CfCli => None,
        }
    }

    /// The first version whose Local Explorer can invoke a Worker's Cron Triggers.
    pub fn cron_since(self) -> Option<Version> {
        match self {
            DevTool::Wrangler => Some(Version::new(4, 139, 0)),
            DevTool::VitePlugin => Some(Version::new(1, 60, 0)),
            DevTool::CfCli => None,
        }
    }
}

/// An npm package's version, as far as Local Explorer's requirements need one: `4.118.0`, with a
/// pre-release (`4.118.0-beta.1`) before the release it leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: bool,
}

impl Version {
    pub const fn new(major: u64, minor: u64, patch: u64) -> Version {
        Version {
            major,
            minor,
            patch,
            pre: false,
        }
    }

    /// `4.118.0`, `v4.118`, `4.118.0-beta.1` or `4.118.0+build`; a missing minor or patch is 0.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim().trim_start_matches(['v', 'V', '=']);
        // Build metadata (`+build.7`) doesn't order versions.
        let core = text.split_once('+').map_or(text, |(core, _)| core);
        let (core, pre) = match core.split_once('-') {
            Some((core, pre)) => (core, !pre.is_empty()),
            None => (core, false),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
        if parts.next().is_some() {
            return None;
        }
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            // A pre-release comes before its release.
            .then((!self.pre).cmp(&!other.pre))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
