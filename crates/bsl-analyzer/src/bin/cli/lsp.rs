use std::error::Error;

pub fn run_lsp_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    use lsp_server::Connection;

    tracing::info!("Starting BSL Analyzer LSP server");

    let (connection, io_threads) = Connection::stdio();
    tracing::info!("LSP connection established via stdio");

    tracing::info!("Entering main loop");
    if let Err(e) = bsl_analyzer::main_loop(connection) {
        tracing::error!("Main loop error: {}", e);
        tracing::error!("Error chain: {:?}", e);
        return Err(e.into());
    }

    // Client may close the connection before we finish sending — log and continue.
    tracing::info!("Joining IO threads");
    if let Err(e) = io_threads.join() {
        tracing::debug!("IO threads join error (expected during shutdown): {}", e);
    }

    tracing::info!("LSP server shut down cleanly");
    Ok(())
}
