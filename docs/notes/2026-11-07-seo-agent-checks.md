# SEO and agent-readiness checks (B8, B8.1)

The browser's SEO panel and `browser_seo` run one table of checks, keyed by id
(`crates/core/src/web/seo/catalog.rs`). 132 checks: the search and sharing checks (layer SEO), and
the agent-readiness checks in four layers, Discovery (weight 20), Access (30), Usability (40) and
Payments (10). Checks that test the same thing for search engines and for agents (robots.txt, the
sitemap, llms.txt, JSON-LD, noindex, metadata) are one entry, shown in both lists.

## How it scores

- A check passes (all its points), warns (part of them) or fails (none). Checks that don't apply
  (no MCP server to probe, no commerce, no hreflang) and checks not run are left out.
- **Search and sharing:** points earned over points possible among the SEO checks.
- **Layers:** earned (bonus checks add, never cost) over the possible points of required and
  recommended checks; emerging checks are shown but left out; a layer is capped at 100%.
  Payments count when any one protocol is supported.
- **Agent readiness:** the layers' percentages weighted 20/30/40/10, over the layers that apply;
  graded A+ (95+), A (90+), B (75+), C (45+), D (30+), F.
- **Essentials** (content sites): required checks 80 points, recommended 20, up to 5 more from
  bonus and emerging checks.
- **Scan type:** all, content (API checks left out) or api (content checks left out).
- **External score** (optional, public addresses only): an outside scanner's HTTP API; its total
  is shown beside Sawit's and its results fill the checks marked "external scan" below. Local
  and private addresses are never sent.

## Compared with an outside agent-readiness scanner (2026-11-07)

On the checks both run: kyzn.life 77 of 95 the same before tuning, bahrul.me 80 of 94; the
differences left are mostly where the outside scanner probes more places (a docs MCP server, a
blog post that mentions batch uploads) or reads differently (it wants 3 extended schema types for
a pass). Sawit's total for kyzn.life was 49, the same as the outside scanner's 49.

## The checks

