//! `sawit github token`: hands the app's one GitHub sign-in to an AI client as it connects to
//! GitHub's MCP server, so no client's config ever holds the token. The config holds this
//! command instead: Claude Code runs it as the server's `headersHelper` and Codex as its
//! `http_headers_helper` (`--header`), and the other clients run the `mcp-remote` bridge through
//! it (`--exec`), which reads the header from its environment.
//!
//! The token comes from the Keychain item the app signs in to (`sawit`/`github`, or with a data
//! folder of its own `github:<SAWIT_HOME>`), each time it runs. Signed out, it prints nothing on
//! stdout and says why on stderr, so a client fails cleanly rather than sending an empty header.
//!
//! Any program that can run this command as you gets the token: the same as the Keychain item
//! itself, which `security` reads without asking. That is the exposure the shared sign-in accepts.

use std::ffi::OsString;

use crate::github::TokenStore;

/// The variable `--exec` puts `Bearer <token>` in, for the program it starts: `mcp-remote` puts
/// `${SAWIT_GITHUB_AUTHORIZATION}` in its `--header` from its environment.
pub const ENV: &str = "SAWIT_GITHUB_AUTHORIZATION";

/// What a client reads when Sawit isn't signed in to GitHub (or the Keychain item is gone).
pub const SIGNED_OUT: &str = "sawit: not signed in to GitHub. Sign in from Sawit (Sign In to GitHub in the title bar), then reconnect this MCP server.";

/// `sawit github token …`: how the token is handed over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenArgs {
    /// `--header`: `{"Authorization":"Bearer <token>"}` on stdout, the JSON object of headers
    /// Claude Code's `headersHelper` and Codex's `http_headers_helper` read.
    Header,
    /// `--exec <program> [args…]`: runs the program with `Bearer <token>` in [`ENV`], in place of
    /// this process, so its stdin and stdout are the client's.
    Exec(Vec<OsString>),
}

impl TokenArgs {
    /// The words after `sawit github`.
    pub fn parse(args: &[OsString]) -> std::result::Result<TokenArgs, String> {
        let words: Vec<Option<&str>> = args.iter().map(|a| a.to_str()).collect();
        match words.as_slice() {
            [Some("token"), Some("--header")] => Ok(TokenArgs::Header),
            [Some("token"), Some("--exec"), _, ..] => Ok(TokenArgs::Exec(args[2..].to_vec())),
            [Some("token"), Some("--exec")] => {
                Err("`sawit github token --exec` needs the program to run.".into())
            }
            [Some("token"), ..] => Err(
                "`sawit github token` hands the GitHub sign-in to an MCP client: --header (JSON for Claude Code's headersHelper and Codex's http_headers_helper) or --exec <program> [args…] (with the header in SAWIT_GITHUB_AUTHORIZATION)."
                    .into(),
            ),
            _ => Err("`sawit github` takes one word: token.".into()),
        }
    }
}

/// The JSON object of headers `--header` prints for `token`.
pub fn header_json(token: &str) -> String {
    serde_json::json!({ "Authorization": format!("Bearer {token}") }).to_string()
}

/// The sign-in `store` holds, or why there is none to hand over.
fn token(store: &dyn TokenStore) -> std::result::Result<String, String> {
    match store.load() {
        Ok(Some(token)) if !token.trim().is_empty() => Ok(token.trim().to_string()),
        Ok(_) => Err(SIGNED_OUT.into()),
        Err(e) => Err(format!("sawit: couldn't read the GitHub sign-in: {e}")),
    }
}

/// `--header` with the sign-in `store` holds: what goes on stdout, or the message for stderr.
pub fn header(store: &dyn TokenStore) -> std::result::Result<String, String> {
    token(store).map(|t| header_json(&t))
}

/// `program` set up to run with the sign-in `store` holds in [`ENV`].
pub fn exec_command(
    store: &dyn TokenStore,
    program: &[OsString],
) -> std::result::Result<std::process::Command, String> {
    let token = token(store)?;
    let (first, rest) = program
        .split_first()
        .ok_or("`sawit github token --exec` needs the program to run.")?;
    let mut cmd = std::process::Command::new(first);
    cmd.args(rest).env(ENV, format!("Bearer {token}"));
    Ok(cmd)
}

