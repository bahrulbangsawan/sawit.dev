//! The open request the CLI sends the running app over its Unix socket, and the app's replies.

use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use super::{OpenRequest, Target};

const MAGIC: &[u8] = b"sawit-open/1";

/// The request as NUL-separated fields: magic, flags, then path, line and
/// column per target. The CLI writes it and closes its sending half.
pub fn encode(request: &OpenRequest) -> Vec<u8> {
    let mut flags = String::new();
    for (on, c) in [
        (request.new_window, 'n'),
        (request.reuse, 'r'),
        (request.wait, 'w'),
    ] {
        if on {
            flags.push(c);
        }
    }
    let mut fields: Vec<Vec<u8>> = vec![MAGIC.to_vec(), flags.into_bytes()];
    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default().into_bytes();
    for t in &request.targets {
        fields.push(t.path.as_os_str().as_bytes().to_vec());
        fields.push(num(t.line));
        fields.push(num(t.column));
    }
    fields.join(&0u8)
}

pub fn decode(bytes: &[u8]) -> Option<OpenRequest> {
    let mut fields = bytes.split(|&b| b == 0);
    if fields.next()? != MAGIC {
        return None;
    }
    let flags = fields.next()?;
    let rest: Vec<&[u8]> = fields.collect();
    if !rest.len().is_multiple_of(3) {
        return None;
    }
    let num = |f: &[u8]| -> Option<Option<u32>> {
        if f.is_empty() {
            return Some(None);
        }
        std::str::from_utf8(f).ok()?.parse().ok().map(Some)
    };
    let targets = rest
        .chunks(3)
        .map(|t| {
            Some(Target {
                path: PathBuf::from(OsString::from_vec(t[0].to_vec())),
                line: num(t[1])?,
                column: num(t[2])?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(OpenRequest {
        targets,
        new_window: flags.contains(&b'n'),
        reuse: flags.contains(&b'r'),
        wait: flags.contains(&b'w'),
    })
}

/// Replies, one per line: `ok` once handled, then `done` for `--wait` when
/// every waited file is closed, or `error: …`.
pub const REPLY_OK: &str = "ok";
pub const REPLY_DONE: &str = "done";
pub const REPLY_ERROR: &str = "error: ";
