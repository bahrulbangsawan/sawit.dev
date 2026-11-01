//! What a page may have Sawit fetch for it, and keep.

use super::*;

/// What curl got is kept only from where the page may reach: redirects can end on this
/// Mac, which only the URL asked for showed otherwise.
#[test]
fn answers_are_kept_by_where_they_came_from() {
    let web = "https://shop.example/";
    let ok = |href: &str, final_url: &str, ip: &str, page: &str| {
        answer_allowed(href, final_url, ip, page).is_ok()
    };
    assert!(ok(
        "https://cdn.example/a.png",
        "https://cdn.example/a.png",
        "93.184.215.14",
        web
    ));
    assert!(!ok(
        "https://cdn.example/a.png",
        "http://127.0.0.1:5173/a.png",
        "127.0.0.1",
        web
    ));
    assert!(!ok(
        "https://cdn.example/a.png",
        "https://cdn.example/a.png",
        "192.168.1.2",
        web
    ));
    assert!(!ok(
        "https://cdn.example/a.png",
        "https://cdn.example/a.png",
        "",
        web
    ));
    let local = "http://localhost:5173/";
    assert!(ok(
        "http://localhost:5173/a.png",
        "http://localhost:5173/a.png",
        "127.0.0.1",
        local
    ));
    assert!(ok(
        "http://localhost:5173/a.png",
        "https://cdn.example/a.png",
        "93.184.215.14",
        local
    ));
    assert!(!ok(
        "http://localhost:5173/a.png",
        "http://192.168.1.2/a.png",
        "192.168.1.2",
        local
    ));
}

/// A page on this Mac or its network keeps every download: its own (a `.test` page's,
/// from 127.0.0.1), what another local name serves it (Vite's `127.0.0.1:5173`), and the
/// web's. The files go to the disk, never back to the page.
#[test]
fn a_local_page_keeps_every_download() {
    let ok = |href: &str, final_url: &str, ip: &str| {
        download_allowed(href, final_url, ip, false, true).is_ok()
    };
    let own = "http://myapp.test/logo.png";
    assert!(ok(own, own, "127.0.0.1"));
    let vite = "http://127.0.0.1:5173/resources/hero.png";
    assert!(ok(vite, vite, "127.0.0.1"));
    assert!(ok(
        "http://localhost:5173/a.png",
        "http://192.168.1.2/a.png",
        "192.168.1.2"
    ));
    let cdn = "https://cdn.example/a.png";
    assert!(ok(cdn, cdn, "93.184.215.14"));
}

/// A page on the web keeps no download from this Mac or its network: not from a host
/// named so, where it asked or where redirects ended, and not from a local address (a
/// public name resolving here). Through a proxy the address is the proxy's, so the names
/// decide.
#[test]
fn a_web_page_keeps_no_download_from_this_mac() {
    let ok = |href: &str, final_url: &str, ip: &str, proxied: bool| {
        download_allowed(href, final_url, ip, proxied, false).is_ok()
    };
    let cdn = "https://cdn.example/a.png";
    assert!(ok(cdn, cdn, "93.184.215.14", false));
    let dev = "http://127.0.0.1:5173/a.png";
    assert!(!ok(dev, dev, "127.0.0.1", false));
    let printer = "http://printer.local/a.png";
    assert!(!ok(printer, printer, "192.168.1.2", false));
    assert!(!ok(cdn, dev, "127.0.0.1", false));
    assert!(!ok(cdn, cdn, "192.168.1.2", false));
    let dev_name = "http://myapp.test/a.png";
    assert!(!ok(dev_name, dev_name, "127.0.0.1", false));
    assert!(!ok(cdn, cdn, "100.64.3.4", false));
    assert!(!ok(cdn, cdn, "", false));
    assert!(!ok(
        "https://user@cdn.example/a.png",
        cdn,
        "93.184.215.14",
        false
    ));
    // Through a proxy on the office network.
    assert!(ok(cdn, cdn, "10.0.0.5", true));
    assert!(!ok(dev, dev, "10.0.0.5", true));
    assert!(!ok(cdn, "http://192.168.1.2/a.png", "10.0.0.5", true));
}

fn css(text: &str, final_url: &str, remote_ip: &str) -> Fetched {
    Fetched {
        text: text.into(),
        content_type: "text/css; charset=utf-8".into(),
        final_url: final_url.into(),
        remote_ip: remote_ip.into(),
    }
}

