//! A dev server's routes, for the canvas's route frames (Add Routes from Dev Server,
//! `canvas_add_routes`): read from the files of its framework ([`from_files`]), which the
//! project's `package.json` names; and for an app whose files say nothing Sawit reads, or for
//! samples of dynamic routes, found by following the server's own links ([`crawl`]).
//!
//! A dynamic route (`/blog/[slug]`) is a page only with a value: it is added with the first page
//! the crawl finds that it matches (`/blog/hello`), else skipped, and the skipped are named.

mod crawl;
mod files;
mod pattern;

use std::path::Path;

use serde_json::Value;

pub use crawl::{
    CRAWL, Crawled, Limits, answers, crawl, crawl_with, host_port, links_in, loopback_addrs,
    same_origin_path,
};
pub use pattern::{Pattern, Segment, not_a_page};

/// A framework whose route files Sawit reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framework {
    Next,
    TanStack,
    SvelteKit,
    Remix,
    ReactRouter,
    Nuxt,
    Astro,
}

impl Framework {
    pub fn name(self) -> &'static str {
        match self {
            Framework::Next => "Next.js",
            Framework::TanStack => "TanStack Router",
            Framework::SvelteKit => "SvelteKit",
            Framework::Remix => "Remix",
            Framework::ReactRouter => "React Router",
            Framework::Nuxt => "Nuxt",
            Framework::Astro => "Astro",
        }
    }

    /// The framework `package.json`'s dependencies name, the first that has its own routing.
    pub fn of_package(package: &Value) -> Option<Framework> {
        let has = |name: &str| {
            ["dependencies", "devDependencies"]
                .iter()
                .any(|key| package[key].get(name).is_some())
        };
        let tanstack = [
            "@tanstack/react-start",
            "@tanstack/solid-start",
            "@tanstack/start",
            "@tanstack/react-router",
            "@tanstack/solid-router",
            "@tanstack/router-plugin",
        ];
        [
            (has("next"), Framework::Next),
            (has("@sveltejs/kit"), Framework::SvelteKit),
            (has("nuxt"), Framework::Nuxt),
            (has("astro"), Framework::Astro),
            (tanstack.iter().any(|n| has(n)), Framework::TanStack),
            (
                has("@remix-run/dev") || has("@remix-run/react"),
                Framework::Remix,
            ),
            (has("@react-router/dev"), Framework::ReactRouter),
        ]
        .into_iter()
        .find_map(|(found, framework)| found.then_some(framework))
    }
}

/// The routes an app's files make: its framework, as its `package.json` names it, and the
/// routes of that framework's folders in `folder`, the app's own (where its dev server runs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileRoutes {
    pub framework: Option<Framework>,
    pub patterns: Vec<Pattern>,
}

/// The routes the files of the app in `folder` make ([`FileRoutes`]).
pub fn from_files(folder: &Path) -> FileRoutes {
    let package = std::fs::read_to_string(folder.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let Some(framework) = package.as_ref().and_then(Framework::of_package) else {
        return FileRoutes::default();
    };
    let is_dir = |rel: &str| folder.join(rel).is_dir();
    let mut patterns = Vec::new();
    let mut add = |rel: &str, read: &dyn Fn(&Path, &str) -> Vec<Pattern>| {
        if is_dir(rel) {
            patterns.extend(read(&folder.join(rel), rel));
        }
    };
    let pages = |exts: &'static [&'static str], astro: bool| {
        move |root: &Path, label: &str| files::page_files(root, label, exts, astro)
    };
    match framework {
        Framework::Next => {
            for rel in ["app", "src/app"] {
                add(rel, &files::next_app);
            }
            for rel in ["pages", "src/pages"] {
                add(rel, &files::next_pages);
            }
        }
        Framework::TanStack => {
            let configured = tanstack_folder(folder);
            let folders = configured
                .iter()
                .map(String::as_str)
                .chain(["src/routes", "app/routes"]);
            if let Some(rel) = folders.into_iter().find(|rel| is_dir(rel)) {
                add(rel, &files::tanstack);
            }
        }
        Framework::SvelteKit => add("src/routes", &files::sveltekit),
        Framework::Remix => add("app/routes", &files::flat_routes),
        Framework::ReactRouter => {
            // `app/routes.ts` lists the routes in code unless it hands them to the files.
            let config = std::fs::read_to_string(folder.join("app/routes.ts")).unwrap_or_default();
            if config.is_empty() || config.contains("flatRoutes") {
                add("app/routes", &files::flat_routes);
            }
        }
        Framework::Nuxt => {
            let exts: &[&str] = &["vue", "tsx", "jsx"];
            for rel in ["app/pages", "pages"] {
                add(rel, &pages(exts, false));
            }
        }
        Framework::Astro => {
            let exts: &[&str] = &["astro", "md", "mdx", "html", "markdoc"];
            add("src/pages", &pages(exts, true));
        }
    }
    FileRoutes {
        framework: Some(framework),
        patterns,
    }
}

