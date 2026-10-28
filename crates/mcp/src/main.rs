//! `sawit-mcp`: stdio MCP server. Register it in an agent, e.g.
//! `claude mcp add sawit -- /path/to/sawit-mcp`.

use sawit_core::instances::AppPaths;
use sawit_mcp::Sawit;
use rmcp::{ServiceExt, transport::stdio};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let service = Sawit::new(AppPaths::default_macos())
        .serve(stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
