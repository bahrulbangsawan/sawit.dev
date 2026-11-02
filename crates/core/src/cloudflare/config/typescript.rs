//! Reading a `cloudflare.config.ts` without running it: its tokens, comments dropped, and the
//! parts written as literals. `defineConfig({ worker: { name: "app", env: { DB: bindings.d1({
//! name: "app-db", id: "…" }) }, triggers: [triggers.scheduled({ schedule: "* * * * *" })] } })`
//! gives the name `app`, a D1 binding `DB` and Cron Triggers; a name worked out at run time
//! (`isStaging ? … : …`) gives none.

use super::{Binding, BindingKind, Config};

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    /// A string literal; `None` for a template with `${…}` in it, which isn't known until it
    /// runs.
    Str(Option<String>),
    Punct(char),
}

pub(super) fn read(text: &str) -> Config {
    let tokens = tokens(text);
    Config {
        name: worker_object(&tokens).and_then(|open| top_level_string(&tokens, open, &["name"])),
        bindings: bindings(&tokens),
        crons: tokens.windows(3).any(|w| {
            w[0] == Token::Ident("triggers".into())
                && w[1] == Token::Punct('.')
                && w[2] == Token::Ident("scheduled".into())
        }),
    }
}

fn tokens(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut ix = 0;
    while ix < chars.len() {
        let c = chars[ix];
        if c.is_whitespace() {
            ix += 1;
        } else if c == '/' && chars.get(ix + 1) == Some(&'/') {
            while ix < chars.len() && chars[ix] != '\n' {
                ix += 1;
            }
        } else if c == '/' && chars.get(ix + 1) == Some(&'*') {
            ix += 2;
            while ix < chars.len() && !(chars[ix] == '*' && chars.get(ix + 1) == Some(&'/')) {
                ix += 1;
            }
            ix += 2;
        } else if c == '"' || c == '\'' || c == '`' {
            let mut value = String::new();
            let mut known = true;
            ix += 1;
            while ix < chars.len() && chars[ix] != c {
                match chars[ix] {
                    '\\' => {
                        if let Some(&n) = chars.get(ix + 1) {
                            value.push(n);
                        }
                        ix += 2;
                        continue;
                    }
                    '$' if c == '`' && chars.get(ix + 1) == Some(&'{') => known = false,
                    other => value.push(other),
                }
                ix += 1;
            }
            ix += 1;
            out.push(Token::Str(known.then_some(value)));
        } else if c.is_alphanumeric() || c == '_' || c == '$' {
            let start = ix;
            while ix < chars.len()
                && (chars[ix].is_alphanumeric() || chars[ix] == '_' || chars[ix] == '$')
            {
                ix += 1;
            }
            out.push(Token::Ident(chars[start..ix].iter().collect()));
        } else {
            out.push(Token::Punct(c));
            ix += 1;
        }
    }
    out
}

fn ident(token: Option<&Token>, name: &str) -> bool {
    matches!(token, Some(Token::Ident(i)) if i == name)
}

fn punct(token: Option<&Token>, c: char) -> bool {
    token == Some(&Token::Punct(c))
}

/// Where the Worker's object opens: `worker: {` or `defineWorker({`.
fn worker_object(tokens: &[Token]) -> Option<usize> {
    (0..tokens.len()).find_map(|ix| {
        let (a, b, c) = (tokens.get(ix), tokens.get(ix + 1), tokens.get(ix + 2));
        let opens = (ident(a, "worker") && punct(b, ':') && punct(c, '{'))
            || (ident(a, "defineWorker") && punct(b, '(') && punct(c, '{'));
        opens.then_some(ix + 2)
    })
}

/// The literal of the first of `keys` set at the top level of the object opening at `open`.
fn top_level_string(tokens: &[Token], open: usize, keys: &[&str]) -> Option<String> {
    let mut depth = 0usize;
    let mut found: Vec<(usize, String)> = Vec::new();
    for ix in open..tokens.len() {
        match &tokens[ix] {
            Token::Punct('{' | '(' | '[') => depth += 1,
            Token::Punct('}' | ')' | ']') => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Token::Ident(key) | Token::Str(Some(key)) if depth == 1 => {
                let rank = keys.iter().position(|k| k == key);
                if let (Some(rank), true, Some(Token::Str(Some(value)))) =
                    (rank, punct(tokens.get(ix + 1), ':'), tokens.get(ix + 2))
                    && (ix == open + 1 || punct(tokens.get(ix - 1), ','))
                {
                    found.push((rank, value.clone()));
                }
            }
            _ => {}
        }
    }
    found.sort_by_key(|(rank, _)| *rank);
    found.into_iter().next().map(|(_, v)| v)
}

/// Every `KEY: bindings.kind(…)`, skipping plain values and secrets (`text`, `json`,
/// `secret`), which have no data to look at.
fn bindings(tokens: &[Token]) -> Vec<Binding> {
    let mut out = Vec::new();
    for ix in 0..tokens.len() {
        if !(ident(tokens.get(ix), "bindings") && punct(tokens.get(ix + 1), '.')) {
            continue;
        }
        let Some(Token::Ident(method)) = tokens.get(ix + 2) else {
            continue;
        };
        // `bindings.queue<Job>(…)`: the type arguments come before the call.
        let mut call = ix + 3;
        if punct(tokens.get(call), '<') {
            let mut depth = 0usize;
            while let Some(token) = tokens.get(call) {
                match token {
                    Token::Punct('<') => depth += 1,
                    Token::Punct('>') => depth -= 1,
                    _ => {}
                }
                call += 1;
                if depth == 0 {
                    break;
                }
            }
        }
        if !punct(tokens.get(call), '(') || matches!(method.as_str(), "text" | "json" | "secret") {
            continue;
        }
        let key = match (ix.checked_sub(2).map(|k| &tokens[k]), ix.checked_sub(1)) {
            (Some(Token::Ident(k) | Token::Str(Some(k))), Some(colon))
                if punct(tokens.get(colon), ':') =>
            {
                k.clone()
            }
            _ => continue,
        };
        let args = punct(tokens.get(call + 1), '{').then_some(call + 1);
        let read = |keys: &[&str]| args.and_then(|open| top_level_string(tokens, open, keys));
        out.push(Binding {
            kind: BindingKind::from_tag(method),
            binding: key,
            name: read(&[
                "name",
                "databaseName",
                "bucket",
                "bucketName",
                "exportName",
                "className",
                "destinationAddress",
            ]),
            id: read(&["id", "databaseId", "namespaceId"]),
            // cloudflare.config.ts documents no preview id.
            preview_id: None,
        });
    }
    out
}
