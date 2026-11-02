//! The secrets the network log never keeps: the values of fields named like secrets, in a
//! request's URL and in the bodies of requests and responses.

/// What a secret's value is in the network log instead ([`NetworkEntry::redacted`]).
///
/// [`NetworkEntry::redacted`]: super::NetworkEntry::redacted
pub const REDACTED: &str = "[redacted]";

/// Whether a field named `name` holds a secret: a password, a token, a key, a cookie, a session
/// or a card's number. Names are compared by their letters and digits in lower case, so
/// `new_password`, `apiKey`, `X-Auth-Token`, `user[password]` and `client_secret` are secrets,
/// and `max_tokens`, `token_type` and `author` aren't.
pub(super) fn secret_name(name: &str) -> bool {
    /// Names of secrets on their own, which are other words inside longer names.
    const WHOLE: &[&str] = &[
        "pass", "pw", "pin", "mfa", "cvc", "cvv", "cvv2", "csc", "auth", "jwt", "bearer",
    ];
    /// How the names of secrets end.
    const ENDS: &[&str] = &[
        "token",
        "session",
        "sessionid",
        "sessionkey",
        "sessid",
        "sid",
        "pwd",
        "otp",
        "otpcode",
        "mfacode",
        "pincode",
        "passcode",
    ];
    /// Words in the names of secrets.
    const PARTS: &[&str] = &[
        "password",
        "passwd",
        "passphrase",
        "secret",
        "apikey",
        "privatekey",
        "authorization",
        "cookie",
        "credential",
        "signature",
        "creditcard",
        "cardnumber",
    ];
    let name: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    WHOLE.contains(&name.as_str())
        || ENDS.iter().any(|end| name.ends_with(end))
        || PARTS.iter().any(|part| name.contains(part))
}

/// `url` with its secrets [`REDACTED`]: the password of `user:password@`, and the values of the
/// secret fields of its query and of its fragment (`#access_token=…`).
pub(crate) fn redact_url(url: &str) -> String {
    let (rest, fragment) = match url.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (url, None),
    };
    let (base, query) = match rest.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (rest, None),
    };
    let mut out = match base.split_once("://") {
        Some((scheme, after)) => {
            let host_end = after.find('/').unwrap_or(after.len());
            match after[..host_end].rsplit_once('@') {
                Some((userinfo, host)) if userinfo.contains(':') => {
                    let user = userinfo.split_once(':').map_or(userinfo, |(user, _)| user);
                    format!("{scheme}://{user}:{REDACTED}@{host}{}", &after[host_end..])
                }
                _ => base.to_string(),
            }
        }
        None => base.to_string(),
    };
    if let Some(query) = query {
        out.push('?');
        out.push_str(&redact_form(query));
    }
    if let Some(fragment) = fragment {
        out.push('#');
        out.push_str(&redact_form(fragment));
    }
    out
}

/// A request's or a response's body with its secrets [`REDACTED`]: as JSON when it starts as JSON
/// does, as a form when it looks like one ([`looks_like_form`]), else as it is. The note the page
/// agent puts after a body it cut short (`… (5000 chars)`) stays.
pub(super) fn redact_body(body: &str) -> String {
    let (head, note) = cut_note(body);
    let redacted = if head.trim_start().starts_with(['{', '[']) {
        redact_json(head)
    } else if looks_like_form(head) {
        redact_form(head)
    } else {
        head.to_string()
    };
    redacted + note
}

/// `body` split before the note the page agent's `clip` puts after a body it cut: `… (5000
/// chars)`. Without one, all of it and no note.
pub(super) fn cut_note(body: &str) -> (&str, &str) {
    const START: &str = "… (";
    if let Some(head) = body.strip_suffix(" chars)")
        && let Some(at) = head.rfind(START)
        && head.len() > at + START.len()
        && head[at + START.len()..].bytes().all(|b| b.is_ascii_digit())
    {
        return body.split_at(at);
    }
    (body, "")
}

/// Whether `text` reads as a form: `name=value` pairs joined by `&`, each name written as forms
/// write them. Text, HTML and SQL have other characters before their first `=`.
fn looks_like_form(text: &str) -> bool {
    text.contains('=')
        && text.split('&').all(|pair| {
            let name = pair.split_once('=').map_or(pair, |(name, _)| name);
            name.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.[]%+*~:".contains(&b))
        })
}

