//! Unix socket transport for MCP server.

use crate::McpServer;
use anyhow::Result;
use rmcp::ServiceExt;
use std::path::Path;
use tokio::net::UnixListener;

/// Start MCP server on a Unix socket.
///
/// Accepts connections in a loop, spawning a new MCP session per client.
/// The server runs until the listener is dropped or an unrecoverable error occurs.
pub async fn serve_unix_socket(socket_path: &Path, server: McpServer) -> Result<()> {
    // Remove stale socket file if it exists
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }

    let listener = UnixListener::bind(socket_path)?;
    tracing::info!(?socket_path, "MCP server listening on unix socket");

    loop {
        let (stream, _addr) = listener.accept().await?;
        let server = server.clone();

        tokio::spawn(async move {
            tracing::debug!("MCP client connected");
            match server.serve(stream).await {
                Ok(session) => {
                    if let Err(e) = session.waiting().await {
                        tracing::debug!("MCP session ended: {e}");
                    }
                }
                Err(e) => {
                    tracing::warn!("MCP serve error: {e}");
                }
            }
        });
    }
}
