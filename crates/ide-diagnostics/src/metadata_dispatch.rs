//! Metadata-based diagnostics collection.
//!
//! This module collects diagnostics that use module_metadata from HIR.

use crate::{handlers, Diagnostic, DiagnosticsContext};

/// Collect metadata-based diagnostics using module_metadata from HIR.
///
/// Phase 2 diagnostics that have been migrated to use ModuleMetadata directly
/// instead of loading Configuration for each file. These are part of module_bodies()
/// and are cached by Salsa for performance.
///
/// Returns empty vec for test contexts where source_root is not set.
pub fn collect_metadata_diagnostics(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let module_bodies =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ctx.module_bodies())) {
            Ok(bodies) => bodies,
            Err(_) => return Vec::new(),
        };

    let mut diagnostics = Vec::new();

    let metadata = ctx.module_metadata();
    let metadata_ref = metadata.as_ref();

    diagnostics.extend(handlers::common_module_invalid_type::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_client::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_global::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_cached::from_metadata(metadata_ref, ctx));

    diagnostics
        .extend(handlers::common_module_name_client_server::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_full_access::from_metadata(metadata_ref, ctx));

    diagnostics
        .extend(handlers::common_module_name_global_client::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_server_call::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::common_module_name_words::from_metadata(metadata_ref, ctx));

    for var in module_bodies.module_vars() {
        if var.is_export {
            if let Some(diag) = handlers::export_variables::from_hir(&var.name, var.range, ctx) {
                diagnostics.push(diag);
            }
        }
    }

    diagnostics
        .extend(handlers::same_metadata_object_and_child_names::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::deny_incomplete_values::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::metadata_object_name_length::from_metadata(metadata_ref, ctx));

    diagnostics
        .extend(handlers::wrong_data_path_for_form_elements::from_metadata(metadata_ref, ctx));

    diagnostics.extend(handlers::wrong_http_service_handler::from_metadata(metadata_ref, ctx));

    diagnostics
}
