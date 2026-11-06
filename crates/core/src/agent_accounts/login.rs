//! Signing an account in without a terminal: the unmodified `claude auth login --claudeai` for
//! the account's folder, in the background. It prints a link (whose page shows a code) and waits
//! for that code on stdin; it also asks `$BROWSER` to open a second link that completes by itself.
//! `BROWSER` is a two-line script that only writes that link down. The code goes to Claude Code's
//! stdin and is not kept; Claude Code stores the sign-in in its own Keychain item.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use super::{Account, claude, run};
use crate::{CoreError, Result};

/// How long a sign-in may wait.
pub const LIFETIME_SECS: u64 = 10 * 60;
const URL_VAR: &str = "SAWIT_LOGIN_URL_FILE";

/// One sign-in under way.
pub struct Login {
    child: Child,
    stdin: Option<ChildStdin>,
    output: Arc<Mutex<String>>,
    capture: PathBuf,
}

impl Login {
    /// Starts Claude Code's sign-in for `account`. `email` is passed as a hint when it is one.
    pub fn start(
        claude_bin: &Path,
        account: &Account,
        home: &Path,
        root: &Path,
        email: Option<&str>,
    ) -> Result<Login> {
        let capture = super::store::dir(root).join("login").join(super::new_id());
        super::store::private_dir(&capture)?;
        let script = capture.join("record-url");
        std::fs::write(
            &script,
            format!("#!/bin/sh\nprintf %s \"$1\" > \"${URL_VAR}\"\n"),
        )?;
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        }
        let mut args = vec!["auth", "login", "--claudeai"];
        if let Some(e) = email.map(str::trim).filter(|e| e.contains('@')) {
            args.extend(["--email", e]);
        }
        let user = std::env::var("USER").unwrap_or_default();
        let mut child = Command::new(claude_bin)
            .args(&args)
            .env_clear()
            .envs(claude::environment(
                account,
                home,
                &user,
                &run::base_path(home),
            ))
            .env("BROWSER", &script)
            .env(URL_VAR, capture.join("url"))
            .current_dir(home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CoreError::Config(format!("Couldn't start Claude Code's sign-in: {e}")))?;
        let output = Arc::new(Mutex::new(String::new()));
        for pipe in [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        ]
        .into_iter()
        .flatten()
        {
            let output = output.clone();
            std::thread::spawn(move || {
                let mut pipe = pipe;
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    if let Ok(mut o) = output.lock() {
                        o.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            });
        }
        Ok(Login {
            stdin: child.stdin.take(),
            child,
            output,
            capture,
        })
    }

    fn text(&self) -> String {
        self.output.lock().map(|o| o.clone()).unwrap_or_default()
    }

    /// The link to open anywhere; its page shows a code to paste back.
    pub fn manual_url(&self) -> Option<String> {
        manual_url(&self.text())
    }

    /// The link `$BROWSER` was asked to open: it finishes the sign-in by itself on this Mac.
    pub fn callback_url(&self) -> Option<String> {
        let url = std::fs::read_to_string(self.capture.join("url")).ok()?;
        let url = url.trim();
        url.starts_with("https://").then(|| url.to_string())
    }

    /// Sends the code from the authorization page (`<code>#<state>`).
    pub fn submit(&mut self, code: &str) -> Result<()> {
        let code = code.trim();
        if !plausible_code(code) {
            return Err(CoreError::Config(
                "That doesn't look like the code from the sign-in page. Copy all of it, then paste it again.".into(),
            ));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| CoreError::Config("The sign-in has ended.".into()))?;
        stdin.write_all(format!("{code}\n").as_bytes())?;
        stdin.flush()?;
        Ok(())
    }

    /// The exit code once Claude Code has finished; `Ok(None)` while it waits.
    pub fn finished(&mut self) -> Result<Option<i32>> {
        Ok(self.child.try_wait()?.map(|s| s.code().unwrap_or(-1)))
    }

    /// Why it failed, from its output, links left out.
    pub fn failure(&self) -> Option<String> {
        failure_reason(&self.text())
    }

    /// Stops the sign-in.
    pub fn cancel(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            self.cancel();
        }
        let _ = std::fs::remove_dir_all(&self.capture);
    }
}

/// The first `https://` link after "browser" (or anywhere) in Claude Code's output.
pub fn manual_url(text: &str) -> Option<String> {
    let start = text.find("https://")?;
    let url: String = text[start..]
        .chars()
        .take_while(|c| !c.is_whitespace())
        .collect();
    (url.len() > "https://".len()).then_some(url)
}

/// A code looks like `<code>#<state>`: letters, digits and `-_#`, not too short.
pub fn plausible_code(code: &str) -> bool {
    code.len() >= 8
        && code.len() <= 2048
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_#.~".contains(c))
}

/// The last line of the output that says something, links replaced.
pub fn failure_reason(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with("Paste code here")
                && !l.starts_with("Opening browser")
                && !l.contains("didn't open")
        })
        .map(|l| {
            l.split_whitespace()
                .map(|w| if w.starts_with("http") { "<link>" } else { w })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .next_back()
}
