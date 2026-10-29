//! `sawit-mcp`: stdio MCP server. Register it in an agent, e.g.
//! `claude mcp add sawit -- /path/to/sawit-mcp`.

use rmcp::{ServiceExt, transport::stdio};
use sawit_core::instances::AppPaths;
use sawit_mcp::Sawit;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // `ssh_exec` runs ssh with this binary as SSH_ASKPASS for hosts with a
    // saved password: answer the prompt and exit.
    if let Some(account) = std::env::var_os(sawit_core::ssh::ASKPASS_ENV) {
        let prompt = std::env::args().nth(1).unwrap_or_default();
        std::process::exit(sawit_mcp::askpass(&account.to_string_lossy(), &prompt));
    }
    let service = Sawit::new(AppPaths::default_macos()).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
