use std::path::PathBuf;

use anyhow::{Context, Result};
use rmcp::{transport::stdio, ServiceExt};
use remote_exec_mcp::config::Config;
use remote_exec_mcp::mcp::RemoteExecMcp;
use remote_exec_mcp::runtime::RemoteExecService;

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is reserved for MCP stdio frames.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let config_path = config_path()?;
    let config = Config::load(&config_path)?;
    let service = RemoteExecMcp::new(RemoteExecService::new(config));
    let running = service.serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

fn config_path() -> Result<PathBuf> {
    if let Some(path) = std::env::args_os().nth(1) {
        return Ok(path.into());
    }
    std::env::var_os("REMOTE_EXEC_CONFIG")
        .map(PathBuf::from)
        .context("config path is required as argv[1] or REMOTE_EXEC_CONFIG")
}
