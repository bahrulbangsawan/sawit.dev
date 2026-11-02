//! What the address field loads for typed text.

use crate::browser::address::SEARCH;
use crate::browser::{normalize_address, search_url};

#[test]
fn addresses() {
    for (typed, url) in [
        ("localhost:5173", "http://localhost:5173"),
        ("127.0.0.1:8080/x", "http://127.0.0.1:8080/x"),
        ("app.test", "http://app.test"),
        ("example.com/a?b=1", "https://example.com/a?b=1"),
        ("http://x.dev", "http://x.dev"),
        ("HTTPS://X.dev", "HTTPS://X.dev"),
        ("file:///tmp/a.html", "file:///tmp/a.html"),
        ("about:blank", "about:blank"),
        ("", "about:blank"),
        ("myhost:3000", "https://myhost:3000"),
    ] {
        assert_eq!(normalize_address(typed), url, "{typed}");
    }
    assert_eq!(
        normalize_address("rust gpui"),
        "https://duckduckgo.com/?q=rust+gpui"
    );
    assert_eq!(
        normalize_address("c++"),
        "https://duckduckgo.com/?q=c%2B%2B"
    );
}

/// Text that isn't a URL WebKit can read is searched, never loaded: the browser asks
/// `NSURL` for what it loads, and `NSURL` has none for these (wry unwraps it, so loading
/// one aborted Sawit). A second `:port`, whitespace or a bad port in the host, and a bare
/// IPv6 address are typos.
#[test]
fn hosts_that_arent_urls_are_searched() {
    for typed in [
        "localhost:3000:3000",
        "127.0.0.1:3000:3000",
        "http://localhost 3000",
        "http:// localhost:3000",
        "https://my site.com",
        "https://localhost:3000:3000",
        "HTTPS://localhost:3000:3000/x",
        "http://127.0.0.1:3000:3000",
        "https://foo:exa%7Cmple.com",
        "https://::1",
        "https://user@host:1:2",
        "https://",
        "example.com:80a",
        "a.b:c/x",
    ] {
        let url = normalize_address(typed);
        assert!(url.starts_with(SEARCH), "{typed} → {url}");
        assert_eq!(url, search_url(typed), "{typed}");
    }
    // Spaces after the host are the path's: WebKit encodes them.
    assert_eq!(
        normalize_address("https://example.com/a b"),
        "https://example.com/a b"
    );
    assert_eq!(normalize_address("[::1]:3000/x"), "http://[::1]:3000/x");
    assert_eq!(
        normalize_address("https://[::1]:3000"),
        "https://[::1]:3000"
    );
    assert_eq!(
        normalize_address("user@example.com:8443/x"),
        "https://user@example.com:8443/x"
    );
    // Mail and phone links go to their apps, as the page's links do.
    assert_eq!(normalize_address("mailto:a@b.c"), "mailto:a@b.c");
    assert_eq!(normalize_address("tel:+15551234"), "tel:+15551234");
}
