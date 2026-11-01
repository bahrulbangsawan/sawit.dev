//! Which hosts and pages are on this Mac or its network.

use std::net::IpAddr;

use super::*;

/// A dev site on a name that resolves to this Mac (Herd and Valet's `.test`, DDEV's
/// `*.ddev.site`, `lvh.me`, a hosts-file name) is on this Mac, though its name doesn't say
/// so; a page with any public address, or none, is on the web.
#[test]
fn a_dev_site_on_a_name_that_resolves_here_is_local() {
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    assert!(page_is_local("http://myapp.test/", &[ip("127.0.0.1")]));
    assert!(page_is_local(
        "https://shop.ddev.site/",
        &[ip("127.0.0.1"), ip("::1")]
    ));
    assert!(page_is_local("http://lvh.me:3000/", &[ip("127.0.0.1")]));
    assert!(page_is_local("http://nas.lan:8080/", &[ip("192.168.1.10")]));
    // Named so: no lookup needed.
    assert!(page_is_local("http://localhost:5173/", &[]));
    assert!(page_is_local("http://127.0.0.1:5173/", &[]));
    assert!(page_is_local("http://[::1]:5173/", &[]));
    assert!(page_is_local("http://printer.local/", &[]));
    // On the web.
    assert!(!page_is_local(
        "https://shop.example/",
        &[ip("93.184.215.14")]
    ));
    assert!(!page_is_local(
        "https://evil.example/",
        &[ip("127.0.0.1"), ip("93.184.215.14")]
    ));
    assert!(!page_is_local("https://shop.example/", &[]));
    // Not a web page.
    assert!(!page_is_local(
        "file:///Users/a/index.html",
        &[ip("127.0.0.1")]
    ));
    assert!(!page_is_local("about:blank", &[ip("127.0.0.1")]));
    assert!(is_local_ip(ip("::ffff:10.0.0.1")) && !is_local_ip(ip("2606:4700::1")));
}

#[tokio::test]
async fn local_pages_by_name_need_no_lookup() {
    assert!(local_page("http://localhost:5173/").await);
    assert!(local_page("http://127.0.0.1/").await);
    assert!(local_page("http://app.localhost./").await);
    assert!(!local_page("file:///Users/a/index.html").await);
    assert!(!local_page("about:blank").await);
}

/// A host with letters curl may read another way (no IDN support in macOS's curl) isn't
/// one Sawit names: `is_local_host` couldn't tell.
#[test]
fn hosts_are_ascii() {
    assert_eq!(url_host("https://ex\u{e4}mple.com/a.css"), None);
    assert_eq!(url_host("https://\u{0441}dn.example/a.css"), None);
    assert_eq!(
        url_host("https://xn--exmple-cua.com/a.css").as_deref(),
        Some("xn--exmple-cua.com")
    );
}

#[test]
fn local_hosts() {
    for h in [
        "127.0.0.1",
        "127.1",
        "0x7f000001",
        "2130706433",
        "0177.0.0.1",
        "0.0.0.0",
        "10.1.2.3",
        "172.16.0.1",
        "172.31.255.255",
        "192.168.0.10",
        "169.254.169.254",
        "::1",
        "[::1]",
        "::",
        "fc00::1",
        "fd12:3456::1",
        "fe80::1%en0",
        "::ffff:127.0.0.1",
        "localhost",
        "LocalHost.",
        "app.localhost",
        "printer.local",
        "100.64.0.1",
        "100.127.255.254",
    ] {
        assert!(is_local_host(h), "{h}");
    }
    for h in [
        "93.184.216.34",
        "172.32.0.1",
        "100.63.255.255",
        "100.128.0.1",
        "8.8.8.8",
        "2606:4700::1",
        "example.com",
        "localhost.example.com",
        "deadbeef",
        "",
    ] {
        assert!(!is_local_host(h), "{h}");
    }
    assert_eq!(url_host("http://[::1]:8080/a").as_deref(), Some("::1"));
    assert_eq!(url_host("http://user@[::1]:8080/a"), None);
    assert_eq!(
        url_host("http://%6cocalhost./x").as_deref(),
        Some("localhost")
    );
    assert_eq!(url_host("https://A.dev:443?q").as_deref(), Some("a.dev"));
}