| id | name | layer | tier | points | applies to | SEO list | answered by | what it checks |
|---|---|---|---|---|---|---|---|---|
| `title` | Page title | SEO | Required | 2 | All | yes | Sawit | The page's <title>: present, 10 to 60 characters. |
| `meta-description` | Meta description | SEO | Required | 2 | All | yes | Sawit | <meta name="description">: present, 50 to 160 characters. |
| `canonical` | Canonical address | SEO | Required | 1 | All | yes | Sawit | <link rel="canonical">: present, absolute, on this site. |
| `noindex-directives` | Not hidden from search | Discovery | Recommended | 1 | All | yes | Sawit | No noindex in the robots, googlebot or bingbot meta tags, nor in an X-Robots-Tag header. |
| `robots-txt` | robots.txt | SEO | Required | 1 | All | yes | Sawit | /robots.txt answers, parses, and doesn't block the whole site for every crawler. |
| `hreflang` | Language alternates | SEO | Recommended | 1 | All | yes | Sawit | <link rel="alternate" hreflang>: valid codes, absolute addresses, the page itself and x-default among them. |
| `headings` | Headings outline | SEO | Required | 2 | All | yes | Sawit | Exactly one <h1>, and no heading level skipped. |
| `favicon` | Favicon | SEO | Required | 1 | All | yes | Sawit | An icon from <link rel="icon"> or /favicon.ico loads as a picture. |
| `apple-touch-icon` | Touch icon | SEO | Recommended | 1 | All | yes | Sawit | <link rel="apple-touch-icon"> names a 180×180 icon that loads. |
| `web-manifest` | Web app manifest | SEO | Recommended | 1 | All | yes | Sawit | <link rel="manifest"> names a JSON manifest with a name and icons. |
| `open-graph` | Open Graph tags | SEO | Required | 2 | All | yes | Sawit | og:title, og:description, og:image, og:url and og:type are set. |
| `og-image` | Share image | SEO | Required | 2 | All | yes | Sawit | The og:image loads as a picture, at least 1200×630, under 8 MB. |
| `twitter-card` | X card | SEO | Recommended | 1 | All | yes | Sawit | <meta name="twitter:card"> is set (summary_large_image for a big preview). |
| `json-ld` | JSON-LD structured data | Access | Required | 4 | Content | yes | Sawit | <script type="application/ld+json"> blocks parse, name a @type, and describe the site's entity (Organization, Person, WebSite) with a name and description. |
| `sitemap` | Sitemap | Access | Required | 2 | Content | yes | Sawit | /sitemap.xml (or one robots.txt names) is a valid URL set or sitemap index with entries. |
| `sitemap-md` | Markdown sitemap | SEO | Bonus | 1 | All | yes | Sawit | /sitemap.md answers with Markdown links to the site's pages. |
| `llms-txt-exists` | llms.txt | Access | Required | 1 | Content | yes | Sawit | /llms.txt answers with text, not the site's HTML. |
| `llms-txt-formatting` | llms.txt layout | Access | Recommended | 2 | Content | yes | Sawit | llms.txt starts with an # H1, has a > summary, ## sections and Markdown links. |
| `llms-full-txt` | llms-full.txt | SEO | Recommended | 1 | All | yes | Sawit | /llms-full.txt (or /full-llms.txt) answers with the site's content as Markdown. |
| `image-alt` | Image alt text | SEO | Recommended | 2 | All | yes | Sawit | Every <img> has an alt attribute (empty for decorative images). |
| `broken-links` | Broken links | SEO | Recommended | 2 | All | yes | Sawit | Up to 30 of the page's links answer below 400. |
| `https` | HTTPS | SEO | Required | 1 | All | yes | Sawit | The page is served over HTTPS (local addresses don't need it). |
| `viewport` | Mobile viewport | SEO | Recommended | 1 | All | yes | Sawit | <meta name="viewport" content="width=device-width…"> is set. |
| `metadata-completeness` | Metadata completeness | Access | Required | 2 | Content | yes | Sawit | A canonical address, <html lang>, og:image and og:type are all present. |
| `ard-catalog` | Agent resource catalog | Discovery | Required | 1 | All |  | Sawit | /.well-known/ard.json (or a rel="ard" link or Link header) lists what the site offers agents: MCP servers, APIs, skills. The older /.well-known/ai-catalog.json counts with a warning. |
| `ai-catalog-published` | AI catalog | Discovery | Bonus | 1 | All |  | Sawit | /.well-known/ai-catalog.json answers with a valid catalog. |
| `ard-entries-valid` | Catalog entries complete | Discovery | Bonus | 2 | All |  | Sawit | Each catalog entry has an identifier, a name, a media type and a target URL. |
| `ard-trust-manifest` | Catalog trust metadata | Discovery | Bonus | 2 | All |  | Sawit | At least one catalog entry carries a trustManifest (identity, attestations or a signature). |
| `robots-ai-policy-quality` | AI crawler policy | Discovery | Required | 2 | Content |  | Sawit | robots.txt lets AI search crawlers (OAI-SearchBot, PerplexityBot, Claude-SearchBot) read the home page; blocking training crawlers is a choice and fine. |
| `dns-aid` | DNS agent discovery | Discovery | Emerging | 1 | All |  | Sawit | A TXT record at _agents.<domain> points agents to the site's agent resources (production addresses only). |
| `registry-branding` | MCP server branding | Discovery | Emerging | 2 | All |  | Sawit | The MCP server card names the server, describes it and gives an icon. |
| `agentic-search-usecase` | Found for its category | Discovery | Recommended | 6 | Content |  | external scan | Whether answer engines name the site when asked about what it does. |
| `agentic-search-specific` | Developer resources found | Discovery | Recommended | 3 | Content |  | external scan | Whether a search for the site's docs finds its own docs. |
| `brand-search-accuracy` | Found by its name | Discovery | Required | 3 | Content |  | external scan | Whether a search for the brand ranks the site first. |
| `wikipedia-presence` | Encyclopedia entity | Discovery | Bonus | 4 | Content |  | external scan | Whether an encyclopedia article or knowledge-base item names the site. |
| `mcp-registry-listed` | Listed in MCP registries | Discovery | Recommended | 1 | All |  | external scan | Whether the site's MCP server is listed in public registries. |
| `npm-sdk-package` | SDK package | Discovery | Recommended | 1 | Api |  | external scan | Whether an SDK package for the site exists in public package registries. |
| `agent-rules-repo` | Agent configs repository | Discovery | Emerging | 1 | All |  | external scan | Whether a public repository holds agent rules or configs for the site. |
| `agent-plugins-repo` | Agent plugins manifest | Discovery | Bonus | 1 | All |  | external scan | Whether a public repository holds an agent plugin manifest. |
| `chatgpt-app-listed` | Listed as an assistant app | Discovery | Bonus | 2 | All |  | external scan | Whether the site has an app in an assistant's app directory. |
| `sitemap-lastmod` | Sitemap freshness | Access | Bonus | 1 | Content |  | Sawit | Most sitemap entries carry a <lastmod>. |
| `content-no-js` | Content without JavaScript | Access | Required | 3 | Content |  | Sawit | The server's HTML, before scripts run, has the page's text (500+ characters) and an <h1>, with text at least 5% of the HTML. |
| `bot-detection` | Not blocked by bot checks | Access | Required | 2 | Content |  | Sawit | The page answers 2xx to the agents' user agents (ChatGPT-User, Claude-User, PerplexityBot, OAI-SearchBot), not a challenge page. |
| `agent-crawler-reachability` | Reachable to AI crawlers | Access | Recommended | 2 | Content |  | Sawit | Each AI agent both gets the page and is allowed by robots.txt. |
| `robots-agent-user-policy` | Agents acting for people | Access | Required | 2 | Content |  | Sawit | robots.txt doesn't block the agents that fetch for a person (ChatGPT-User, Claude-User, Perplexity-User). |
| `agent-discovery-file` | Agent discovery file | Access | Required | 2 | Content |  | Sawit | An agent skills index (/.well-known/agent-skills/index.json) or an AGENTS.md answers. |
| `agent-skills-index-v2` | Agent skills index format | Access | Emerging | 2 | Content |  | Sawit | The skills index declares its version and each skill a name, URL and digest. |
| `a2a-agent-card` | Agent card | Access | Bonus | 2 | All |  | Sawit | /.well-known/agent-card.json (or agent.json) describes the site's agent. |
| `pricing-md` | pricing.md | Access | Emerging | 2 | Content |  | Sawit | /pricing.md answers with the prices as Markdown. |
| `nlweb-schema-feeds` | Schema feeds | Access | Emerging | 1 | Content |  | Sawit | robots.txt names a schema feed (Schemamap:). |
| `mcp-well-known-discovery` | MCP well-known file | Access | Bonus | 2 | All |  | Sawit | /.well-known/mcp.json (or the server card) says where the MCP server is. |
| `agent-mode-view` | Agent view | Access | Emerging | 2 | Content |  | Sawit | The page with ?mode=agent answers a version for agents (Markdown, or pointers to llms.txt, the API, MCP). |
| `link-headers-discovery` | Link discovery headers | Access | Bonus | 1 | All |  | Sawit | The page's HTTP Link header advertises describedby, service-desc, service-doc, api-catalog or a Markdown alternate. |
| `markdown-url-fallback` | Markdown at .md | Access | Emerging | 2 | Content |  | Sawit | /index.md answers with the home page as Markdown. |
| `modular-llms-txt` | Section llms.txt | Access | Emerging | 1 | Content |  | Sawit | A section has its own llms.txt (/docs/llms.txt, /api/llms.txt, /blog/llms.txt). |
| `llms-txt-links-resolve` | llms.txt links load | Access | Recommended | 2 | Content |  | Sawit | The first links in llms.txt answer below 400. |
| `pricing-info` | Pricing for agents | Access | Required | 3 | Content |  | Sawit | JSON-LD Offer or PriceSpecification, or at least a pricing page linked from the home page. |
| `public-api-docs` | Docs linked | Access | Required | 3 | Content |  | Sawit | The home page links to public docs or an API reference. |
| `agent-instruction` | Guidance for agents | Access | Required | 3 | Content |  | Sawit | llms.txt or AGENTS.md tells agents when to use the site and how. |
| `json-ld-entity-linking` | Entity links (sameAs) | Access | Recommended | 2 | Content |  | Sawit | The JSON-LD entity links its profiles elsewhere with sameAs. |
| `org-schema-completeness` | Organization details | Access | Recommended | 2 | Content |  | Sawit | The Organization has a contactPoint and an address. |
| `schema-type-breadth` | Structured data breadth | Access | Recommended | 2 | Content |  | Sawit | JSON-LD goes past the basics: FAQPage, Service, Product, Offer, Article, HowTo, Event, AggregateRating… |
| `trust-anchors` | About, contact, privacy | Access | Required | 2 | Content |  | Sawit | The page links About, Contact and Privacy pages. |
| `markdown-link-alternate` | Markdown alternate link | Access | Emerging | 1 | Content |  | Sawit | <link rel="alternate" type="text/markdown"> (or a Link header) names a Markdown version that answers. |
| `markdown-frontmatter` | Markdown front matter | Access | Emerging | 1 | Content |  | Sawit | Markdown the site serves starts with front matter (title, description). |
| `redirect-hygiene` | Redirects | Access | Recommended | 1 | All |  | Sawit | At most two redirects, no meta refresh, no hop to another site. |
| `page-token-budget` | Page size for agents | Access | Recommended | 1 | Content |  | Sawit | The page's text fits an agent's context (under about 25,000 tokens). |
| `code-fence-validity` | Code fences | Access | Emerging | 1 | Content |  | Sawit | Code fences in the served Markdown are balanced. |
| `docs-auth-gate` | Content without sign-in | Access | Recommended | 2 | Content |  | Sawit | The page answers without a sign-in wall (no 401/403, no redirect to a login page). |
| `openapi-spec` | OpenAPI spec | Access | Required | 7 | Api |  | Sawit | An OpenAPI or Swagger document answers at a usual place (/openapi.json, /openapi.yaml, /swagger.json, /api/openapi.json) or the API catalog names one. |
| `developer-portal` | Developer portal | Access | Recommended | 6 | Api |  | Sawit | A developer or docs portal is linked and answers (/developers, /docs, docs.<domain>). |
| `api-catalog-rfc9727` | API catalog | Access | Bonus | 2 | Api |  | Sawit | /.well-known/api-catalog answers with a linkset of the site's APIs. |
| `markdown-negotiation` | Markdown on request | Access | Emerging | 1 | Content |  | Sawit | The page answers text/markdown when asked with Accept: text/markdown. |
| `markdown-negotiation-vary` | Markdown negotiation with Vary | Access | Bonus | 1 | Content |  | Sawit | The same address serves HTML and Markdown by Accept, and says Vary: Accept. |
| `agent-ua-markdown` | Markdown for agent user agents | Access | Emerging | 1 | Content |  | Sawit | An AI agent's user agent gets Markdown even when it asks for HTML. |
| `skills-sh-listed` | Skills listed publicly | Access | Emerging | 1 | All |  | external scan | Whether official skills for the site are listed in a public skills directory. |
| `skills-sh-quality` | Public skills quality | Access | Emerging | 2 | All |  | external scan | How good the site's public skills are. |
| `mcp-server` | MCP server | Usability | Required | 6 | All |  | Sawit | An MCP server named by the well-known files (or at /mcp) answers initialize over Streamable HTTP, or asks for OAuth. |
| `mcp-transport-modern` | MCP transport | Usability | Bonus | 1 | All |  | Sawit | The MCP server speaks Streamable HTTP (not only the old SSE transport). |
| `mcp-server-identity` | MCP server identity | Usability | Bonus | 1 | All |  | Sawit | initialize names the server and its version, with instructions. |
| `mcp-tool-listing` | MCP tool count | Usability | Bonus | 3 | All |  | Sawit | tools/list answers 1 to 30 tools. |
| `mcp-tool-descriptions` | MCP tool descriptions | Usability | Bonus | 3 | All |  | Sawit | Every tool has a description of 30 characters or more. |
| `mcp-param-schemas` | MCP parameter schemas | Usability | Bonus | 2 | All |  | Sawit | Tools declare an inputSchema with typed properties. |
| `mcp-tool-naming` | MCP tool names | Usability | Bonus | 2 | All |  | Sawit | Tool names follow one convention (snake_case or camelCase) and aren't generic (run, query, tool). |
| `mcp-tool-annotations` | MCP tool annotations | Usability | Bonus | 2 | All |  | Sawit | Tools carry annotations (readOnlyHint, destructiveHint…). |
| `mcp-error-handling` | MCP errors | Usability | Bonus | 2 | All |  | Sawit | An unknown method answers a JSON-RPC error with a code and message. |
| `mcp-resource-listing` | MCP resources | Usability | Recommended | 3 | All |  | Sawit | resources/list answers resources. |
| `mcp-server-card` | MCP server card | Usability | Bonus | 2 | All |  | Sawit | /.well-known/mcp/server-card.json describes the server with a name and URL. |
| `mcp-auth-mechanism` | MCP auth | Usability | Bonus | 2 | All |  | Sawit | The MCP server is public, or asks for OAuth with a WWW-Authenticate header. |
| `mcp-oauth-metadata` | MCP OAuth metadata | Usability | Bonus | 2 | All |  | Sawit | A protected MCP server publishes its protected-resource metadata. |
| `mcp-pkce-s256` | MCP PKCE | Usability | Bonus | 2 | All |  | Sawit | The MCP server's authorization server supports PKCE S256. |
| `public-api` | Public API | Usability | Required | 7 | Api |  | Sawit | An API answers: an OpenAPI document, or JSON from /api. |
| `oauth-support` | OAuth 2.0 | Usability | Required | 5 | Api |  | Sawit | /.well-known/oauth-authorization-server or /.well-known/openid-configuration answers. |
| `scoped-permissions` | Scoped permissions | Usability | Recommended | 5 | Api |  | Sawit | The OAuth metadata or the OpenAPI document declares scopes. |
| `oauth-protected-resource` | Protected resource metadata | Usability | Bonus | 2 | Api |  | Sawit | /.well-known/oauth-protected-resource answers with the resource and its authorization servers. |
| `auth-md-exists` | auth.md | Usability | Bonus | 2 | Api |  | Sawit | /auth.md answers with how agents sign in. |
| `auth-md-structure` | auth.md sections | Usability | Bonus | 2 | Api |  | Sawit | auth.md is laid out in sections (discover, pick a method, get a token, use it). |
| `agent-auth-www-authenticate` | Auth hint on 401 | Usability | Bonus | 1 | Api |  | Sawit | A 401 from the API names its resource metadata in WWW-Authenticate. |
| `agent-auth-discovery-metadata` | Auth discovery | Usability | Bonus | 3 | Api |  | Sawit | Both protected-resource and authorization-server metadata answer. |
| `web-bot-auth-directory` | Signed agent keys | Usability | Emerging | 2 | All |  | Sawit | /.well-known/http-message-signatures-directory answers (Web Bot Auth). |
| `webmcp` | Page tools for agents | Usability | Required | 5 | Content |  | Sawit | The page declares tools for agents in the browser (navigator.modelContext / registerTool, or forms with toolname). |
| `json-error-responses` | JSON errors | Usability | Required | 4 | Api |  | Sawit | A missing API path answers 4xx with a JSON body. |
| `rate-limit-headers` | Rate limit headers | Usability | Recommended | 2 | Api |  | Sawit | API answers carry RateLimit headers. |
| `agent-friendly-404` | Readable 404s | Usability | Recommended | 2 | All |  | Sawit | A missing page answers 404 (not 200), ideally with text or Markdown that says where to go. |
| `ax-document-structure` | Document structure | Usability | Bonus | 3 | Content |  | Sawit | The server's HTML has a <main>, landmarks, one <h1> and no heading jumps over one level. |
| `ax-native-controls` | Native controls | Usability | Bonus | 3 | Content |  | Sawit | Actions use <button>, <a>, <input>, not clickable <div>s. |
| `ax-accessible-names` | Named controls | Usability | Bonus | 2 | Content |  | Sawit | Buttons, inputs and links have accessible names. |
| `ax-form-labeling` | Form labels | Usability | Bonus | 2 | Content |  | Sawit | Form fields have labels. |
| `ax-tree-injection-safe` | No hidden instructions | Usability | Bonus | 2 | Content |  | Sawit | No hidden or off-screen text tells agents to ignore their instructions. |
| `nlweb-ask` | Ask endpoint | Usability | Emerging | 1 | Content |  | Sawit | /ask?query=… answers JSON results. |
| `cli-tool` | CLI tool | Usability | Recommended | 3 | Api |  | Sawit | llms.txt or the docs mention a command-line tool (npx, brew, pip, cargo install). |
| `api-schema-analysis` | API operations described | Usability | Recommended | 2 | Api |  | Sawit | OpenAPI operations have operationIds and summaries or descriptions. |
| `response-schema-coverage` | Typed responses | Usability | Recommended | 2 | Api |  | Sawit | OpenAPI operations define typed response schemas. |
| `api-error-model` | Typed errors | Usability | Recommended | 3 | Api |  | Sawit | OpenAPI defines an error schema that 4xx/5xx responses use. |
| `api-versioning-policy` | API versioning | Usability | Recommended | 3 | Api |  | Sawit | The API is versioned (path or header) and says how it deprecates (Sunset, deprecated). |
| `pagination-shape` | Pagination | Usability | Recommended | 2 | Api |  | Sawit | List operations paginate, preferably with cursors. |
| `function-calling-compat` | Function calling fit | Usability | Recommended | 2 | Api |  | Sawit | Operations have operationIds and typed request schemas, so they map to tool calls. |
| `idempotency-key-support` | Idempotency keys | Usability | Recommended | 3 | Api |  | Sawit | Write operations accept an Idempotency-Key header. |
| `batch-endpoints` | Batch operations | Usability | Bonus | 2 | Api |  | Sawit | The API has batch or bulk operations. |
| `async-job-pattern` | Async jobs | Usability | Recommended | 2 | Api |  | Sawit | Long operations answer 202 with a job to poll. |
| `graphql-endpoint` | GraphQL | Usability | Recommended | 2 | Api |  | Sawit | When /graphql answers, it allows introspection with descriptions. |
| `mcp-app-registry` | MCP Apps | Usability | Recommended | 4 | All |  | Sawit | The MCP server offers UI resources (ui://) for agent hosts. |
| `onboarding-friction` | Agent onboarding | Usability | Recommended | 2 | Api |  | external scan | Whether an agent can start without a human sign-up step. |
| `rest-sdk-packages` | SDKs in many languages | Usability | Recommended | 3 | Api |  | external scan | Whether SDK packages exist across language registries. |
| `sandbox-environment` | Sandbox | Usability | Bonus | 2 | Api |  | external scan | Whether a sandbox or test environment is offered. |
| `mcp-multi-surface-coverage` | Product and docs MCP | Usability | Bonus | 2 | All |  | external scan | Whether both the product and the docs have MCP servers. |
| `pap-discovery` | Personal agent protocol | Usability | Emerging | 2 | All |  | external scan | Whether the site publishes a personal agent protocol file. |
| `x402-support` | x402 payments | Payments | Bonus | 2 | All |  | Sawit | A paid resource answers 402 with x402 payment requirements, or /.well-known/x402 answers. |
| `mpp-support` | Machine payments | Payments | Bonus | 2 | All |  | Sawit | /.well-known/mpp answers, or a 402 asks for a Payment authorization. |
| `ucp-support` | Universal commerce | Payments | Bonus | 3 | All |  | Sawit | /.well-known/ucp answers with the store's commerce profile. |
| `acp-support` | Agentic commerce | Payments | Bonus | 3 | All |  | Sawit | /.well-known/acp.json answers with the merchant's checkout. |
| `ap2-support` | Agent payments protocol | Payments | Bonus | 3 | All |  | external scan | Whether the site supports the agent payments protocol. |
