//! Salsa-cached diagnostics query.

use crate::{diagnostics, Diagnostic, DiagnosticsConfig, DiagnosticsContext};
use base_db::{DiagnosticsConfigId, FileIdInput};
use ide_db::RootDatabase;
use ide_db::SalsaProvider;
use std::sync::Arc;

/// Salsa-cached diagnostics query.
///
/// Computes diagnostics for a file with the given configuration.
/// Results are cached by Salsa and automatically invalidated when:
/// - File content changes (via FileIdInput dependency)
/// - Config changes (via DiagnosticsConfigId)
///
/// ## Performance
/// - **LRU cache:** 256 files
/// - **First call:** ~700ms (full computation)
/// - **Cached call:** < 1ms (cache hit)
/// - **After file change:** ~700ms (recomputes for that file only)
/// - **After config change:** ~700ms × N files (all invalidated)
#[salsa::tracked(lru = 256)]
pub fn file_diagnostics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
    config_id: DiagnosticsConfigId<'db>,
) -> Arc<Vec<Diagnostic>> {
    let file_id = file_id_input.file_id(db);
    let config_input = config_id.config(db);
    let config = DiagnosticsConfig::from_input(&config_input);

    let _span = tracing::info_span!("file_diagnostics_query", file_id = file_id.0,).entered();

    let config_path_input = ide_db::configuration_path_for_file(db, file_id);
    let provider = SalsaProvider::new(db, config_path_input);
    let ctx = DiagnosticsContext::new(&config, file_id, &provider);

    Arc::new(diagnostics(&ctx))
}
