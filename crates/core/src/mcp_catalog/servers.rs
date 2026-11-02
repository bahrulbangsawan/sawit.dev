//! Every server in the catalog, in the MCP page's order: the bundled `sawit-mcp` first, then the
//! remote servers and those run on this Mac.

use super::{Auth, GITHUB_ID, Server, Source};

pub const SERVERS: &[Server] = &[
    Server {
        id: crate::mcp_clients::SERVER_NAME,
        name: "Sawit",
        about: "Drive Sawit from your agent: databases (read and write), terminals, git, the canvas, the browser, ports, SSH and more.",
        source: Source::Bundled,
        auth: Auth::None,
        note: None,
        logo: None,
    },
    Server {
        id: GITHUB_ID,
        name: "GitHub",
        about: "GitHub's official server: repositories, issues, pull requests, Actions and code search, as you.",
        source: Source::Http {
            url: "https://api.githubcopilot.com/mcp/",
            headers: &[],
        },
        auth: Auth::GitHub,
        note: Some(
            "Uses Sawit's GitHub sign-in, handed to each client by `sawit github token` as it connects: no config holds the token. VS Code and Copilot CLI sign in to GitHub themselves.",
        ),
        logo: Some(("github", false)),
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
