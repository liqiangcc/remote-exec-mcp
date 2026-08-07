use std::path::PathBuf;

use anyhow::{Context, Result};
use remote_exec_mcp::config::Config;
use remote_exec_mcp::mcp::RemoteExecMcp;
use remote_exec_mcp::runtime::RemoteExecService;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let config_path = config_path()?;
    let config = Config::load(&config_path)?;
    let app = RemoteExecService::try_new(config)?;
    let service = RemoteExecMcp::new(app);
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