/// `a=1&password=…`, a form or a URL's query, with the value of each secret field [`REDACTED`].
fn redact_form(form: &str) -> String {
    form.split('&')
        .map(|pair| match pair.split_once('=') {
            Some((name, _)) if secret_name(&percent_decode(name)) => format!("{name}={REDACTED}"),
            _ => pair.to_string(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// A form's name as it was before it was written into the form: `%5F` is `_`, `+` a space.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .filter(|hex| bytes[at] == b'%' && hex.iter().all(u8::is_ascii_hexdigit));
        match hex.and_then(|hex| u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()) {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `json`, also cut short, with every value under a secret field's name ([`secret_name`]) as
/// `"[redacted]"`: a string or a number, and each one inside an object or an array there. The
/// rest is copied as it is, spaces and all.
fn redact_json(json: &str) -> String {
    /// An object or an array the text is in, and whether everything in it is a secret's.
    struct Open {
        object: bool,
        secret: bool,
    }
    let secret_value = format!("\"{REDACTED}\"");
    let bytes = json.as_bytes();
    let mut out = String::with_capacity(json.len());
    let mut open: Vec<Open> = Vec::new();
    // The names read so far, as written, and whether each is a secret's: the objects of a list
    // repeat theirs, so each name is looked at once (up to `NAMES` of them).
    let mut names: Vec<(&str, bool)> = Vec::new();
    // In an object: whether the next string is a name, and whether the value after the last
    // name is a secret.
    let (mut name_next, mut value_secret) = (false, false);
    let mut at = 0;
    while at < bytes.len() {
        let secret = match open.last() {
            Some(o) if o.object => value_secret,
            Some(o) => o.secret,
            None => false,
        };
        match bytes[at] {
            c @ (b'{' | b'[') => {
                open.push(Open {
                    object: c == b'{',
                    secret,
                });
                name_next = c == b'{';
                out.push(c as char);
                at += 1;
            }
            c @ (b'}' | b']') => {
                open.pop();
                name_next = false;
                out.push(c as char);
                at += 1;
            }
            b',' => {
                name_next = open.last().is_some_and(|o| o.object);
                out.push(',');
                at += 1;
            }
            b'"' => {
                let end = string_end(bytes, at);
                let text = &json[at..end];
                if name_next {
                    let inherited = open.last().is_some_and(|o| o.secret);
                    value_secret = inherited
                        || match names.iter().find(|(name, _)| *name == text) {
                            Some(&(_, secret)) => secret,
                            None => {
                                let secret = secret_name(text);
                                if names.len() < NAMES {
                                    names.push((text, secret));
                                }
                                secret
                            }
                        };
                    name_next = false;
                    out.push_str(text);
                } else if secret {
                    out.push_str(&secret_value);
                } else {
                    out.push_str(text);
                }
                at = end;
            }
            b'-' | b'0'..=b'9' => {
                let len = bytes[at..]
                    .iter()
                    .take_while(|b| b"+-.eE0123456789".contains(b))
                    .count();
                out.push_str(if secret {
                    secret_value.as_str()
                } else {
                    &json[at..at + len]
                });
                at += len;
            }
            // Spaces, `:`, `true`, `false` and `null`: all of it up to what an arm above reads,
            // at once (those are ASCII, so the cut falls between characters).
            _ => {
                let len = bytes[at..]
                    .iter()
                    .position(|b| {
                        matches!(
                            b,
                            b'{' | b'[' | b'}' | b']' | b',' | b'"' | b'-' | b'0'..=b'9'
                        )
                    })
                    .unwrap_or(bytes.len() - at);
                out.push_str(&json[at..at + len]);
                at += len;
            }
        }
    }
    out
}

/// How many names [`redact_json`] remembers the answer of [`secret_name`] for.
const NAMES: usize = 64;

/// Where the JSON string whose `"` is at `start` ends: after its closing `"`, or at the end of a
/// text cut short inside it.
fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut at = start + 1;
    while at < bytes.len() {
        match bytes[at] {
            b'\\' => at += 2,
            b'"' => return at + 1,
            _ => at += 1,
        }
    }
    bytes.len()
}
