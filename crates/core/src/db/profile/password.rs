//! Saved passwords: the Keychain, and `.pgpass` files read as libpq reads them.

use super::ConnProfile;
use crate::{Result, keychain};

/// The password for `profile` in a `.pgpass` file (`passfile`, else
/// `$PGPASSFILE`, else `~/.pgpass`), matched like libpq: `*` matches
/// anything, `localhost` also matches socket connections, `\:` and `\\`
/// are escapes, and a file readable by others is ignored.
pub fn pgpass_password(profile: &ConnProfile) -> Option<String> {
    let path = profile
        .passfile
        .clone()
        .or_else(|| std::env::var("PGPASSFILE").ok())
        .or_else(|| std::env::var("HOME").ok().map(|h| format!("{h}/.pgpass")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&path).ok()?.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    pgpass_lookup(&std::fs::read_to_string(path).ok()?, profile)
}

pub(in crate::db) fn pgpass_lookup(text: &str, profile: &ConnProfile) -> Option<String> {
    let host = if profile.is_socket() {
        "localhost"
    } else {
        profile.host.as_str()
    };
    let port = profile.port.to_string();
    let wanted = [
        host,
        port.as_str(),
        profile.database.as_str(),
        profile.user.as_str(),
    ];
    text.lines().find_map(|line| {
        if line.trim_start().starts_with('#') {
            return None;
        }
        let mut fields = vec![String::new()];
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => fields.last_mut()?.extend(chars.next()),
                ':' if fields.len() < 5 => fields.push(String::new()),
                c => fields.last_mut()?.push(c),
            }
        }
        if fields.len() != 5 {
            return None;
        }
        let matches = fields[..4]
            .iter()
            .zip(wanted)
            .all(|(field, want)| field == "*" || field == want);
        matches.then(|| fields[4].clone())
    })
}

/// The saved password: the Keychain first, then `.pgpass`.
pub fn resolve_password(profile: &ConnProfile) -> Option<String> {
    load_password(profile)
        .ok()
        .flatten()
        .or_else(|| pgpass_password(profile))
}

const KEYCHAIN_SERVICE: &str = "sawit";
/// The service passwords were saved under before the rename to Sawit (PG
/// Desk); they move to `KEYCHAIN_SERVICE` when first read.
const LEGACY_KEYCHAIN_SERVICE: &str = "pgdesk";
pub fn store_password(profile: &ConnProfile, password: &str) -> Result<()> {
    keychain::set(KEYCHAIN_SERVICE, &profile.keychain_account(), password)
}

pub fn load_password(profile: &ConnProfile) -> Result<Option<String>> {
    let account = profile.keychain_account();
    if let Some(password) = keychain::get(KEYCHAIN_SERVICE, &account)? {
        return Ok(Some(password));
    }
    let Some(password) = keychain::get(LEGACY_KEYCHAIN_SERVICE, &account)? else {
        return Ok(None);
    };
    if store_password(profile, &password).is_ok() {
        let _ = keychain::delete(LEGACY_KEYCHAIN_SERVICE, &account);
    }
    Ok(Some(password))
}

pub fn delete_password(profile: &ConnProfile) -> Result<()> {
    let account = profile.keychain_account();
    // Not migrated yet; it must not come back on the next read.
    let _ = keychain::delete(LEGACY_KEYCHAIN_SERVICE, &account);
    keychain::delete(KEYCHAIN_SERVICE, &account)
}
