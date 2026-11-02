//! Route discovery from each framework's files, on the fixture apps in
//! `crates/core/src/browser/routes/fixtures/`, and dynamic routes filled in from samples.

use std::path::PathBuf;

use crate::browser::routes::{
    Framework, Pattern, Segment, from_files, not_a_page, resolve, url_of,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/browser/routes/fixtures")
        .join(name)
}

/// The routes and skipped dynamic routes the fixture app `name` makes with no samples.
fn found(name: &str) -> (Option<Framework>, Vec<String>, Vec<String>) {
    let files = from_files(&fixture(name));
    let (routes, skipped) = resolve(&files.patterns, &[]);
    let paths = routes.into_iter().map(|r| r.path).collect();
    let mut skipped: Vec<String> = skipped.into_iter().map(|s| s.route).collect();
    skipped.sort();
    (files.framework, paths, skipped)
}

#[test]
fn next_reads_the_app_and_pages_routers() {
    let (framework, routes, skipped) = found("next");
    assert_eq!(framework, Some(Framework::Next));
    // Groups leave the path; private folders, slots, intercepting routes, route handlers,
    // `_app`, the error pages and the API make no page. An optional catch-all is its folder.
    assert_eq!(
        routes,
        ["/", "/about", "/docs", "/legacy", "/posts", "/pricing"]
    );
    assert_eq!(skipped, ["/blog/[slug]", "/posts/[id]", "/shop/[...all]"]);
}

#[test]
fn tanstack_reads_its_file_routes() {
    let (framework, routes, skipped) = found("tanstack");
    assert_eq!(framework, Some(Framework::TanStack));
    // `__root`, `-components`, tests, pathless layouts and groups leave the path; `route.tsx`
    // and `index` are their folder's; `.lazy` is the same route; `{-$lang}` may be left out.
    // `sitemap[.]xml` and `api/` aren't pages.
    assert_eq!(
        routes,
        [
            "/",
            "/about",
            "/changelog",
            "/docs",
            "/login",
            "/posts",
            "/settings",
            "/settings/profile"
        ]
    );
    assert_eq!(
        skipped,
        [
            "/files/[..._splat]",
            "/posts/[postId]",
            "/posts/[postId]/edit"
        ]
    );
}

/// Sawit's own site, the case the canvas is checked on: `/` and `/changelog`.
#[test]
fn sawits_own_site_has_two_routes() {
    let site = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../site");
    let files = from_files(&site);
    assert_eq!(files.framework, Some(Framework::TanStack));
    let (routes, skipped) = resolve(&files.patterns, &[]);
    let paths: Vec<&str> = routes.iter().map(|r| r.path.as_str()).collect();
    assert_eq!(paths, ["/", "/changelog"]);
    assert!(skipped.is_empty());
    assert_eq!(routes[1].file.as_deref(), Some("src/routes/changelog.tsx"));
}

#[test]
fn sveltekit_reads_its_page_folders() {
    let (framework, routes, skipped) = found("sveltekit");
    assert_eq!(framework, Some(Framework::SvelteKit));
    // `[[lang]]` may be left out; `+server.ts` alone is no page.
    assert_eq!(routes, ["/", "/about", "/dashboard", "/faq"]);
    assert_eq!(skipped, ["/blog/[slug]", "/items/[id]"]);
}

#[test]
fn remix_reads_its_flat_routes() {
    let (framework, routes, skipped) = found("remix");
    assert_eq!(framework, Some(Framework::Remix));
    // `_index` is `/`, `_auth.` a pathless layout, `concerts_.` not nested, `($lang)` optional, a
    // folder its `route.tsx`; its other files and `.server` modules are no routes.
    assert_eq!(
        routes,
        [
            "/",
            "/about",
            "/concerts/mine",
            "/concerts/trending",
            "/dashboard",
            "/login",
            "/privacy"
        ]
    );
    assert_eq!(skipped, ["/concerts/[city]", "/files/[...*]"]);
}

#[test]
fn react_router_reads_flat_routes_when_routes_ts_asks_for_them() {
    let (framework, routes, skipped) = found("react-router");
    assert_eq!(framework, Some(Framework::ReactRouter));
    assert_eq!(routes, ["/", "/home"]);
    assert_eq!(skipped, ["/team/[id]"]);
    // Routes listed in code are the crawl's to find.
    let dir = tempfile::tempdir().unwrap();
    let app = fixture("react-router");
    std::fs::copy(app.join("package.json"), dir.path().join("package.json")).unwrap();
    std::fs::create_dir_all(dir.path().join("app/routes")).unwrap();
    std::fs::write(dir.path().join("app/routes/home.tsx"), "").unwrap();
    let config = "export default [index(\"routes/home.tsx\")];\n";
    std::fs::write(dir.path().join("app/routes.ts"), config).unwrap();
    assert!(from_files(dir.path()).patterns.is_empty());
}

