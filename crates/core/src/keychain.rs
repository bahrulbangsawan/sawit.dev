//! Generic passwords in the login Keychain, kept through `/usr/bin/security`
//! so no build of Sawit has to ask for them.
//!
//! macOS ties a Keychain item to the program that saved it, down to its code
//! hash when the program is ad-hoc signed, as Sawit is. Every rebuild or
//! update was a stranger to its own items and asked for the login password
//! again ("sawit wants to use your confidential information"), even with
//! "Always Allow". The `security` tool's identity never changes, so items it
//! saves it can always read back without a prompt. Secrets go to it on stdin,
//! never on the command line where `ps` would show them.
//!
//! Items saved in-process by older builds are read the old way (which may
//! ask one last time) and saved again through `security`; [`MARKER`], the
//! item's comment, tells the two apart without touching the secret.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use crate::{CoreError, Result};

const SECURITY: &str = "/usr/bin/security";
/// The comment on items saved through `security`.
const MARKER: &str = "Saved by Sawit";
/// `security`'s exit status when no item matches.
const NOT_FOUND: i32 = 44;

fn security(args: &[&str], stdin: Option<&str>) -> Result<Output> {
    let mut child = Command::new(SECURITY)
        .args(args)
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())?;
    }
    Ok(child.wait_with_output()?)
}

fn error(out: &Output) -> CoreError {
    let msg = String::from_utf8_lossy(&out.stderr);
    let msg = msg.lines().next().unwrap_or("").trim_start_matches("security: ");
    CoreError::Config(format!("Keychain: {msg}"))
}

/// The item's attributes as `security` prints them, or `None` if there is
/// none. Never reads the secret, so never asks.
fn attributes(service: &str, account: &str) -> Result<Option<String>> {
    let out = security(&["find-generic-password", "-s", service, "-a", account], None)?;
    match out.status.code() {
        Some(0) => Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned())),
        Some(NOT_FOUND) => Ok(None),
        _ => Err(error(&out)),
    }
}

pub fn get(service: &str, account: &str) -> Result<Option<String>> {
    let Some(attrs) = attributes(service, account)? else {
        return Ok(None);
    };
    if !attrs.contains(&format!("\"icmt\"<blob>=\"{MARKER}\"")) {
        return get_legacy(service, account);
    }
    let out = security(&["find-generic-password", "-s", service, "-a", account, "-g"], None)?;
    if !out.status.success() {
        return Err(error(&out));
    }
    // `-g` prints `password: "text"`, or `password: 0x<HEX>  "..."` when the
    // secret isn't plain ASCII, on stderr.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let value = stderr
        .lines()
        .find_map(|l| l.strip_prefix("password:"))
        .map(str::trim)
        .ok_or_else(|| CoreError::Config("Keychain: no password in the item".into()))?;
    if value.is_empty() {
        return Ok(Some(String::new()));
    }
    if let Some(hex) = value.strip_prefix("0x") {
        let hex = hex.split_whitespace().next().unwrap_or("");
        return Ok(Some(String::from_utf8_lossy(&decode_hex(hex)?).into_owned()));
    }
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .map(|v| Some(v.to_string()))
        .ok_or_else(|| CoreError::Config("Keychain: unreadable password".into()))
}

/// An item an older build saved in-process: read it as before, then save it
/// again through `security` so this is the last time it can ask.
fn get_legacy(service: &str, account: &str) -> Result<Option<String>> {
    match security_framework::passwords::get_generic_password(service, account) {
        Ok(bytes) => {
            let secret = String::from_utf8_lossy(&bytes).into_owned();
            // Best effort: if it fails, the next read simply asks again.
            let _ = set(service, account, &secret);
            Ok(Some(secret))
        }
        Err(e) if e.code() == -25300 => Ok(None),
        Err(e) => Err(CoreError::Config(format!("Keychain: {e}"))),
    }
}

pub fn set(service: &str, account: &str, secret: &str) -> Result<()> {
    // Replaced rather than updated: updating an item another build saved
    // would ask for the password.
    delete(service, account)?;
    // `-X` (hex) carries any bytes but needs at least one.
    let data = match secret {
        "" => "-w \"\"".to_string(),
        _ => format!("-X {}", encode_hex(secret.as_bytes())),
    };
    let line = format!(
        "add-generic-password -s {} -a {} -j {} {data}\n",
        quote(service),
        quote(account),
        quote(MARKER),
    );
    let out = security(&["-i"], Some(&line))?;
    if out.status.success() { Ok(()) } else { Err(error(&out)) }
}

/// Whether an item is saved, without reading (or asking for) its secret.
pub fn exists(service: &str, account: &str) -> Result<bool> {
    Ok(attributes(service, account)?.is_some())
}

pub fn delete(service: &str, account: &str) -> Result<()> {
    let out = security(&["delete-generic-password", "-s", service, "-a", account], None)?;
    match out.status.code() {
        Some(0 | NOT_FOUND) => Ok(()),
        _ => Err(error(&out)),
    }
}

/// A word for `security -i`, which splits on spaces and honours `"` and `\`.
fn quote(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|b| u8::from_str_radix(b, 16).ok())
                .ok_or_else(|| CoreError::Config("Keychain: unreadable password".into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let bytes = "p\"ä s\\x\n".as_bytes();
        assert_eq!(decode_hex(&encode_hex(bytes)).unwrap(), bytes);
        assert_eq!(decode_hex("7022C3A4").unwrap(), "p\"ä".as_bytes());
        assert!(decode_hex("7").is_err());
    }

    #[test]
    fn quote_escapes() {
        assert_eq!(quote(r#"a "b" \c"#), r#""a \"b\" \\c""#);
    }

    #[test]
    #[ignore = "writes to the login Keychain"]
    fn round_trip_without_prompting() {
        let (service, account) = ("sawit.keychain-test", "probe");
        for secret in ["plain", "", "a\"b c", "pässwörd \\ with\nnewline"] {
            set(service, account, secret).unwrap();
            assert!(exists(service, account).unwrap());
            assert_eq!(get(service, account).unwrap().as_deref(), Some(secret));
        }
        delete(service, account).unwrap();
        assert!(!exists(service, account).unwrap());
        assert_eq!(get(service, account).unwrap(), None);
    }
}
