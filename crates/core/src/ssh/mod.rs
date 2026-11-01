//! SSH to remote servers: saved hosts (one JSON file), hosts read from
//! `~/.ssh/config`, the `ssh` command line for each, saved passwords in the
//! Keychain, and a quick reachability check.
//!
//! Sessions run the system `ssh` in a terminal, so keys, the agent,
//! `known_hosts`, `ProxyJump` and the user's own config all keep working.
//! Every session to the same server shares one connection (ControlMaster):
//! a second tab opens instantly and asks for nothing.

mod command;
mod config;
mod host;
mod host_list;
mod password;
mod probe;

pub use command::{Launch, connection_args, ensure_ssh_dir, launch, launch_alias, plain_command};
pub use config::{ConfigHost, load_config_hosts, parse_config, user_config_path};
pub use host::{Auth, SshHost, now, parse_forward, split_forwards};
pub use host_list::HostList;
pub use password::{
    askpass_main, delete_password, has_password, is_password_prompt, load_password, store_password,
};
pub use probe::{Probe, probe_alias, probe_host};

const KEYCHAIN_SERVICE: &str = "sawit.ssh";
/// Set on the askpass helper's environment: the host whose password to give.
pub const ASKPASS_ENV: &str = "SAWIT_SSH_ASKPASS";
/// The system client; `PATH` may hold an older Homebrew one, which is fine too.
const SSH: &str = "ssh";

fn query_words(query: &str) -> Vec<String> {
    query.split_whitespace().map(|w| w.to_lowercase()).collect()
}

#[cfg(test)]
mod tests;
