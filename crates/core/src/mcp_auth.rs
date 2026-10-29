//! Signing in to remote MCP servers through the clients' own commands:
//! `claude mcp login` (per Claude Code profile) and `codex mcp login`. Both
//! run in a pseudo-terminal, since they refuse to take the redirect address
//! without one. The sign-in link is read from their output and the address
//! the browser ends on is typed back in; tokens stay with the client.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::channel::mpsc;
use serde_json::Value;

use crate::mcp_clients::{CliCall, Client, Target, cli_call, run_cli};
use crate::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignIn {
    SignedIn,
    Needed,
    /// Failed to connect, or the client couldn't tell.
    Unknown,
}

/// `claude mcp list`: `name: address - ✓ Connected` per line.
pub fn parse_claude_list(text: &str) -> HashMap<String, SignIn> {
    text.lines()
        .filter_map(|line| {
            let (name, rest) = line.split_once(": ")?;
            let (_, state) = rest.rsplit_once(" - ")?;
            let state = if state.contains("Needs authentication") {
                SignIn::Needed
            } else if state.contains("Connected") {
                SignIn::SignedIn
            } else {
                SignIn::Unknown
            };
            Some((name.trim().to_string(), state))
        })
        .collect()
}

/// `codex mcp list --json`: an array with each server's `auth_status`.
pub fn parse_codex_list(json: &str) -> HashMap<String, SignIn> {
    let Ok(Value::Array(servers)) = serde_json::from_str::<Value>(json) else {
        return HashMap::new();
    };
    servers
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let state = match s.get("auth_status").and_then(|a| a.as_str()) {
                Some("not_logged_in") => SignIn::Needed,
                Some("o_auth" | "bearer_token") => SignIn::SignedIn,
                _ => SignIn::Unknown,
            };
            Some((name, state))
        })
        .collect()
}

/// Each server's sign-in state in one Claude Code or Codex profile. Claude
/// Code connects to every server to find out, so this takes a while.
pub async fn sign_in_states(target: &Target, cli: &Path) -> Result<HashMap<String, SignIn>> {
    let args: &[&str] = match target.client {
        Client::ClaudeCode => &["mcp", "list"],
        Client::Codex => &["mcp", "list", "--json"],
        _ => return Ok(HashMap::new()),
    };
    let call = cli_call(target, cli, args.iter().map(|a| a.to_string()).collect())
        .ok_or_else(|| CoreError::Config("This client has no CLI.".into()))?;
    let out = run_cli(&call, std::time::Duration::from_secs(90)).await?;
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(match target.client {
        Client::Codex => parse_codex_list(&text),
        _ => parse_claude_list(&text),
    })
}

/// The command that signs `target` in to the server `name`.
pub fn login_call(target: &Target, cli: &Path, name: &str) -> Option<CliCall> {
    match target.client {
        Client::ClaudeCode | Client::Codex => cli_call(
            target,
            cli,
            vec![
                "mcp".into(),
                "login".into(),
                name.into(),
                "--no-browser".into(),
            ],
        ),
        _ => None,
    }
}

/// Whether `text` looks like the address a sign-in redirects to.
pub fn is_callback_url(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("http://localhost") || t.starts_with("http://127.0.0.1"))
        && (t.contains("code=") || t.contains("error="))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginEvent {
    /// The address to open in a browser.
    Link(String),
    /// A line the client printed after the link, e.g. why an address was
    /// rejected.
    Message(String),
    /// The command ended. `message` is its last line.
    Exited { success: bool, message: String },
}

/// Removes terminal escape sequences (colours, cursor moves, hyperlinks).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // OSC: up to BEL or ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' {
                        break;
                    }
                    if c == '\x1b' {
                        chars.next();
                        break;
                    }
                }
            }
            // CSI: parameters, then a final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Prompts and instructions the client prints around the link.
fn is_prompt(line: &str) -> bool {
    let l = line.to_lowercase();
    [
        "or paste",
        "callback url",
        "waiting for authorization",
        "after signing in",
        "if the callback page",
        "visit this url",
        "authorize the mcp server",
        "starting authentication",
    ]
    .iter()
    .any(|p| l.starts_with(p))
}

/// Turns the client's output into events, one line at a time.
#[derive(Default)]
struct Parser {
    pending: Vec<u8>,
    link: bool,
    last: String,
}

