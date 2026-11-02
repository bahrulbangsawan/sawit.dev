//! `data:` URLs and percent-encoding, decoded.

use std::borrow::Cow;

pub(super) fn percent_decode(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
            if let Some(v) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

pub fn decode_data_url(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mut parts = meta.split(';');
    let mime = parts.next().unwrap_or("").trim();
    let mime = if mime.is_empty() { "text/plain" } else { mime };
    let is_b64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    // With no `%` in it the payload is its own decoding, so it isn't copied.
    let raw: Cow<[u8]> = if data.contains('%') {
        Cow::Owned(percent_decode(data.as_bytes()))
    } else {
        Cow::Borrowed(data.as_bytes())
    };
    let bytes = if is_b64 {
        // ASCII with no whitespace in it, as base64 usually is, is decoded as it is.
        let text: Cow<str> = match std::str::from_utf8(&raw) {
            Ok(t) if t.bytes().all(|b| b.is_ascii() && !b.is_ascii_whitespace()) => {
                Cow::Borrowed(t)
            }
            _ => Cow::Owned(
                raw.iter()
                    .filter(|b| !b.is_ascii_whitespace())
                    .map(|&b| b as char)
                    .collect(),
            ),
        };
        crate::browser::base64_decode(&text)?
    } else {
        raw.into_owned()
    };
    Some((mime.to_ascii_lowercase(), bytes))
}
