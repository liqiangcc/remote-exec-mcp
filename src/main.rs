use std::sync::Arc;

use anyhow::{bail, Context, Result};
use remote_exec_mcp::config::Config;
use remote_exec_mcp::mcp::RemoteExecMcp;
use remote_exec_mcp::runtime::RemoteExecRuntime;
use rmcp::{transport::stdio, ServiceExt};

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is reserved for MCP JSON-RPC frames when using stdio.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let config_path = config_path()?;
    let config = Config::load(&config_path)
        .with_context(|| format!("failed to load remote-exec-mcp config from {config_path}"))?;
    let runtime = Arc::new(RemoteExecRuntime::new(config)?);
    let service = RemoteExecMcp::new(runtime).serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

fn config_path() -> Result<String> {
    let mut args = std::env::args().skip(1);
    match args.next() {
        None => Ok(std::env::var("REMOTE_EXEC_MCP_CONFIG")
            .unwrap_or_else(|_| "config/example.yaml".to_owned())),
        Some(flag) if flag == "--config" => args
            .next()
            .context("--config requires a path argument"),
        Some(flag) if flag == "--help" || flag == "-h" => {
            eprintln!("Usage: remote-exec-mcp [--config PATH]\nEnvironment: REMOTE_EXEC_MCP_CONFIG");
            std::process::exit(0);
        }
        Some(other) => bail!("unexpected argument: {other}; use --config PATH"),
    }
}
