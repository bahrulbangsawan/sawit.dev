//! Where a URL's host is: on this Mac or its network, or on the web.

use std::net::IpAddr;
use std::time::Duration;

use super::data_url::percent_decode;

/// The URL's host: lowercased, decoded, without port, brackets or trailing dot. `None` for
/// a URL curl might read another way: one with userinfo, a `\`, whitespace or a control
/// character anywhere, a host with anything but name or address characters, or a bad port.
pub fn url_host(url: &str) -> Option<String> {
    if url
        .chars()
        .any(|c| c == '\\' || c.is_ascii_whitespace() || c.is_control())
    {
        return None;
    }
    let (_, rest) = url.split_once("://")?;
    // As curl splits it: the authority ends at the path, query or fragment.
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return None;
    }
    let (host, port) = match authority.strip_prefix('[') {
        Some(v6) => v6.split_once(']')?,
        None => match authority.find(':') {
            Some(at) => authority.split_at(at),
            None => (authority, ""),
        },
    };
    if !port.is_empty()
        && !port
            .strip_prefix(':')
            .is_some_and(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    let host = String::from_utf8_lossy(&percent_decode(host.as_bytes())).to_ascii_lowercase();
    let host = host.trim_end_matches('.');
    let v6 = authority.starts_with('[');
    // ASCII only: macOS's curl has no IDN support, so a name with other letters is one
    // `is_local_host` can't vouch for.
    let fits = |c: char| {
        c.is_ascii_alphanumeric()
            || c == '.'
            || c == '-'
            || c == '_'
            || (v6 && (c == ':' || c == '%'))
    };
    (!host.is_empty() && host.chars().all(fits)).then(|| host.to_string())
}

/// An IPv4 address as `inet_aton` (and so curl) reads it: `127.1`, `0x7f000001`, `0177.0.0.1`.
fn loose_v4(host: &str) -> Option<std::net::Ipv4Addr> {
    let num = |p: &str| -> Option<u64> {
        if let Some(hex) = p.strip_prefix("0x") {
            u64::from_str_radix(hex, 16).ok()
        } else if p.len() > 1 && p.starts_with('0') {
            u64::from_str_radix(&p[1..], 8).ok()
        } else {
            p.parse().ok()
        }
    };
    let parts: Vec<u64> = host.split('.').map(num).collect::<Option<_>>()?;
    let (last, head) = parts.split_last()?;
    if head.len() > 3 || head.iter().any(|&n| n > 255) {
        return None;
    }
    let rest = 4 - head.len() as u32;
    if *last >= 1u64 << (8 * rest) {
        return None;
    }
    let v = head.iter().fold(0u64, |v, &n| v << 8 | n) << (8 * rest) | last;
    Some(std::net::Ipv4Addr::from(v as u32))
}

fn local_v4(ip: std::net::Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || o[0] == 0
        // Shared address space (CGNAT, Tailscale): 100.64.0.0/10.
        || (o[0] == 100 && o[1] & 0xc0 == 64)
}

/// Whether `ip` is this Mac or its network: loopback, private, link-local, shared
/// (100.64/10) and unique-local addresses.
pub fn is_local_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => local_v4(v4),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            v6.is_loopback()
                || v6.is_unspecified()
                || first & 0xfe00 == 0xfc00
                || first & 0xffc0 == 0xfe80
                || v6.to_ipv4_mapped().is_some_and(local_v4)
        }
    }
}

/// Whether `host` (a name or an address) is this Mac or its network: an address
/// [`is_local_ip`] counts, `localhost` and `.local` names.
pub fn is_local_host(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Some(ip) = loose_v4(host) {
        return local_v4(ip);
    }
    // A scoped address (`fe80::1%en0`) is link-local whatever its zone.
    if let Ok(ip) = host
        .split('%')
        .next()
        .unwrap_or("")
        .parse::<std::net::Ipv6Addr>()
    {
        return is_local_ip(IpAddr::V6(ip));
    }
    host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
}

/// Whether the page at `page_url` (the tab's URL, by Sawit's word) is on this Mac or its
/// network: an http(s) page whose host is named so ([`is_local_host`]), or whose every
/// address is (`addrs`, where its host resolves; none when it didn't).
pub fn page_is_local(page_url: &str, addrs: &[IpAddr]) -> bool {
    if !is_http_url(page_url) {
        return false;
    }
    let Some(host) = url_host(page_url) else {
        return false;
    };
    is_local_host(&host) || (!addrs.is_empty() && addrs.iter().all(|ip| is_local_ip(*ip)))
}

/// How long a page's host may take to resolve; after that it counts as on the web.
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(3);

/// [`page_is_local`], with the page's host looked up as the system does (the hosts file,
/// `/etc/resolver`, DNS): a dev site on `myapp.test` (Herd, Valet), `*.ddev.site` or `lvh.me`
/// is on this Mac though its name doesn't say so.
pub async fn local_page(page_url: &str) -> bool {
    if page_is_local(page_url, &[]) {
        return true;
    }
    let Some(host) = url_host(page_url).filter(|_| is_http_url(page_url)) else {
        return false;
    };
    let lookup = tokio::net::lookup_host((host.as_str(), 0));
    let addrs: Vec<IpAddr> = match tokio::time::timeout(RESOLVE_TIMEOUT, lookup).await {
        Ok(Ok(found)) => found.map(|a| a.ip()).collect(),
        _ => Vec::new(),
    };
    page_is_local(page_url, &addrs)
}

fn is_http_url(url: &str) -> bool {
    let l = url.trim().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}
