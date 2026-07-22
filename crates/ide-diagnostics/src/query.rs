use crate::{file_diagnostics, Diagnostic, DiagnosticTag, DiagnosticsConfig, Fix, TextEdit};
use base_db::{DiagnosticsConfigId, FileIdInput};
use ide_db::RootDatabase;
use std::sync::Arc;
use stdx::heap::vec_bytes;

/// Approximate live heap of a memoised diagnostics list, for salsa's
/// `heap_size` hook: the backing vec plus each diagnostic's owned message,
/// tags, and fix payloads. New heap-owning fields in [`Diagnostic`] or
/// [`Fix`] must be added here too.
fn diagnostics_heap(v: &Arc<Vec<Diagnostic>>) -> usize {
    let mut bytes = vec_bytes::<Diagnostic>(v.len());
    for d in v.iter() {
        bytes += d.message.capacity();
        bytes += vec_bytes::<DiagnosticTag>(d.tags.len());
        bytes += vec_bytes::<Fix>(d.fixes.len());
        for fix in &d.fixes {
            bytes += fix.label.capacity();
            bytes += vec_bytes::<TextEdit>(fix.edits.len());
            for edit in &fix.edits {
                bytes += edit.new_text.capacity();
            }
        }
    }
    bytes
}

#[salsa::tracked(lru = 256, heap_size = diagnostics_heap, returns(clone))]
pub fn file_diagnostics_query<'db>(
    db: &'db dyn RootDatabase,
    file_id_input: FileIdInput<'db>,
    config_id: DiagnosticsConfigId<'db>,
) -> Arc<Vec<Diagnostic>> {
    let total_start = std::time::Instant::now();
    let file_id = file_id_input.file_id(db);
    let config_input = config_id.config(db);
    let config = DiagnosticsConfig::from_input(config_input);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticCode, Severity};
    use ide_db::TextRange;

    #[test]
    fn diagnostics_heap_counts_messages_and_fixes() {
        let message = "переменная не используется: ОченьДлинноеИмяПеременной".to_string();
        let fix_label = "Удалить переменную".to_string();
        let new_text = String::new();
        let owned = message.capacity() + fix_label.capacity() + new_text.capacity();
        let diagnostics = Arc::new(vec![Diagnostic {
            code: DiagnosticCode::UnreachableCode,
            message,
            severity: Severity::Warning,
            range: TextRange::new(0.into(), 10.into()),
            tags: vec![DiagnosticTag::Unnecessary],
            fixes: vec![Fix::safe(
                fix_label,
                vec![TextEdit { range: TextRange::new(0.into(), 10.into()), new_text }],
            )],
        }]);
        let bytes = diagnostics_heap(&diagnostics);
        // At least the owned string payloads plus the vec backing stores; well
        // under a kilobyte for a single small diagnostic.
        assert!(bytes > owned);
        assert!(bytes < 1024);
    }
}
