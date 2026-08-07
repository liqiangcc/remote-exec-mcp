//! MCP protocol adapter boundary.
//!
//! This module depends on application/catalog ports only. It must not call SSH,
//! SFTP, secret providers, or concrete executors directly.

pub mod tools;