/// TanStack Router's routes folder as `tsr.config.json` names it, relative to `folder`.
fn tanstack_folder(folder: &Path) -> Option<String> {
    let text = std::fs::read_to_string(folder.join("tsr.config.json")).ok()?;
    let config: Value = serde_json::from_str(&text).ok()?;
    let rel = config["routesDirectory"].as_str()?;
    Some(
        rel.trim_start_matches("./")
            .trim_end_matches('/')
            .to_string(),
    )
}

/// A route to add: the path the server answers, and where it comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// `/`, `/changelog`, `/blog/hello`.
    pub path: String,
    /// The file that makes it, relative to the app's folder; `None` for a page the crawl found.
    pub file: Option<String>,
    /// For a sample of a dynamic route, that route as the canvas writes it: `/blog/[slug]`.
    pub sample_of: Option<String>,
}

/// A dynamic route left out for want of a sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// `/blog/[slug]`.
    pub route: String,
    pub file: String,
}

/// The routes to add of `patterns`, an app's file routes, with `links`, the pages its server links
/// to, as samples: each route with no dynamic part (or whose dynamic parts may be left out) as it
/// is, each dynamic one by the first link it matches, and those with none skipped. Files and APIs
/// (`/rss.xml`, `/api/…`) aren't pages. `/` comes first, then the rest by path.
pub fn resolve(patterns: &[Pattern], links: &[String]) -> (Vec<Route>, Vec<Skipped>) {
    let mut routes: Vec<Route> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    let add = |routes: &mut Vec<Route>, route: Route| {
        if !not_a_page(&route.path) && !routes.iter().any(|r| r.path == route.path) {
            routes.push(route);
        }
    };
    for pattern in patterns.iter().filter(|p| p.bare_path().is_some()) {
        let path = pattern.bare_path().unwrap_or_default();
        add(
            &mut routes,
            Route {
                path,
                file: Some(pattern.file.clone()),
                sample_of: None,
            },
        );
    }
    for pattern in patterns.iter().filter(|p| p.bare_path().is_none()) {
        let written = pattern.written();
        if !pattern.is_page() {
            continue;
        }
        let taken = |link: &&String| routes.iter().any(|r| &r.path == *link);
        match links
            .iter()
            .filter(|l| !taken(l))
            .find(|l| pattern.matches(l))
        {
            Some(link) => {
                let route = Route {
                    path: link.clone(),
                    file: Some(pattern.file.clone()),
                    sample_of: Some(written),
                };
                add(&mut routes, route);
            }
            None if !skipped.iter().any(|s| s.route == written) => {
                skipped.push(Skipped {
                    route: written,
                    file: pattern.file.clone(),
                });
            }
            None => {}
        }
    }
    sort(&mut routes);
    (routes, skipped)
}

/// `/` first, then by path.
fn sort(routes: &mut [Route]) {
    routes.sort_by(|a, b| {
        (a.path != "/", a.path.to_lowercase()).cmp(&(b.path != "/", b.path.to_lowercase()))
    });
}

/// What discovering a dev server's routes found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Found {
    pub framework: Option<Framework>,
    pub routes: Vec<Route>,
    pub skipped: Vec<Skipped>,
    /// The crawl, when one ran: for samples, or for the routes themselves.
    pub crawl: Option<Crawled>,
    /// Why the crawl couldn't run, when it couldn't.
    pub crawl_error: Option<String>,
}

/// The routes of the dev server at `base` (`http://localhost:3000`) running the app in `folder`:
/// its file routes ([`from_files`]), with samples for the dynamic ones from a crawl ([`crawl`]);
/// or, when its files make none Sawit reads, the pages the crawl fetches from `/`.
pub async fn discover(folder: &Path, base: &str, limits: &Limits) -> Found {
    let files = from_files(folder);
    let dynamic = files.patterns.iter().any(|p| p.bare_path().is_none());
    let mut found = Found {
        framework: files.framework,
        ..Found::default()
    };
    if !files.patterns.is_empty() && !dynamic {
        (found.routes, found.skipped) = resolve(&files.patterns, &[]);
        return found;
    }
    let (statics, _) = resolve(&files.patterns, &[]);
    let mut start: Vec<String> = vec!["/".to_string()];
    start.extend(statics.into_iter().map(|r| r.path).filter(|p| p != "/"));
    let crawled = match crawl(base, &start, limits).await {
        Ok(crawled) => crawled,
        Err(error) => {
            found.crawl_error = Some(error);
            Crawled::default()
        }
    };
    if files.patterns.is_empty() {
        found.routes = (crawled.pages.iter())
            .filter(|p| !not_a_page(p))
            .map(|path| Route {
                path: path.clone(),
                file: None,
                sample_of: None,
            })
            .collect();
        sort(&mut found.routes);
    } else {
        let mut links = crawled.pages.clone();
        links.extend(crawled.links.iter().cloned());
        (found.routes, found.skipped) = resolve(&files.patterns, &links);
    }
    found.crawl = Some(crawled);
    found
}

/// `path` on the server at `base`: `http://localhost:3000` and `/changelog` make
/// `http://localhost:3000/changelog`.
pub fn url_of(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if path == "/" {
        format!("{base}/")
    } else {
        format!("{base}/{}", path.trim_start_matches('/'))
    }
}
