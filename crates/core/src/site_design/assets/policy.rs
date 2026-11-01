//! What a page may have Sawit fetch for it, and keep: a sheet that goes back to the page, a
//! download that only reaches the disk.

use std::net::IpAddr;

use super::curl::Fetched;
use super::hosts::{is_local_host, is_local_ip, url_host};

/// Whether `content_type` is a style sheet's (`text/css`, any parameters).
pub fn is_css(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("text/css")
}

/// Whether Sawit may fetch `url` for the page at `page_url`: a host on this Mac or its
/// network only when it is the page's own.
pub fn may_fetch_for(url: &str, page_url: &str) -> bool {
    let Some(host) = url_host(url) else {
        return false;
    };
    !is_local_host(&host) || url_host(page_url).is_some_and(|p| p == host)
}

/// Whether a sheet curl got for `href` may go back to the page at `page_url` (Sawit's own
/// URL for the tab, not the page's word): asked for, and ended (redirects followed), at hosts
/// the page may reach ([`may_fetch_for`]), and from a public address (`remote_ip`) — unless
/// the page is itself on this Mac or its network and the answer from its own host. An
/// answer whose address curl didn't report isn't kept. Otherwise why not. (Downloads, which
/// only reach the disk, follow [`download_allowed`].)
pub fn answer_allowed(
    href: &str,
    final_url: &str,
    remote_ip: &str,
    page_url: &str,
) -> Result<(), String> {
    if !may_fetch_for(href, page_url) || !may_fetch_for(final_url, page_url) {
        return Err("it is on this Mac or its network".into());
    }
    let page_host = url_host(page_url);
    let local_page = page_host.as_deref().is_some_and(is_local_host);
    let own_host = local_page && url_host(final_url).is_some_and(|h| page_host == Some(h));
    let ip = remote_ip.trim();
    if ip.parse::<std::net::IpAddr>().is_err() {
        return Err(format!("curl didn't say where its host is (“{ip}”)"));
    }
    if !own_host && is_local_host(ip) {
        return Err(format!("its host is at {ip}"));
    }
    Ok(())
}

/// Whether a page on the web may have Sawit download `url`: a host Sawit can name, and not
/// one on this Mac or its network.
pub(super) fn may_download(url: &str) -> bool {
    url_host(url).is_some_and(|h| !is_local_host(&h))
}

/// Whether a file curl downloaded for an export may be kept. The file only reaches the
/// disk, never the page, so a page on this Mac or its network (`local_page`, from
/// [`local_page`]) keeps whatever it listed: a dev site's assets are on this Mac, often on
/// another local name than the page's (Vite's `127.0.0.1:5173` for a `.test` page). A page on
/// the web keeps nothing from this Mac or its network: hosts named so are refused where curl
/// asked and where redirects ended (`final_url`), and the address curl got the file from
/// (`remote_ip`) must be public, which a name resolving to this Mac fails. Through a proxy
/// (`proxied`, curl's `%{proxy_used}`) that address is the proxy's, so the names decide.
/// Otherwise why not.
///
/// [`local_page`]: super::local_page
pub fn download_allowed(
    href: &str,
    final_url: &str,
    remote_ip: &str,
    proxied: bool,
    local_page: bool,
) -> Result<(), String> {
    if local_page {
        return Ok(());
    }
    if !may_download(href) || !may_download(final_url) {
        return Err("it is on this Mac or its network".into());
    }
    if proxied {
        return Ok(());
    }
    let ip = remote_ip.trim();
    match ip.parse::<IpAddr>() {
        Err(_) => Err(format!("curl didn't say where its host is (“{ip}”)")),
        Ok(addr) if is_local_ip(addr) => Err(format!("its host is at {ip}")),
        Ok(_) => Ok(()),
    }
}

/// The text of a sheet fetched from `href`, when it may go back to the page at `page_url`:
/// served as CSS, and [`answer_allowed`]. Otherwise why not.
pub fn sheet_for_page(href: &str, fetched: Fetched, page_url: &str) -> Result<String, String> {
    if !is_css(&fetched.content_type) {
        return Err(format!(
            "it was served as “{}”, not text/css",
            fetched.content_type
        ));
    }
    answer_allowed(href, &fetched.final_url, &fetched.remote_ip, page_url)?;
    Ok(fetched.text)
}
