use crate::{file_diagnostics, Diagnostic, DiagnosticsConfig};
use base_db::{DiagnosticsConfigId, FileIdInput};
use ide_db::RootDatabase;
use std::sync::Arc;

#[salsa::tracked(lru = 256)]
pub fn file_diagnostics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
    config_id: DiagnosticsConfigId<'db>,
) -> Arc<Vec<Diagnostic>> {
    let total_start = std::time::Instant::now();
    let file_id = file_id_input.file_id(db);
    let config_input = config_id.config(db);
    let config = DiagnosticsConfig::from_input(&config_input);

    let _span = tracing::info_span!("file_diagnostics_query", file_id = file_id.0,).entered();

    let diagnostics = file_diagnostics(db, file_id, &config);
    tracing::info!(
        file_id = file_id.0,
        diagnostic_count = diagnostics.len(),
        elapsed_ms = total_start.elapsed().as_millis() as u64,
        "file diagnostics query complete",
    );

    Arc::new(diagnostics)
}
