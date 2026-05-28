use std::error::Error;

pub fn run_dap_server() -> Result<(), Box<dyn Error + Send + Sync>> {
    tracing::info!("Starting DAP debug adapter");
    bsl_debug::dap::run_dap_stdio();
    Ok(())
}
