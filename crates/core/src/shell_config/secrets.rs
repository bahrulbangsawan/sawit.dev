//! Spotting secrets in startup files before they are saved: variables named like a key or a
//! password, and values shaped like a known kind of token.

/// Lines that set what looks like a secret, as `line number, variable`: a
/// name like `OPENAI_API_KEY`, `GITHUB_PAT` or `DB_PASSWORD`, or a value
/// shaped like a known token (`sk-…`, `ghp_…`, `xoxb-…`, `AKIA…`), in the
/// ways shells set variables ([`assignment`]). Values that only point at
/// another variable or a command (`$(op read …)`) don't count.
pub fn find_secrets(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (ix, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = assignment(line) else {
            continue;
        };
        let valid = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
        let reference = value.is_empty() || value.starts_with('$') || value.starts_with('`');
        if valid && !reference && (secret_name(name) || secret_value(value)) {
            out.push((ix + 1, name.to_string()));
        }
    }
    out
}

/// The variable a line sets and its value, as shells write it: `NAME=value`
/// alone or after `export`, `typeset -x`, `declare -x`, `local` or
/// `readonly`; fish's `set -gx NAME value`; csh's `setenv NAME value`;
/// `launchctl setenv NAME value`; PowerShell's `$env:NAME = "value"` and
/// `[Environment]::SetEnvironmentVariable("NAME", "value", …)`.
fn assignment(line: &str) -> Option<(&str, String)> {
    fn words(rest: &str) -> Option<(&str, String)> {
        let mut words = rest.split_whitespace().skip_while(|w| w.starts_with('-'));
        let name = words.next()?;
        Some((name, words.collect::<Vec<_>>().join(" ")))
    }
    for prefix in ["set ", "setenv ", "launchctl setenv "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return words(rest);
        }
    }
    let lower = line.to_ascii_lowercase();
    if lower.starts_with("$env:") {
        let (name, value) = line[5..].split_once('=')?;
        return Some((name.trim(), value.trim().to_string()));
    }
    for prefix in [
        "[environment]::setenvironmentvariable(",
        "[system.environment]::setenvironmentvariable(",
    ] {
        if lower.starts_with(prefix) {
            let mut parts = line[prefix.len()..].splitn(3, ',');
            let name = parts.next()?.trim().trim_matches(|c| c == '"' || c == '\'');
            let value = parts.next()?.trim().trim_end_matches(')').trim();
            return Some((name, value.to_string()));
        }
    }
    let mut rest = line;
    for keyword in ["export ", "typeset ", "declare ", "local ", "readonly "] {
        if let Some(after) = rest.strip_prefix(keyword) {
            rest = after.trim_start();
            while rest.starts_with('-') {
                rest = rest.split_once(char::is_whitespace)?.1.trim_start();
            }
            break;
        }
    }
    let (name, value) = rest.split_once('=')?;
    Some((name.trim(), value.trim().to_string()))
}

/// A variable name that holds a secret.
fn secret_name(name: &str) -> bool {
    const PARTS: [&str; 9] = [
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "PASSPHRASE",
        "API_KEY",
        "PRIVATE_KEY",
        "ACCESS_KEY",
        "CREDENTIAL",
    ];
    let upper = name.to_ascii_uppercase();
    PARTS.iter().any(|p| upper.contains(p)) || upper.ends_with("_KEY") || upper.ends_with("_PAT")
}

/// A value shaped like a known kind of token or key.
fn secret_value(value: &str) -> bool {
    const PREFIXES: [&str; 17] = [
        "sk-",
        "ghp_",
        "gho_",
        "ghu_",
        "ghs_",
        "ghr_",
        "github_pat_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "glpat-",
        "hf_",
        "npm_",
        "AIza",
        "AKIA",
        "ASIA",
    ];
    PREFIXES
        .iter()
        .any(|p| value.starts_with(p) && value.len() >= p.len() + 8 && !value.contains(' '))
}