impl Parser {
    fn feed(&mut self, bytes: &[u8], out: &mut Vec<LoginEvent>) {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            self.line(&String::from_utf8_lossy(&raw), out);
        }
    }

    fn line(&mut self, raw: &str, out: &mut Vec<LoginEvent>) {
        let clean = strip_ansi(raw);
        // A carriage return redraws the line: the last part is what shows.
        let line = clean
            .split('\r')
            .map(str::trim)
            .rfind(|s| !s.is_empty())
            .unwrap_or_default()
            .to_string();
        if line.is_empty() {
            return;
        }
        if !self.link
            && let Some(start) = line.find("https://")
        {
            let url: String = line[start..]
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            self.link = true;
            out.push(LoginEvent::Link(url));
            return;
        }
        if is_prompt(&line) {
            return;
        }
        self.last = line.clone();
        if self.link {
            out.push(LoginEvent::Message(line));
        }
    }

    fn finish(&mut self, out: &mut Vec<LoginEvent>) {
        if !self.pending.is_empty() {
            let rest = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).to_string();
            self.line(&rest, out);
        }
    }
}

/// A running sign-in. Dropping it stops the command.
pub struct Login {
    master: std::fs::File,
    pid: libc::pid_t,
    exited: Arc<AtomicBool>,
}

impl Login {
    /// Types the address the browser ended on into the client's prompt.
    pub fn submit(&self, address: &str) -> Result<()> {
        let line = format!("{}\r", address.trim());
        (&self.master).write_all(line.as_bytes())?;
        Ok(())
    }

    pub fn cancel(&self) {
        if !self.exited.load(Ordering::SeqCst) {
            // SAFETY: the command leads its own process group (setsid).
            unsafe { libc::killpg(self.pid, libc::SIGKILL) };
        }
    }
}

