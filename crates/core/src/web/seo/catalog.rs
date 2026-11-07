//! Every check the scan runs, in one table keyed by id: its layer, tier, points, which sites it
//! applies to, and how to fix it. The search and sharing checks and the agent-readiness checks
//! share entries where they test the same thing (`robots.txt`, the sitemap, `llms.txt`, JSON-LD),
//! so nothing runs twice. `docs/notes/2026-11-07-seo-agent-checks.md` lists the table.

use super::{Layer, Tier};

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
#[allow(dead_code)]
const fn outside(def: CheckDef) -> CheckDef {
    CheckDef {
        external: true,
        ..def
    }
}

use Applies::{All, Content};
use Layer::{Access, Discovery};
use Tier::{Bonus, Recommended, Required};

pub const CHECKS: &[CheckDef] = &[
    // Search and sharing.
    seo(
        "title",
        "Page title",
        Required,
        2,
        "The page's <title>: present, 10 to 60 characters.",
        "Give the page a unique <title> of 10 to 60 characters that says what it is.",
        "https://developers.google.com/search/docs/appearance/title-link",
    ),
    seo(
        "meta-description",
        "Meta description",
        Required,
        2,
        "<meta name=\"description\">: present, 50 to 160 characters.",
        "Add <meta name=\"description\" content=\"…\"> of 50 to 160 characters summing up the page.",
        "https://developers.google.com/search/docs/appearance/snippet",
    ),
    seo(
        "canonical",
        "Canonical address",
        Required,
        1,
        "<link rel=\"canonical\">: present, absolute, on this site.",
        "Add <link rel=\"canonical\" href=\"https://…\"> with the page's preferred absolute address.",
        "https://developers.google.com/search/docs/crawling-indexing/consolidate-duplicate-urls",
    ),
    shared(agent(
        "noindex-directives",
        "Not hidden from search",
        Discovery,
        Recommended,
        1,
        All,
        "No noindex in the robots, googlebot or bingbot meta tags, nor in an X-Robots-Tag header.",
        "Remove noindex from <meta name=\"robots\"> and the X-Robots-Tag header on pages that should be found.",
        "https://developers.google.com/search/docs/crawling-indexing/block-indexing",
    )),
    seo(
        "robots-txt",
        "robots.txt",
        Required,
        1,
        "/robots.txt answers, parses, and doesn't block the whole site for every crawler.",
        "Serve /robots.txt with User-agent groups and a Sitemap: line; don't Disallow: / for *.",
        "https://www.rfc-editor.org/rfc/rfc9309",
    ),
    seo(
        "hreflang",
        "Language alternates",
        Recommended,
        1,
        "<link rel=\"alternate\" hreflang>: valid codes, absolute addresses, the page itself and x-default among them.",
        "List every language version with hreflang, including the page itself and an x-default.",
        "https://developers.google.com/search/docs/specialty/international/localized-versions",
    ),
    seo(
        "headings",
        "Headings outline",
        Required,
        2,
        "Exactly one <h1>, and no heading level skipped.",
        "Use one <h1> for the page's subject and nest <h2>…<h6> without skipping levels.",
        "https://developer.mozilla.org/docs/Web/HTML/Element/Heading_Elements",
    ),
    seo(
        "favicon",
        "Favicon",
        Required,
        1,
        "An icon from <link rel=\"icon\"> or /favicon.ico loads as a picture.",
        "Add <link rel=\"icon\" href=\"/favicon.svg\"> (or a 32×32 PNG) and serve /favicon.ico.",
        "https://developers.google.com/search/docs/appearance/favicon-in-search",
    ),
    seo(
        "apple-touch-icon",
        "Touch icon",
        Recommended,
        1,
        "<link rel=\"apple-touch-icon\"> names a 180×180 icon that loads.",
        "Add <link rel=\"apple-touch-icon\" href=\"/apple-touch-icon.png\"> with a 180×180 PNG.",
        "https://developer.apple.com/library/archive/documentation/AppleApplications/Reference/SafariWebContent/ConfiguringWebApplications/ConfiguringWebApplications.html",
    ),
    seo(
        "web-manifest",
        "Web app manifest",
        Recommended,
        1,
        "<link rel=\"manifest\"> names a JSON manifest with a name and icons.",
        "Add <link rel=\"manifest\" href=\"/manifest.webmanifest\"> with name, icons (192 and 512 px) and theme_color.",
        "https://developer.mozilla.org/docs/Web/Manifest",
    ),
    seo(
        "open-graph",
        "Open Graph tags",
        Required,
        2,
        "og:title, og:description, og:image, og:url and og:type are set.",
        "Add the og:title, og:description, og:image, og:url and og:type <meta property> tags.",
        "https://ogp.me/",
    ),
    seo(
        "og-image",
        "Share image",
        Required,
        2,
        "The og:image loads as a picture, at least 1200×630, under 8 MB.",
        "Point og:image at an absolute address of a 1200×630 PNG or JPEG under 8 MB.",
        "https://ogp.me/#structured",
    ),
    seo(
        "twitter-card",
        "X card",
        Recommended,
        1,
        "<meta name=\"twitter:card\"> is set (summary_large_image for a big preview).",
        "Add <meta name=\"twitter:card\" content=\"summary_large_image\">; X falls back to Open Graph for the rest.",
        "https://developer.x.com/en/docs/x-for-websites/cards/overview/markup",
    ),
    shared(agent(
        "json-ld",
        "JSON-LD structured data",
        Access,
        Required,
        4,
        Content,
        "<script type=\"application/ld+json\"> blocks parse, name a @type, and describe the site's entity (Organization, Person, WebSite) with a name and description.",
        "Add a JSON-LD block describing the Organization or Person behind the site, with name, description, url, logo and sameAs.",
        "https://schema.org/docs/gs.html",
    )),
    shared(agent(
        "sitemap",
        "Sitemap",
        Access,
        Required,
        2,
        Content,
        "/sitemap.xml (or one robots.txt names) is a valid URL set or sitemap index with entries.",
        "Serve a valid XML sitemap at /sitemap.xml listing every indexable page, and name it in robots.txt.",
        "https://www.sitemaps.org/protocol.html",
    )),
    seo(
        "sitemap-md",
        "Markdown sitemap",
        Bonus,
        1,
        "/sitemap.md answers with Markdown links to the site's pages.",
        "Serve /sitemap.md: a Markdown list of the site's pages with a line on each, for agents.",
        "https://llmstxt.org/",
    ),
    shared(agent(
        "llms-txt-exists",
        "llms.txt",
        Access,
        Required,
        1,
        Content,
        "/llms.txt answers with text, not the site's HTML.",
        "Serve /llms.txt: an # H1 with the site's name, a > summary, and ## sections of Markdown links to the key pages.",
        "https://llmstxt.org/",
    )),
    shared(agent(
        "llms-txt-formatting",
        "llms.txt layout",
        Access,
        Recommended,
        2,
        Content,
        "llms.txt starts with an # H1, has a > summary, ## sections and Markdown links.",
        "Lay out llms.txt as the spec says: # Name, > one-line summary, then ## sections of [title](url): notes links.",
        "https://llmstxt.org/#format",
    )),
    seo(
        "llms-full-txt",
        "llms-full.txt",
        Recommended,
        1,
        "/llms-full.txt (or /full-llms.txt) answers with the site's content as Markdown.",
        "Serve /llms-full.txt with the full text of the key pages as Markdown, for agents that read it all at once.",
        "https://llmstxt.org/",
    ),
    seo(
        "image-alt",
        "Image alt text",
        Recommended,
        2,
        "Every <img> has an alt attribute (empty for decorative images).",
        "Add alt text to each image that says what it shows; use alt=\"\" for decorative ones.",
        "https://www.w3.org/WAI/tutorials/images/",
    ),
    seo(
        "broken-links",
        "Broken links",
        Recommended,
        2,
        "Up to 30 of the page's links answer below 400.",
        "Fix or remove the links that answer 404 or fail.",
        "https://developers.google.com/search/docs/crawling-indexing/links-crawlable",
    ),
    seo(
        "https",
        "HTTPS",
        Required,
        1,
        "The page is served over HTTPS (local addresses don't need it).",
        "Serve the site over HTTPS and redirect http:// to it.",
        "https://developers.google.com/search/docs/appearance/page-experience",
    ),
    seo(
        "viewport",
        "Mobile viewport",
        Recommended,
        1,
        "<meta name=\"viewport\" content=\"width=device-width…\"> is set.",
        "Add <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">.",
        "https://developer.mozilla.org/docs/Web/HTML/Viewport_meta_tag",
    ),
    shared(agent(
        "metadata-completeness",
        "Metadata completeness",
        Access,
        Required,
        2,
        Content,
        "A canonical address, <html lang>, og:image and og:type are all present.",
        "Add the missing ones: <link rel=\"canonical\">, <html lang=\"…\">, og:image and og:type.",
        "https://ogp.me/",
    )),
];

/// The check with `id`.
pub fn def(id: &str) -> Option<&'static CheckDef> {
    CHECKS.iter().find(|c| c.id == id)
}
