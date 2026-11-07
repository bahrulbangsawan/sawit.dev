//! A process's executable and arguments, and its name, as the OS keeps them.

/// A process's executable and its arguments (`argv[0]` included), as the OS
/// keeps them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessArgs {
    pub exec_path: String,
    pub argv: Vec<String>,
}

/// Reads `KERN_PROCARGS2`'s layout: the argument count, the executable's
/// path, padding NULs, then the arguments, each NUL-ended (the environment
/// follows, and is left alone).
pub fn parse_procargs(buf: &[u8]) -> Option<ProcessArgs> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let rest = &buf[4..];
    let end = rest.iter().position(|&b| b == 0)?;
    let exec_path = String::from_utf8_lossy(&rest[..end]).into_owned();
    let mut at = end;
    while rest.get(at) == Some(&0) {
        at += 1;
    }
    let mut argv = Vec::new();
    for _ in 0..argc.max(0) {
        let tail = rest.get(at..)?;
        let len = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
        argv.push(String::from_utf8_lossy(&tail[..len]).into_owned());
        at += len + 1;
    }
    Some(ProcessArgs { exec_path, argv })
}

/// Process `pid`'s `KERN_PROCARGS2` buffer: its executable, arguments and environment.
fn procargs(pid: u32) -> Option<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let mut max: libc::c_int = 0;
        let mut size = std::mem::size_of::<libc::c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `max` is a c_int, and `size` says so.
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&mut max as *mut libc::c_int).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 || max <= 0 {
            return None;
        }
        let mut buf = vec![0u8; max as usize];
        let mut size = buf.len();
        let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as libc::c_int];
        // SAFETY: `buf` holds `size` bytes, the most the kernel writes.
        let ok = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                3,
                buf.as_mut_ptr().cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if ok != 0 {
            return None;
        }
        buf.truncate(size);
        Some(buf)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// The executable and arguments of process `pid`. Only the user's own
/// processes can be read; `None` for others, for one that has ended, and off
/// macOS.
pub fn process_args(pid: u32) -> Option<ProcessArgs> {
    parse_procargs(&procargs(pid)?)
}

/// The variables that say which account an agent runs as: Claude Code's and Codex's folder.
pub const PROFILE_VARS: &[&str] = &["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// Which account folder process `pid` runs with: the first of [`PROFILE_VARS`] in its
/// environment, as (variable, value). No other variable is read or kept.
pub fn profile_env(pid: u32) -> Option<(String, String)> {
    parse_profile_env(&procargs(pid)?)
}

/// [`profile_env`] from a `KERN_PROCARGS2` buffer: the environment follows the arguments.
pub fn parse_profile_env(buf: &[u8]) -> Option<(String, String)> {
    let argc = i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?);
    let rest = &buf[4..];
    let mut at = rest.iter().position(|&b| b == 0)?;
    while rest.get(at) == Some(&0) {
        at += 1;
    }
    let mut strings = rest[at..].split(|&b| b == 0);
    for _ in 0..argc.max(0) {
        strings.next()?;
    }
    strings
        .take_while(|s| !s.is_empty())
        .filter_map(|s| std::str::from_utf8(s).ok())
        .filter_map(|s| s.split_once('='))
        .find(|(k, _)| PROFILE_VARS.contains(k))
        .map(|(k, v)| (k.to_string(), v.to_string()))
}

/// The name the OS gives process `pid` (its executable's, cut to 16
/// characters on macOS). `None` when it can't be read, as off macOS.
pub fn process_name(pid: u32) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let mut name = [0u8; 64];
        // SAFETY: `name` holds the length given.
        let len = unsafe {
            libc::proc_name(
                pid as libc::c_int,
                name.as_mut_ptr().cast(),
                name.len() as u32,
            )
        };
        if len > 0 {
            return Some(String::from_utf8_lossy(&name[..len as usize]).into_owned());
        }
    }
    let _ = pid;
    None
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn only_the_profile_variable_is_read_from_the_environment() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/claude\0\0\0claude\0-c\0HOME=/u\0SECRET=x\0CLAUDE_CONFIG_DIR=/u/.claude-work\0\0");
        assert_eq!(
            parse_profile_env(&buf),
            Some(("CLAUDE_CONFIG_DIR".into(), "/u/.claude-work".into()))
        );
        let mut none = 1i32.to_ne_bytes().to_vec();
        none.extend_from_slice(b"/bin/sh\0sh\0HOME=/u\0\0");
        assert_eq!(parse_profile_env(&none), None);
    }
}
