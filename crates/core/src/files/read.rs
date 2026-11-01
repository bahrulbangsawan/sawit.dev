//! Reading files safely: text for the editor, within its size and encoding limits, and the
//! bytes of a regular file only, never a pipe or a device.

use std::path::Path;

use super::MAX_TEXT_BYTES;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextFile {
    pub text: String,
    /// Why the file can't be edited (too big, binary); `text` is empty then.
    pub read_only_reason: Option<String>,
}

pub fn read_text(path: &Path) -> Result<TextFile> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        // FIFOs, sockets and devices: opening them can block or never end.
        return Ok(TextFile {
            text: String::new(),
            read_only_reason: Some("This isn't a regular file (it's a pipe, socket or device), so it can't be opened as text.".into()),
        });
    }
    let size = meta.len();
    if size > MAX_TEXT_BYTES {
        return Ok(TextFile {
            text: String::new(),
            read_only_reason: Some(format!(
                "This file is {:.1} MB, over the editor's {} MB limit. Open it in another app.",
                size as f64 / 1_048_576.0,
                MAX_TEXT_BYTES / 1_048_576
            )),
        });
    }
    let bytes = std::fs::read(path)?;
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Ok(TextFile {
            text,
            read_only_reason: None,
        }),
        _ => Ok(TextFile {
            text: String::new(),
            read_only_reason: Some("Binary or non-UTF-8 file. Sawit can't show it as text.".into()),
        }),
    }
}

/// The bytes of the regular file at `path` (a link is followed), read
/// through one handle, which is checked before anything is read. Anything
/// else is refused at once ([`std::io::ErrorKind::InvalidInput`]): a named
/// pipe would block the read for ever, and a device such as `/dev/zero`
/// never ends (a cloned repository can link a file to one). So is a file of
/// more than `max` bytes ([`std::io::ErrorKind::FileTooLarge`]). A missing
/// file is [`std::io::ErrorKind::NotFound`], as for [`std::fs::read`].
pub fn read_regular(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    use std::io::Read as _;

    let (file, len) = open_regular(path, max)?;
    let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    // A file that grew meanwhile is read only as far as `max`.
    file.take(max.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(too_large(max));
    }
    Ok(bytes)
}

/// Checks the file at `path` as [`read_regular`] does before it reads, and
/// reads nothing: `Ok` for a regular file (a link is followed) of at most
/// `max` bytes, else the error `read_regular` gives. It is for a file that
/// something else reads next, by its path, as the canvas engine reads the
/// file its `file.import` names: a pipe or a device is told at once, as the
/// open doesn't block. Something that replaces the file in between isn't
/// caught, which only a process of the user's own could do.
pub fn check_regular(path: &Path, max: u64) -> std::io::Result<()> {
    open_regular(path, max).map(|_| ())
}

/// The file at `path` opened for reading, and its length, when it is a
/// regular file of at most `max` bytes ([`read_regular`]).
fn open_regular(path: &Path, max: u64) -> std::io::Result<(std::fs::File, u64)> {
    use std::os::unix::fs::OpenOptionsExt as _;

    // Opening a pipe for reading waits for a writer, unless it doesn't block.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "not a regular file (a folder, a pipe, a socket or a device)",
        ));
    }
    if meta.len() > max {
        return Err(too_large(max));
    }
    Ok((file, meta.len()))
}

/// [`read_regular`]'s error for a file of more than `max` bytes.
fn too_large(max: u64) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::FileTooLarge,
        format!(
            "it holds more than {} MB, more than is read from one file",
            max / (1024 * 1024)
        ),
    )
}
