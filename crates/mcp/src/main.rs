//! `pgdesk-mcp`: stdio MCP server. Register it in an agent, e.g.
//! `claude mcp add pgdesk -- /path/to/pgdesk-mcp`.

use pgdesk_core::instances::AppPaths;
use pgdesk_mcp::PgDesk;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = PgDesk::new(AppPaths::default_macos())
        .serve(stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