/// Runs `sawit github token …` with the user's Keychain item and returns the exit status: 0 once
/// handed over, 1 signed out (or the program couldn't start).
pub fn run(args: TokenArgs) -> i32 {
    let store = crate::github::Keychain::user();
    match args {
        TokenArgs::Header => match header(&store) {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
        TokenArgs::Exec(program) => match exec_command(&store, &program) {
            Ok(mut cmd) => {
                use std::os::unix::process::CommandExt as _;
                // Only returns when the program couldn't start; its error never holds the token.
                let e = cmd.exec();
                eprintln!(
                    "sawit: couldn't start {}: {e}",
                    program[0].to_string_lossy()
                );
                1
            }
            Err(e) => {
                eprintln!("{e}");
                1
            }
        },
    }
}

/// The header helper Claude Code (`headersHelper`) and Codex (`http_headers_helper`) run for
/// GitHub's MCP server: this `sawit`, by its absolute path (quoted for the shell both run it in),
/// printing the header.
pub fn headers_helper(sawit: &str) -> String {
    format!(
        "{} github token --header",
        crate::cli::shell_quote(sawit.as_ref())
    )
}

/// The arguments of `sawit` that start the `mcp-remote` bridge to `url` with the sign-in: the
/// runner and what goes before the package (`npx` and `-y`, `bunx`, `pnpm` and `dlx`), then
/// `mcp-remote <url> --header Authorization:${SAWIT_GITHUB_AUTHORIZATION}`. The header is
/// written as the variable's name: `mcp-remote` fills it in from its environment, so the token is
/// in no config and on no command line.
pub fn bridge_args(runner: &str, runner_args: &[String], url: &str) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "github".into(),
        "token".into(),
        "--exec".into(),
        runner.into(),
    ];
    args.extend(runner_args.iter().cloned());
    args.extend([
        "mcp-remote".into(),
        url.into(),
        "--header".into(),
        format!("Authorization:${{{ENV}}}"),
    ]);
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::MemoryStore;

    fn os(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// Signed in, `--header` prints the JSON object of headers Claude Code reads, and nothing
    /// else; signed out (or a blank item), nothing for stdout and a message saying how to sign in.
    #[test]
    fn the_header_is_the_sign_in_or_nothing() {
        let store = MemoryStore::with("gho_abc");
        let json = header(&store).unwrap();
        assert_eq!(json, r#"{"Authorization":"Bearer gho_abc"}"#);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 1);

        let out = MemoryStore::default();
        let said = header(&out).unwrap_err();
        assert_eq!(said, SIGNED_OUT);
        assert!(said.contains("Sign in"), "{said}");
        assert_eq!(header(&MemoryStore::with("  ")).unwrap_err(), SIGNED_OUT);
    }

    /// `--exec` gives the program the header in its environment, never on its command line;
    /// signed out it starts nothing.
    #[test]
    fn exec_puts_the_header_in_the_environment_only() {
        let program = os(&["/usr/bin/env"]);
        let cmd = exec_command(&MemoryStore::with("gho_abc"), &program).unwrap();
        let env: Vec<_> = cmd.get_envs().collect();
        assert_eq!(
            env,
            [(
                std::ffi::OsStr::new(ENV),
                Some(std::ffi::OsStr::new("Bearer gho_abc"))
            )]
        );
        assert_eq!(cmd.get_args().count(), 0);
        assert_eq!(
            exec_command(&MemoryStore::default(), &program).unwrap_err(),
            SIGNED_OUT
        );
    }

    /// It runs the program it's given, and the program reads the header from its environment.
    #[test]
    fn the_program_run_reads_the_header() {
        let program = os(&["/bin/sh", "-c", "printf %s \"$SAWIT_GITHUB_AUTHORIZATION\""]);
        let out = exec_command(&MemoryStore::with("gho_abc"), &program)
            .unwrap()
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "Bearer gho_abc");
    }

    #[test]
    fn its_words_are_checked() {
        assert_eq!(
            TokenArgs::parse(&os(&["token", "--header"])),
            Ok(TokenArgs::Header)
        );
        assert_eq!(
            TokenArgs::parse(&os(&["token", "--exec", "/x/npx", "-y", "mcp-remote"])),
            Ok(TokenArgs::Exec(os(&["/x/npx", "-y", "mcp-remote"])))
        );
        for bad in [
            &[][..],
            &["token"],
            &["token", "--exec"],
            &["token", "--header", "x"],
            &["token", "--nope"],
            &["login"],
        ] {
            assert!(TokenArgs::parse(&os(bad)).is_err(), "{bad:?}");
        }
    }

    /// What the configs hold: the helper by its absolute path, quoted for a shell, and the
    /// bridge with the variable's name where the token would go.
    #[test]
    fn the_configs_name_the_command_never_the_token() {
        assert_eq!(
            headers_helper("/Applications/Sawit.app/Contents/MacOS/sawit"),
            "'/Applications/Sawit.app/Contents/MacOS/sawit' github token --header"
        );
        assert_eq!(
            headers_helper("/Users/me/My Apps/it's/sawit"),
            r"'/Users/me/My Apps/it'\''s/sawit' github token --header"
        );
        assert_eq!(
            bridge_args(
                "/opt/homebrew/bin/npx",
                &["-y".into()],
                "https://api.githubcopilot.com/mcp/"
            ),
            [
                "github",
                "token",
                "--exec",
                "/opt/homebrew/bin/npx",
                "-y",
                "mcp-remote",
                "https://api.githubcopilot.com/mcp/",
                "--header",
                "Authorization:${SAWIT_GITHUB_AUTHORIZATION}",
            ]
        );
    }
}
