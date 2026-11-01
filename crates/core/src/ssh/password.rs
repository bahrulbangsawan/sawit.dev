//! Saved passwords: kept in the Keychain, and given to `ssh` by the askpass helper.

use super::{KEYCHAIN_SERVICE, SshHost};
use crate::Result;

pub fn load_password(host: &SshHost) -> Result<Option<String>> {
    load_password_for(host.keychain_account())
}

fn load_password_for(account: &str) -> Result<Option<String>> {
    crate::keychain::get(KEYCHAIN_SERVICE, account)
}

/// Whether a password is saved for `host`, without reading (or asking for) it.
pub fn has_password(host: &SshHost) -> bool {
    matches!(
        crate::keychain::exists(KEYCHAIN_SERVICE, host.keychain_account()),
        Ok(true)
    )
}

pub fn store_password(host: &SshHost, password: &str) -> Result<()> {
    crate::keychain::set(KEYCHAIN_SERVICE, host.keychain_account(), password)
}

pub fn delete_password(host: &SshHost) -> Result<()> {
    crate::keychain::delete(KEYCHAIN_SERVICE, host.keychain_account())
}

/// Whether `ssh` is asking for the account password (not a key passphrase,
/// a one-time code or a host key confirmation).
pub fn is_password_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    p.contains("password") && !p.contains("passphrase") && !p.contains("yes/no")
}

/// Runs as `SSH_ASKPASS`: `ssh` passes the prompt and reads the answer from
/// stdout. The first password prompt gets the saved password; anything else
/// (a retry after a wrong password, a code, `yes/no`) is asked in the
/// terminal the session runs in. Returns the exit code.
pub fn askpass_main(account: &str, prompt: &str) -> i32 {
    if is_password_prompt(prompt)
        && first_prompt_of_parent()
        && let Ok(Some(password)) = load_password_for(account)
    {
        println!("{password}");
        return 0;
    }
    match ask_tty(prompt) {
        Some(answer) => {
            println!("{answer}");
            0
        }
        None => 1,
    }
}

/// True the first time this is asked for the calling `ssh` process, so a
/// rejected password is typed by hand instead of retried.
fn first_prompt_of_parent() -> bool {
    #[cfg(unix)]
    {
        let ppid = unsafe { libc::getppid() };
        let marker = std::env::temp_dir().join(format!("sawit-askpass-{ppid}"));
        let fresh = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .is_ok();
        // Markers of finished sessions are removed as they are noticed.
        if fresh && let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(pid) = name.strip_prefix("sawit-askpass-")
                    && let Ok(pid) = pid.parse::<i32>()
                    && pid != ppid
                    && unsafe { libc::kill(pid, 0) } != 0
                {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
        fresh
    }
    #[cfg(not(unix))]
    true
}

/// Asks on the controlling terminal; input is hidden unless it is a
/// `yes/no` question.
fn ask_tty(prompt: &str) -> Option<String> {
    #[cfg(unix)]
    {
        use std::io::{BufRead, Write};
        use std::os::fd::AsRawFd;
        let tty = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .ok()?;
        let mut out = &tty;
        let _ = out.write_all(prompt.as_bytes());
        let _ = out.flush();
        let fd = tty.as_raw_fd();
        let hide = !prompt.to_lowercase().contains("yes/no");
        let mut saved: libc::termios = unsafe { std::mem::zeroed() };
        let have = unsafe { libc::tcgetattr(fd, &mut saved) } == 0;
        if hide && have {
            let mut quiet = saved;
            quiet.c_lflag &= !libc::ECHO;
            quiet.c_lflag |= libc::ECHONL;
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &quiet) };
        }
        let mut line = String::new();
        let read = std::io::BufReader::new(&tty).read_line(&mut line);
        if hide && have {
            unsafe { libc::tcsetattr(fd, libc::TCSANOW, &saved) };
        }
        read.ok().filter(|n| *n > 0)?;
        Some(line.trim_end_matches(['\r', '\n']).to_string())
    }
    #[cfg(not(unix))]
    {
        let _ = prompt;
        None
    }
}