#[test]
fn only_css_goes_back_to_the_page() {
    let page = "https://a.dev/";
    let href = "https://cdn.b.dev/site.css";
    assert_eq!(
        sheet_for_page(href, css("a{}", href, "93.184.216.34"), page),
        Ok("a{}".into())
    );
    let mut js = css("export default 1", href, "93.184.216.34");
    js.content_type = "application/javascript".into();
    assert!(sheet_for_page(href, js, page).is_err());
    let mut none = css("a{}", href, "93.184.216.34");
    none.content_type.clear();
    assert!(sheet_for_page(href, none, page).is_err());
}

#[test]
fn local_sheets_only_for_their_own_host() {
    let dev = "http://localhost:5173/src/App.css";
    // A public page can't read the dev server (or the LAN) through Sawit.
    assert!(sheet_for_page(dev, css("a{}", dev, "127.0.0.1"), "https://evil.dev/").is_err());
    let lan = "http://192.168.1.5/admin.css";
    assert!(sheet_for_page(lan, css("a{}", lan, "192.168.1.5"), "https://evil.dev/").is_err());
    // Nor by a redirect there, or a public name for a local address.
    let public = "https://cdn.b.dev/x.css";
    assert!(sheet_for_page(public, css("a{}", dev, "127.0.0.1"), "https://a.dev/").is_err());
    assert!(sheet_for_page(public, css("a{}", public, "10.0.0.2"), "https://a.dev/").is_err());
    // The dev server's own page keeps its sheets.
    assert_eq!(
        sheet_for_page(dev, css("a{}", dev, "::1"), "http://localhost:5173/"),
        Ok("a{}".into())
    );
    assert!(may_fetch_for(dev, "http://localhost:3000/"));
    assert!(!may_fetch_for(dev, "http://127.0.0.1:5173/"));
    assert!(!may_fetch_for("not a url", "https://a.dev/"));
}

#[test]
fn hrefs_curl_reads_another_way_are_refused() {
    let page = "https://evil.dev/";
    // curl reads `evil.dev\` as userinfo and connects to 127.0.0.1.
    let sneaky = "http://evil.dev\\@127.0.0.1:18777/x.css";
    assert_eq!(url_host(sneaky), None);
    assert!(!may_fetch_for(sneaky, page));
    assert!(sheet_for_page(sneaky, css("a{}", sneaky, "127.0.0.1"), page).is_err());
    for href in [
        "https://user@cdn.b.dev/x.css",
        "https://user:pw@cdn.b.dev/x.css",
        "https://@cdn.b.dev/x.css",
        "https://evil.dev@127.0.0.1/x.css",
        "https://cdn.b.dev\\x.css",
        "https://cdn.b.dev /x.css",
        "https://cdn.b.dev\t/x.css",
        "https://cdn.b.dev\u{0}/x.css",
        "https://cdn.b.dev:44x/x.css",
        "https://cdn%40b.dev/x.css",
    ] {
        assert!(!may_fetch_for(href, page), "{href}");
    }
    // A final URL with userinfo is dropped too.
    let href = "https://cdn.b.dev/x.css";
    let back = css("a{}", "https://u@cdn.b.dev/x.css", "93.184.216.34");
    assert!(sheet_for_page(href, back, page).is_err());
}

#[test]
fn a_public_page_checks_the_address_even_on_its_own_host() {
    let page = "https://evil.dev/";
    let own = "https://evil.dev/x.css";
    assert!(sheet_for_page(own, css("a{}", own, "127.0.0.1"), page).is_err());
    assert!(sheet_for_page(own, css("a{}", own, "100.100.1.2"), page).is_err());
    assert!(sheet_for_page(own, css("a{}", own, ""), page).is_err());
    assert!(sheet_for_page(own, css("a{}", own, "nonsense"), page).is_err());
    assert_eq!(
        sheet_for_page(own, css("a{}", own, "93.184.216.34"), page),
        Ok("a{}".into())
    );
    // Cross-host: no reported address, or a shared (CGNAT, Tailscale) one, is dropped.
    let cdn = "https://cdn.b.dev/x.css";
    assert!(sheet_for_page(cdn, css("a{}", cdn, ""), page).is_err());
    assert!(sheet_for_page(cdn, css("a{}", cdn, "100.64.3.4"), page).is_err());
    // A local page's own sheets keep the waiver.
    let dev = "http://localhost:5173/src/App.css";
    assert_eq!(
        sheet_for_page(dev, css("a{}", dev, "127.0.0.1"), "http://localhost:5173/"),
        Ok("a{}".into())
    );
    assert!(sheet_for_page(dev, css("a{}", dev, ""), "http://localhost:5173/").is_err());
}
