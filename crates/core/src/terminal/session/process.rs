//! What runs in a session's foreground, and where, as the OS tells it.

use std::path::PathBuf;

use super::Session;

impl Session {
    /// Whether the program started in this PTY is in the foreground: for a
    /// shell, it is at its prompt rather than running a command. Always false
    /// where that cannot be told (Windows).
    pub fn at_prompt(&self) -> bool {
        #[cfg(unix)]
        if let Some((pid, file)) = &self.foreground {
            use std::os::fd::AsRawFd;
            // The program leads its own session, so its group id is its pid.
            let pgrp = unsafe { libc::tcgetpgrp(file.as_raw_fd()) };
            return pgrp > 0 && pgrp as u32 == *pid;
        }
        false
    }

    /// The current folder of the program started in this PTY (for a shell,
    /// where it has `cd`ed to). `None` where that cannot be read: off macOS,
    /// or when the process is gone.
    pub fn cwd(&self) -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        if let Some((pid, _)) = &self.foreground {
            use std::os::unix::ffi::OsStrExt;
            // SAFETY: an all-zero proc_vnodepathinfo is valid plain data.
            let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
            let got = unsafe {
                libc::proc_pidinfo(
                    *pid as libc::c_int,
                    libc::PROC_PIDVNODEPATHINFO,
                    0,
                    (&mut info as *mut libc::proc_vnodepathinfo).cast(),
                    size,
                )
            };
            if got < size {
                return None;
            }
            // The path is a MAXPATHLEN buffer that libc declares as nested arrays.
            let path = &info.pvi_cdir.vip_path;
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(path.as_ptr().cast::<u8>(), std::mem::size_of_val(path))
            };
            let len = bytes.iter().position(|&b| b == 0)?;
            return (len > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&bytes[..len])));
        }
        None
    }

    /// The process group running in the foreground when it is not the
    /// program itself: for a shell, the command it is running.
    pub fn foreground_job(&self) -> Option<u32> {
        #[cfg(unix)]
        if let Some((pid, file)) = &self.foreground {
            use std::os::fd::AsRawFd;
            let pgrp = unsafe { libc::tcgetpgrp(file.as_raw_fd()) };
            return (pgrp > 0 && pgrp as u32 != *pid).then_some(pgrp as u32);
        }
        None
    }

    /// The name of the program in the foreground when it isn't the program
    /// started here (for a shell: the command it runs), as the OS reports
    /// it. `None` where that can't be read.
    pub fn foreground_name(&self) -> Option<String> {
        // The group's leader has the group's id as its pid.
        crate::terminal::agents::process_name(self.foreground_job()?)
    }
}
