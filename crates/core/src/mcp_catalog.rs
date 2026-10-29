//! The MCP servers the MCP page offers to install, besides the bundled
//! `sawit-mcp`. Names match what each service documents, so an entry added
//! earlier by hand (or by `claude mcp add`) is recognised as installed.

use std::path::{Path, PathBuf};

use crate::mcp_clients::Spec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The `sawit-mcp` next to the app.
    Bundled,
    /// A remote server. `{region}` in the URL is filled in at install time.
    Http {
        url: &'static str,
        headers: &'static [(&'static str, &'static str)],
    },
    /// A program run on this Mac. `command` is looked up in `PATH`.
    Stdio {
        command: &'static str,
        args: &'static [&'static str],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// Nothing to sign in to.
    None,
    /// The client signs in through the browser (OAuth).
    Browser,
    /// A key in this environment variable, set before starting the client.
    EnvKey(&'static str),
    /// The program keeps its own sign-in (e.g. `cap login`).
    Program,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Server {
    /// The name in every client's config.
    pub id: &'static str,
    pub name: &'static str,
    pub about: &'static str,
    pub source: Source,
    pub auth: Auth,
    /// A requirement or caveat shown with the server.
    pub note: Option<&'static str>,
    /// The file in `assets/agent_logos`, if there is one.
    pub logo: Option<(&'static str, bool)>,
}

/// The region the AWS MCP Server URL uses when none is given.
pub const DEFAULT_AWS_REGION: &str = "us-east-1";

pub const SERVERS: &[Server] = &[
    Server {
        id: crate::mcp_clients::SERVER_NAME,
        name: "Sawit",
        about: "Instances, read-only queries and DATABASE_URL from this app.",
        source: Source::Bundled,
        auth: Auth::None,
        note: None,
        logo: None,
    },
    Server {
        id: "aws",
        name: "AWS",
        about: "The AWS MCP Server: AWS APIs and documentation for your account.",
        source: Source::Http {
            url: "https://aws-mcp.{region}.api.aws/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: Some(
            "Signing in needs the AWSMCPSignInOAuthAccessPolicy managed policy on your IAM user or role.",
        ),
        logo: Some(("aws", false)),
    },
    Server {
        id: "cloudflare-api",
        name: "Cloudflare API",
        about: "DNS, Workers, R2, D1 and the rest of your Cloudflare account.",
        source: Source::Http {
            url: "https://mcp.cloudflare.com/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: Some(("cloudflare-color", true)),
    },
    Server {
        id: "cloudflare-bindings",
        name: "Cloudflare Developer Platform",
        about: "Workers, KV, R2, D1 and Hyperdrive bindings.",
        source: Source::Http {
            url: "https://bindings.mcp.cloudflare.com/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: Some(("cloudflare-color", true)),
    },
    Server {
        id: "neon",
        name: "Neon",
        about: "Neon Postgres projects, branches, schemas and queries.",
        source: Source::Http {
            url: "https://mcp.neon.tech/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: Some("Neon recommends it for development, not for production databases."),
        logo: None,
    },
    Server {
        id: "postman",
        name: "Postman",
        about: "Workspaces, collections, environments and mocks in Postman.",
        source: Source::Http {
            url: "https://mcp.postman.com/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: None,
    },
    Server {
        id: "figma",
        name: "Figma",
        about: "Read designs, variables and components from Figma files.",
        source: Source::Http {
            url: "https://mcp.figma.com/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: Some(("figma-color", true)),
    },
    Server {
        id: "mobbin",
        name: "Mobbin",
        about: "Search real app screens and flows for design references.",
        source: Source::Http {
            url: "https://api.mobbin.com/mcp",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: None,
    },
    Server {
        id: "exa",
        name: "Exa",
        about: "Web search and page contents for agents.",
        source: Source::Http {
            url: "https://mcp.exa.ai/mcp",
            headers: &[],
        },
        auth: Auth::None,
        note: None,
        logo: Some(("exa-color", true)),
    },
    Server {
        id: "polar-sandbox",
        name: "Polar Sandbox",
        about: "Polar products, checkouts and customers, in the sandbox.",
        source: Source::Http {
            url: "https://mcp.polar.sh/mcp/polar-sandbox",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: None,
    },
    Server {
        id: "magnific",
        name: "Magnific",
        about: "Upscale and generate images with Magnific.",
        source: Source::Http {
            url: "https://mcp.magnific.com",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: None,
    },
    Server {
        id: "epidemic-sound",
        name: "Epidemic Sound",
        about: "Search and download music, sound effects and voiceovers.",
        source: Source::Http {
            url: "https://www.epidemicsound.com/a/mcp-service/mcp",
            headers: &[("Authorization", "Bearer ${EPIDEMIC_SOUND_MCP_API_KEY}")],
        },
        auth: Auth::EnvKey("EPIDEMIC_SOUND_MCP_API_KEY"),
        note: None,
        logo: None,
    },
    Server {
        id: "meta_social_technologies",
        name: "Meta Developer Tools",
        about: "Meta's hosted developer tools.",
        source: Source::Http {
            url: "https://mcp.facebook.com/devtools",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: Some(("meta-color", true)),
    },
    Server {
        id: "whatsapp_business_tools",
        name: "WhatsApp Business",
        about: "WhatsApp Business tools from Meta.",
        source: Source::Http {
            url: "https://mcp.facebook.com/whatsapp_business_tools",
            headers: &[],
        },
        auth: Auth::Browser,
        note: None,
        logo: Some(("meta-color", true)),
    },
    Server {
        id: "api-anything",
        name: "api-anything",
        about: "Call websites' own data requests instead of a browser.",
        source: Source::Stdio {
            command: "npx",
            args: &["-y", "api-anything", "mcp"],
        },
        auth: Auth::None,
        note: Some("Needs Node.js (npx)."),
        logo: None,
    },
    Server {
        id: "cap",
        name: "Cap",
        about: "Your Cap recordings, transcripts, comments and sharing.",
        source: Source::Stdio {
            command: "cap",
            args: &["mcp", "serve"],
        },
        auth: Auth::Program,
        note: Some("Needs the cap command from Cap Desktop."),
        logo: None,
    },
    Server {
        id: "rea",
        name: "REA",
        about: "Reverse-engineer binaries, bundles, browser pages and apps.",
        source: Source::Stdio {
            command: "npx",
            args: &["-y", "rea-agents@6.3.0", "mcp"],
        },
        auth: Auth::None,
        note: Some("Needs Node.js (npx)."),
        logo: None,
    },
];

impl Server {
    pub fn is_remote(&self) -> bool {
        matches!(self.source, Source::Http { .. })
    }

    /// Whether the URL has a `{region}` to fill in.
    pub fn has_region(&self) -> bool {
        matches!(self.source, Source::Http { url, .. } if url.contains("{region}"))
    }

    /// The URL or command line, for display.
    pub fn address(&self, region: &str) -> String {
        match self.source {
            Source::Bundled => "built in".into(),
            Source::Http { url, .. } => url.replace("{region}", region),
            Source::Stdio { command, args } => format!("{command} {}", args.join(" ")),
        }
    }

    /// The program a local server needs, when it isn't found.
    pub fn missing_program(&self, find: impl Fn(&str) -> Option<PathBuf>) -> Option<&'static str> {
        match self.source {
            Source::Stdio { command, .. } if find(command).is_none() => Some(command),
            _ => None,
        }
    }

    /// What to install. `find` looks a program up in `PATH`: GUI clients get
    /// a minimal `PATH`, so commands are written as absolute paths when found.
    pub fn spec(
        &self,
        server: &Path,
        region: &str,
        find: impl Fn(&str) -> Option<PathBuf>,
    ) -> Spec {
        let resolve = |cmd: &str| {
            find(cmd)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| cmd.to_string())
        };
        let spec = match self.source {
            Source::Bundled => Spec::sawit(server),
            Source::Http { url, headers } => headers.iter().fold(
                Spec::http(self.id, url.replace("{region}", region)),
                |spec, (k, v)| spec.header(k, v),
            ),
            Source::Stdio { command, args } => Spec::stdio(
                self.id,
                resolve(command),
                args.iter().map(|a| a.to_string()).collect(),
            ),
        };
        spec.with_npx(resolve("npx"))
    }
}

pub fn get(id: &str) -> Option<&'static Server> {
    SERVERS.iter().find(|s| s.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_specs_build() {
        let mut ids: Vec<_> = SERVERS.iter().map(|s| s.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), SERVERS.len());
        let none = |_: &str| None;
        for s in SERVERS {
            let spec = s.spec(Path::new("/bin/sawit-mcp"), "eu-west-1", none);
            assert_eq!(spec.name, s.id);
            assert_eq!(spec.is_remote(), s.is_remote());
        }
    }

    #[test]
    fn region_and_paths_are_filled_in() {
        let aws = get("aws").unwrap();
        assert!(aws.has_region());
        assert_eq!(
            aws.address("eu-west-1"),
            "https://aws-mcp.eu-west-1.api.aws/mcp"
        );
        let find = |c: &str| (c == "npx").then(|| PathBuf::from("/opt/homebrew/bin/npx"));
        let rea = get("rea")
            .unwrap()
            .spec(Path::new("/x"), DEFAULT_AWS_REGION, find);
        assert!(matches!(
            rea.transport,
            crate::mcp_clients::Transport::Stdio { ref command, .. } if command == "/opt/homebrew/bin/npx"
        ));
        assert_eq!(rea.npx, "/opt/homebrew/bin/npx");
        assert_eq!(get("cap").unwrap().missing_program(find), Some("cap"));
        assert_eq!(get("rea").unwrap().missing_program(find), None);
    }
}