impl Drop for Login {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// macOS `TIOCSCTTY` (not exported by `libc` for Apple targets).
const TIOCSCTTY: libc::c_ulong = 0x2000_7461;

/// Starts `call` in a pseudo-terminal. Events arrive until `Exited`.
pub fn start(call: &CliCall) -> Result<(Login, mpsc::UnboundedReceiver<LoginEvent>)> {
    use std::os::fd::FromRawFd;
    use std::os::unix::process::CommandExt;

    let (mut master_fd, mut slave_fd) = (0, 0);
    // Wide, so the client never wraps the link.
    let mut size = libc::winsize {
        ws_row: 50,
        ws_col: 1000,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: valid out-pointers; the name and termios arguments may be null.
    let rc = unsafe {
        libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut size,
        )
    };
    if rc != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openpty returned two open descriptors that we now own.
    let master = unsafe { std::fs::File::from_raw_fd(master_fd) };
    let slave = unsafe { std::fs::File::from_raw_fd(slave_fd) };

    let mut cmd = std::process::Command::new(&call.program);
    cmd.args(&call.args).env("TERM", "xterm-256color");
    for (k, v) in &call.env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    cmd.stdin(slave.try_clone()?)
        .stdout(slave.try_clone()?)
        .stderr(slave);
    // SAFETY: only async-signal-safe calls between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            libc::ioctl(0, TIOCSCTTY, 0);
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    // It leads its own process group (setsid), so the group id is its pid.
    let pid = child.id() as libc::pid_t;
    // Close our copies of the terminal's other end, so reading ends when
    // the command exits.
    drop(cmd);

    let (tx, rx) = mpsc::unbounded();
    let exited = Arc::new(AtomicBool::new(false));
    let mut reader = master.try_clone()?;
    let done = exited.clone();
    std::thread::spawn(move || {
        let mut parser = Parser::default();
        let mut buf = [0u8; 8192];
        let mut events = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => parser.feed(&buf[..n], &mut events),
            }
            for e in events.drain(..) {
                let _ = tx.unbounded_send(e);
            }
        }
        parser.finish(&mut events);
        let status = child.wait();
        done.store(true, Ordering::SeqCst);
        for e in events.drain(..) {
            let _ = tx.unbounded_send(e);
        }
        let success = status.is_ok_and(|s| s.success());
        let _ = tx.unbounded_send(LoginEvent::Exited {
            success,
            message: parser.last,
        });
    });
    Ok((
        Login {
            master,
            pid,
            exited,
        },
        rx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_claude_and_codex_sign_in_states() {
        let claude = "Checking MCP server health…\n\n\
            plugin:figma:figma: https://mcp.figma.com/mcp (HTTP) - ! Needs authentication\n\
            cap: cap mcp serve - ✔ Connected\n\
            mobbin: https://api.mobbin.com/mcp (HTTP) - ✔ Connected\n\
            magnific: https://mcp.magnific.com (HTTP) - ! Needs authentication\n\
            broken: https://x.dev/mcp (HTTP) - ✗ Failed to connect\n";
        let s = parse_claude_list(claude);
        assert_eq!(s["magnific"], SignIn::Needed);
        assert_eq!(s["mobbin"], SignIn::SignedIn);
        assert_eq!(s["plugin:figma:figma"], SignIn::Needed);
        assert_eq!(s["broken"], SignIn::Unknown);
        assert!(!s.contains_key("Checking MCP server health…"));

        let codex = r#"[{"name":"postman-mcp","auth_status":"not_logged_in"},
            {"name":"manujujaya","auth_status":"o_auth"},
            {"name":"pencil","auth_status":"unsupported"}]"#;
        let s = parse_codex_list(codex);
        assert_eq!(s["postman-mcp"], SignIn::Needed);
        assert_eq!(s["manujujaya"], SignIn::SignedIn);
        assert_eq!(s["pencil"], SignIn::Unknown);
    }

    #[test]
    fn finds_the_link_in_claude_and_codex_output() {
        // Claude Code: an OSC 8 hyperlink around a coloured URL.
        let claude = "Starting authentication for \"magnific\"…\r\nVisit this URL to authorize:\r\n  \
            \x1b]8;;https://auth.magnific.com/auth?code_challenge=abc&state=x\x07\x1b[94mhttps://auth.magnific.com/auth?code_challenge=abc&state=x\x1b[39m\x1b]8;;\x07\r\n\r\n\
            Waiting for authorization… (^C to cancel)\r\n\x1b[1G\x1b[0JOr paste the redirect URL here: \x1b[33Ghttp://localhost:1/callback?code=bogus\r\r\n\
            That doesn't look like a redirect URL for this sign-in.\r\n";
        let mut p = Parser::default();
        let mut ev = Vec::new();
        // Split mid-sequence, as reads do.
        let (a, b) = claude.as_bytes().split_at(90);
        p.feed(a, &mut ev);
        p.feed(b, &mut ev);
        assert_eq!(
            ev,
            [
                LoginEvent::Link(
                    "https://auth.magnific.com/auth?code_challenge=abc&state=x".into()
                ),
                LoginEvent::Message(
                    "That doesn't look like a redirect URL for this sign-in.".into()
                ),
            ]
        );

        let codex = "Authorize the MCP server by opening this URL in your browser:\r\n\
            https://x.supabase.co/auth/v1/oauth/authorize?response_type=code&redirect_uri=http%3A%2F%2F127.0.0.1%3A60770%2Fcallback\r\n\r\n\
            After signing in, copy the full URL from your browser's address bar.\r\n\
            Callback URL (input hidden): \r\nError: No OAuth callback URL received before input closed\r\n";
        let mut p = Parser::default();
        let mut ev = Vec::new();
        p.feed(codex.as_bytes(), &mut ev);
        assert!(matches!(&ev[0], LoginEvent::Link(u) if u.ends_with("%2Fcallback")));
        assert_eq!(
            p.last,
            "Error: No OAuth callback URL received before input closed"
        );
    }

    #[test]
    fn recognises_callback_addresses() {
        assert!(is_callback_url(
            "http://localhost:55005/callback?code=abc&state=x"
        ));
        assert!(is_callback_url(
            " http://127.0.0.1:60770/callback/49Kj?code=abc\n"
        ));
        assert!(!is_callback_url(
            "https://auth.example.com/authorize?code_challenge=x"
        ));
        assert!(!is_callback_url("http://localhost:3000/"));
    }

    #[tokio::test]
    async fn runs_in_a_terminal_and_takes_the_address() {
        use futures::StreamExt;
        // Prints a link, then echoes what's typed only if stdin is a terminal.
        let script = "[ -t 0 ] || { echo 'Error: not a terminal'; exit 2; }; \
            echo 'Visit this URL to authorize:'; echo '  https://auth.example.com/a?x=1'; \
            printf 'Or paste the redirect URL here: '; read -r url; echo; echo \"Got $url\"";
        let call = CliCall {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            env: vec![],
        };
        let (login, mut rx) = start(&call).unwrap();
        assert_eq!(
            rx.next().await,
            Some(LoginEvent::Link("https://auth.example.com/a?x=1".into()))
        );
        login.submit("http://localhost:9/callback?code=1").unwrap();
        let mut last = None;
        while let Some(e) = rx.next().await {
            last = Some(e);
        }
        assert_eq!(
            last,
            Some(LoginEvent::Exited {
                success: true,
                message: "Got http://localhost:9/callback?code=1".into()
            })
        );
    }
}