#[test]
fn nuxt_reads_its_pages() {
    let (framework, routes, skipped) = found("nuxt");
    assert_eq!(framework, Some(Framework::Nuxt));
    assert_eq!(routes, ["/", "/about", "/docs", "/settings", "/users"]);
    assert_eq!(skipped, ["/posts/[...slug]", "/users/[id]"]);
}

#[test]
fn astro_reads_its_pages_but_not_endpoints_or_private_files() {
    let (framework, routes, skipped) = found("astro");
    assert_eq!(framework, Some(Framework::Astro));
    assert_eq!(routes, ["/", "/about", "/blog", "/guide"]);
    assert_eq!(skipped, ["/blog/[slug]", "/docs/[...path]"]);
}

#[test]
fn an_app_without_a_framework_sawit_reads_has_no_file_routes() {
    let files = from_files(&fixture("plain"));
    assert_eq!(files.framework, None);
    assert!(files.patterns.is_empty());
    assert_eq!(from_files(&fixture("nowhere")), Default::default());
}

/// A dynamic route is added with the first page the server links to that it matches, and the
/// page it is a sample of is said; one with no sample stays skipped.
#[test]
fn dynamic_routes_take_samples_from_links() {
    let files = from_files(&fixture("tanstack"));
    let links: Vec<String> = [
        "/posts",
        "/posts/hello-world",
        "/posts/hello-world/edit",
        "/x/y",
    ]
    .map(String::from)
    .to_vec();
    let (routes, skipped) = resolve(&files.patterns, &links);
    let sample = |pattern: &str| {
        (routes.iter())
            .find(|r| r.sample_of.as_deref() == Some(pattern))
            .map(|r| r.path.clone())
    };
    assert_eq!(
        sample("/posts/[postId]").as_deref(),
        Some("/posts/hello-world")
    );
    assert_eq!(
        sample("/posts/[postId]/edit").as_deref(),
        Some("/posts/hello-world/edit")
    );
    let skipped: Vec<&str> = skipped.iter().map(|s| s.route.as_str()).collect();
    assert_eq!(skipped, ["/files/[..._splat]"]);
    // A link that is a static route already is no sample.
    assert!(
        !routes
            .iter()
            .any(|r| r.path == "/posts" && r.sample_of.is_some())
    );
}

#[test]
fn patterns_match_the_paths_of_their_pages() {
    let p = |segments: Vec<Segment>| Pattern {
        segments,
        file: String::new(),
    };
    let s = |n: &str| Segment::Static(n.into());
    let blog = p(vec![s("blog"), Segment::Param("slug".into())]);
    assert!(blog.matches("/blog/hello") && blog.matches("/Blog/hello/"));
    assert!(!blog.matches("/blog") && !blog.matches("/blog/a/b") && !blog.matches("/news/a"));
    let docs = p(vec![s("docs"), Segment::Rest("path".into())]);
    assert!(docs.matches("/docs/a") && docs.matches("/docs/a/b/c") && !docs.matches("/docs"));
    let opt = p(vec![Segment::Optional("lang".into()), s("faq")]);
    assert!(opt.matches("/faq") && opt.matches("/en/faq") && !opt.matches("/en/fr/faq"));
    assert_eq!(opt.bare_path().as_deref(), Some("/faq"));
    let any = p(vec![s("shop"), Segment::OptionalRest("all".into())]);
    assert!(any.matches("/shop") && any.matches("/shop/a/b"));
    assert_eq!(docs.bare_path(), None);
    assert_eq!(docs.written(), "/docs/[...path]");
    assert_eq!(p(vec![]).bare_path().as_deref(), Some("/"));
}

#[test]
fn files_and_apis_are_not_pages() {
    for path in [
        "/rss.xml",
        "/api",
        "/api/users",
        "/sitemap.xml",
        "/blog/feed.json",
    ] {
        assert!(not_a_page(path), "{path}");
    }
    for path in ["/", "/about", "/index.html", "/apis", "/.well-known"] {
        assert!(!not_a_page(path), "{path}");
    }
}

#[test]
fn a_route_is_a_url_on_its_server() {
    assert_eq!(
        url_of("http://localhost:3000", "/"),
        "http://localhost:3000/"
    );
    assert_eq!(
        url_of("http://localhost:3000/", "/changelog"),
        "http://localhost:3000/changelog"
    );
}
