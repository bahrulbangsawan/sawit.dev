//! What the address field loads for typed text: a URL, or a search.

/// What the address field searches with when the text isn't a URL.
pub(super) const SEARCH: &str = "https://duckduckgo.com/?q=";

/// What the address field loads for typed text: URLs as they are, hosts
/// with a scheme added (`http://` for local ones), anything else searched.
/// A web URL whose host isn't one (whitespace, a second `:port`) is searched
/// too: WebKit can't load it.
pub fn normalize_address(input: &str) -> String {
    let text = input.trim();
    if text.is_empty() {
        return "about:blank".into();
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = ["http://", "https://"]
        .iter()
        .find_map(|s| lower.strip_prefix(s))
    {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        return match split_host(authority) {
            Some(_) => text.to_string(),
            None => search_url(text),
        };
    }
    if ["file://", "about:", "data:", "mailto:", "tel:"]
        .iter()
        .any(|s| lower.starts_with(s))
    {
        return text.to_string();
    }
    if text.contains(char::is_whitespace) {
        return search_url(text);
    }
    let authority = lower.split(['/', '?', '#']).next().unwrap_or_default();
    let Some((name, port)) = split_host(authority) else {
        return search_url(text);
    };
    let local = name == "localhost"
        || name.ends_with(".localhost")
        || name.ends_with(".local")
        || name.ends_with(".test")
        || name == "[::1]"
        || is_ipv4(name);
    if local {
        return format!("http://{text}");
    }
    if name.contains('.') || port {
        return format!("https://{text}");
    }
    search_url(text)
}

/// A URL authority's host and whether it names a port: `None` when it isn't a host
/// (empty, whitespace, a port that isn't a number, a second `:port`, a bare IPv6 address).
fn split_host(authority: &str) -> Option<(&str, bool)> {
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if host.is_empty() || host.contains(char::is_whitespace) {
        return None;
    }
    // `[::1]` and `[::1]:3000`: the brackets hold the colons.
    let (name, port) = match host.strip_prefix('[') {
        Some(v6) => {
            let (inside, after) = v6.split_once(']')?;
            if inside.is_empty() || after.contains(['[', ']']) {
                return None;
            }
            (&host[..inside.len() + 2], after.strip_prefix(':'))
        }
        None => match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        },
    };
    if name.is_empty() || (!name.starts_with('[') && name.contains([':', '[', ']'])) {
        return None;
    }
    match port {
        Some(p) if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) => None,
        Some(_) => Some((name, true)),
        None if host.len() > name.len() => None,
        None => Some((name, false)),
    }
}

/// The search the address field runs for `text`.
pub fn search_url(text: &str) -> String {
    format!("{SEARCH}{}", encode_query(text.trim()))
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok())
}

/// `application/x-www-form-urlencoded` for a search query.
fn encode_query(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
