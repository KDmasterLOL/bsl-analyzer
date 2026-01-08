//! Diagnostic: UnusedLocalVariable
//!
//! Detects local variables that are declared but never used.
//!
//! ## Implementation
//!
//! Uses backward liveness dataflow analysis to accurately detect unused variables.
//! A variable is "unused" if it's never live at any program point (never read).
//!
//! **Algorithm:**
//! 1. Build CFG for each method
//! 2. Run backward liveness analysis (OUT → IN)
//! 3. Check if each declared variable is live at entry point
//! 4. If not live → unused → report diagnostic
//!
//! **Why liveness analysis?**
//! - Handles control flow correctly (loops, branches, etc.)
//! - Distinguishes read vs write (assignment alone doesn't make variable "used")
//! - Fixes false positives from simple tracking (e.g., While loop control variables)
//!
//! ## Severity
//! Info (with Unnecessary tag)
//!
//! ## Example
//! ```bsl
//! // Bad - unused variable
//! Процедура Тест()
//!     Перем НеИспользуется;  // Warning: unused
//!     Сообщить("Привет");
//! КонецПроцедуры
//!
//! // Good - variable is used
//! Процедура Тест()
//!     Перем Сообщение;
//!     Сообщение = "Привет";
//!     Сообщить(Сообщение);
//! КонецПроцедуры
//! ```

use crate::{Diagnostic, DiagnosticCode, DiagnosticTag, DiagnosticsContext, Severity};
use hir_def::{MethodId, ModuleId};
use ide_db::RootDatabase;
use ide_db::TextRange;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

// ============================================================================
// Profiling for UnusedLocalVariable diagnostic
// ============================================================================

/// Total time spent in liveness analysis (nanoseconds)
pub static LIVENESS_TIME_NS: AtomicU64 = AtomicU64::new(0);

/// Total time spent checking implicit variables (nanoseconds)
pub static IMPLICIT_CHECK_TIME_NS: AtomicU64 = AtomicU64::new(0);

/// Total methods analyzed
pub static METHODS_ANALYZED: AtomicU64 = AtomicU64::new(0);

/// Total implicit variables checked
pub static IMPLICIT_VARS_CHECKED: AtomicU64 = AtomicU64::new(0);

/// Total blocks iterated for implicit var check
pub static BLOCKS_ITERATED: AtomicU64 = AtomicU64::new(0);

/// Reset profiling counters.
pub fn reset_profiling() {
    LIVENESS_TIME_NS.store(0, Ordering::Relaxed);
    IMPLICIT_CHECK_TIME_NS.store(0, Ordering::Relaxed);
    METHODS_ANALYZED.store(0, Ordering::Relaxed);
    IMPLICIT_VARS_CHECKED.store(0, Ordering::Relaxed);
    BLOCKS_ITERATED.store(0, Ordering::Relaxed);
}

/// Print profiling stats.
pub fn print_profiling() {
    let liveness_ms = LIVENESS_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let implicit_ms = IMPLICIT_CHECK_TIME_NS.load(Ordering::Relaxed) / 1_000_000;
    let methods = METHODS_ANALYZED.load(Ordering::Relaxed);
    let implicit_vars = IMPLICIT_VARS_CHECKED.load(Ordering::Relaxed);
    let blocks = BLOCKS_ITERATED.load(Ordering::Relaxed);

    eprintln!("\n=== UnusedLocalVariable Profiling ===");
    eprintln!("  liveness_time_ms:     {:>12}", liveness_ms);
    eprintln!("  implicit_check_ms:    {:>12}", implicit_ms);
    eprintln!("  methods_analyzed:     {:>12}", methods);
    eprintln!("  implicit_vars:        {:>12}", implicit_vars);
    eprintln!("  blocks_iterated:      {:>12}", blocks);
}

/// Collect UnusedLocalVariable diagnostics using liveness analysis.
///
/// This function performs dataflow-based detection of unused local variables:
/// 1. Iterates over all methods in the module
/// 2. Runs liveness analysis for each method (cached by Salsa)
/// 3. Checks which declared variables are never live
/// 4. Generates diagnostics for unused variables
///
/// **Performance:**
/// - O(methods × avg_cfg_size) - liveness analysis is cached by Salsa
/// - Only recomputed when method code changes
/// - CFG is shared across multiple analyses
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::UnusedLocalVariable) {
        return vec![];
    }

    let mut diagnostics = Vec::new();
    let module_id = ModuleId::new(ctx.file_id);

    // Load module_bodies ONCE for the entire file
    let module_bodies = ctx.db.module_bodies(module_id);

    // Check each method for unused local variables
    // Pass module_bodies and body directly to avoid repeated Salsa lookups
    for (local_id, body) in module_bodies.iter_bodies() {
        let method_id = MethodId { module: module_id, local_id };
        diagnostics.extend(check_method_direct(
            ctx.db,
            method_id,
            &module_bodies, // Pass module_bodies directly
            body,           // Pass body directly
            ctx,
        ));
    }

    // Check module-level code for unused variables (Phase 2)
    diagnostics.extend(check_module_level_code(ctx.db, module_id, ctx));

    // TODO_OLD: Check module-level code (Phase 2)
    // if let Some(module_code) = module_bodies.module_code() {
    //     diagnostics.extend(check_module_code(ctx.db, module_id, module_code, ctx));
    // }

    diagnostics
}

/// Check a single method for unused variables using direct CFG/liveness (optimized).
///
/// This function avoids repeated Salsa lookups by accepting pre-loaded module_bodies and body.
/// It builds CFG and runs liveness analysis directly instead of going through Salsa queries.
fn check_method_direct(
    _db: &dyn RootDatabase,
    method_id: MethodId,
    module_bodies: &hir_def::ModuleBodies,
    body: &hir_def::Body,
    _ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    METHODS_ANALYZED.fetch_add(1, Ordering::Relaxed);
    let mut diagnostics = Vec::new();

    // Get source_map
    let source_map = match module_bodies.source_map(method_id.local_id) {
        Some(sm) => sm,
        None => return diagnostics,
    };

    // Build CFG directly (no Salsa query)
    let cfg = cfg::CfgBuilder::new().build_graph_from_hir(&body.body_stmts, body, Some(source_map));

    // Build variable index (from_body already returns Arc<VariableIndex>)
    let var_index = dataflow::liveness::VariableIndex::from_body(body);

    // Run liveness analysis directly (no Salsa query) - PROFILED
    let liveness_start = Instant::now();
    let liveness_result = dataflow::liveness::liveness_analysis_direct(
        body,
        &cfg,
        var_index.clone(), // Clone Arc (cheap - just ref count bump)
    );
    let liveness_result = match liveness_result {
        Some(result) => result,
        None => {
            tracing::warn!("Liveness analysis failed for method: {:?}", method_id);
            return diagnostics;
        }
    };
    LIVENESS_TIME_NS.fetch_add(liveness_start.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Get CFG entry point
    let entry = match cfg.entry_point() {
        Some(e) => e,
        None => {
            tracing::warn!("CFG has no entry point for method: {:?}", method_id);
            return diagnostics;
        }
    };

    // Get live variables at entry point (IN set)
    let live_at_entry = match liveness_result.block_in(entry) {
        Some(live) => live,
        None => {
            tracing::warn!("No liveness data for entry block: {:?}", method_id);
            return diagnostics;
        }
    };

    // Collect all declared variables (explicit VarDecl, For, ForEach) and parameters
    let mut declared_vars = rustc_hash::FxHashSet::default();

    // Add parameters
    for &param_id in body.params.iter() {
        let binding = &body.bindings[param_id];
        declared_vars.insert(binding.name.as_str().to_lowercase());
    }

    // Note: var_index was already built earlier (line 163) - reuse it for lookups

    // Check each declared variable
    // 1. Check VarDecl bindings
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::VarDecl { bindings } = &body.stmts[*stmt_id] {
            for &binding_id in bindings.iter() {
                let binding = &body.bindings[binding_id];
                declared_vars.insert(binding.name.as_str().to_lowercase());

                // Variable is unused if it's not live at entry
                // Fast path: use pre-computed binding index (O(1), no allocation)
                let is_unused = if let Some(idx) = var_index.get_index_by_binding(binding_id) {
                    !live_at_entry.is_live_by_idx(idx)
                } else {
                    // Fallback to string-based check
                    !live_at_entry.is_live(binding.name.as_str())
                };

                if is_unused {
                    // Get source range for the variable name
                    if let Some(range) = source_map.binding_range(binding_id) {
                        diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                    }
                }
            }
        }
    }

    // 2. Check For loop variables
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::For { var, .. } = &body.stmts[*stmt_id] {
            let binding = &body.bindings[*var];
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(*var) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(*var) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                }
            }
        }
    }

    // 3. Check ForEach loop variables
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::ForEach { var, .. } = &body.stmts[*stmt_id] {
            let binding = &body.bindings[*var];
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(*var) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(*var) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                }
            }
        }
    }

    // 4. Check implicit variables (assigned without Перем)
    // These are variables in Assign statements that are not declared
    let mut implicit_vars: rustc_hash::FxHashMap<String, (String, ide_db::TextRange)> =
        rustc_hash::FxHashMap::default();

    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::Assign { target, .. } = &body.stmts[*stmt_id] {
            // Check if target is a simple path (variable assignment)
            if let hir_def::hir::Expr::Path(name) = &body.exprs[*target] {
                let lowercase_name = name.as_str().to_lowercase();

                // Skip if already declared or is a parameter
                if !declared_vars.contains(&lowercase_name) {
                    // This is an implicit variable - save it with its first assignment location
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        implicit_vars.entry(lowercase_name)
                    {
                        if let Some(range) = source_map.expr_range(*target) {
                            e.insert((name.as_str().to_string(), range));
                        }
                    }
                }
            }
        }
    }

    // Check if implicit variables are unused - OPTIMIZED with batch union
    // Instead of O(V × B) iteration, we create a union of all live sets ONCE: O(B)
    // Then check each variable against this union: O(V)
    // Total: O(B + V) instead of O(V × B)
    let implicit_start = Instant::now();
    let implicit_var_count = implicit_vars.len();
    let mut blocks_counted = 0u64;

    // Create union of all live sets ONCE (O(B) - iterate all blocks once)
    let mut all_live_union = fixedbitset::FixedBitSet::with_capacity(var_index.size());
    for (_, in_state, out_state) in liveness_result.blocks() {
        blocks_counted += 1;
        all_live_union.union_with(in_state.live_vars());
        all_live_union.union_with(out_state.live_vars());
    }

    // Check each implicit variable against union (O(V) - one lookup per variable)
    for (lowercase_name, (original_name, range)) in implicit_vars {
        IMPLICIT_VARS_CHECKED.fetch_add(1, Ordering::Relaxed);

        // Fast path: check against pre-computed union (O(1) per variable)
        let is_live_anywhere = if let Some(idx) = var_index.get_index(&lowercase_name) {
            all_live_union.contains(idx)
        } else {
            // Variable not in index - can't be live
            false
        };

        if !is_live_anywhere {
            diagnostics.push(create_diagnostic(&original_name, range));
        }
    }

    BLOCKS_ITERATED.fetch_add(blocks_counted, Ordering::Relaxed);
    let implicit_elapsed = implicit_start.elapsed();
    IMPLICIT_CHECK_TIME_NS.fetch_add(implicit_elapsed.as_nanos() as u64, Ordering::Relaxed);

    // Log if this method's implicit check is slow
    if implicit_elapsed.as_micros() > 100 && implicit_var_count > 0 {
        tracing::debug!(
            implicit_vars = implicit_var_count,
            blocks_per_var = blocks_counted / implicit_var_count.max(1) as u64,
            elapsed_us = implicit_elapsed.as_micros(),
            "Slow implicit var check"
        );
    }

    diagnostics
}

/// Check a single method for unused variables using liveness analysis (OLD - kept for reference).
#[allow(dead_code)]
fn check_method(
    db: &dyn RootDatabase,
    method_id: MethodId,
    _ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    METHODS_ANALYZED.fetch_add(1, Ordering::Relaxed);
    let mut diagnostics = Vec::new();

    // Get method body and source_map
    let module_bodies = db.module_bodies(method_id.module);
    let body = match module_bodies.body(method_id.local_id) {
        Some(b) => b,
        None => return diagnostics,
    };
    let source_map = match module_bodies.source_map(method_id.local_id) {
        Some(sm) => sm,
        None => return diagnostics,
    };

    // Run liveness analysis (cached by Salsa) - PROFILED
    let liveness_start = Instant::now();
    let liveness_result = match db.liveness_analysis(method_id) {
        Some(result) => result,
        None => {
            tracing::warn!("Liveness analysis failed for method: {:?}", method_id);
            return diagnostics;
        }
    };
    LIVENESS_TIME_NS.fetch_add(liveness_start.elapsed().as_nanos() as u64, Ordering::Relaxed);

    // Get CFG to find entry point
    let cfg = db.method_cfg(method_id);
    let entry = match cfg.entry_point() {
        Some(e) => e,
        None => {
            tracing::warn!("CFG has no entry point for method: {:?}", method_id);
            return diagnostics;
        }
    };

    // Get live variables at entry point (IN set)
    let live_at_entry = match liveness_result.block_in(entry) {
        Some(live) => live,
        None => {
            tracing::warn!("No liveness data for entry block: {:?}", method_id);
            return diagnostics;
        }
    };

    // Collect all declared variables (explicit VarDecl, For, ForEach) and parameters
    let mut declared_vars = rustc_hash::FxHashSet::default();

    // Add parameters
    for &param_id in body.params.iter() {
        let binding = &body.bindings[param_id];
        declared_vars.insert(binding.name.as_str().to_lowercase());
    }

    // Get var_index for index-based lookups (O(1), no allocation)
    let var_index = live_at_entry.var_index();

    // Check each declared variable
    // 1. Check VarDecl bindings
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::VarDecl { bindings } = &body.stmts[*stmt_id] {
            for &binding_id in bindings.iter() {
                let binding = &body.bindings[binding_id];
                declared_vars.insert(binding.name.as_str().to_lowercase());

                // Variable is unused if it's not live at entry
                // Fast path: use pre-computed binding index (O(1), no allocation)
                let is_unused = if let Some(idx) = var_index.get_index_by_binding(binding_id) {
                    !live_at_entry.is_live_by_idx(idx)
                } else {
                    // Fallback to string-based check
                    !live_at_entry.is_live(binding.name.as_str())
                };

                if is_unused {
                    // Get source range for the variable name
                    if let Some(range) = source_map.binding_range(binding_id) {
                        diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                    }
                }
            }
        }
    }

    // 2. Check For loop variables
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::For { var, .. } = &body.stmts[*stmt_id] {
            let binding = &body.bindings[*var];
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(*var) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(*var) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                }
            }
        }
    }

    // 3. Check ForEach loop variables
    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::ForEach { var, .. } = &body.stmts[*stmt_id] {
            let binding = &body.bindings[*var];
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(*var) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(*var) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range));
                }
            }
        }
    }

    // 4. Check implicit variables (assigned without Перем)
    // These are variables in Assign statements that are not declared
    let mut implicit_vars: rustc_hash::FxHashMap<String, (String, ide_db::TextRange)> =
        rustc_hash::FxHashMap::default();

    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::Assign { target, .. } = &body.stmts[*stmt_id] {
            // Check if target is a simple path (variable assignment)
            if let hir_def::hir::Expr::Path(name) = &body.exprs[*target] {
                let lowercase_name = name.as_str().to_lowercase();

                // Skip if already declared or is a parameter
                if !declared_vars.contains(&lowercase_name) {
                    // This is an implicit variable - save it with its first assignment location
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        implicit_vars.entry(lowercase_name)
                    {
                        if let Some(range) = source_map.expr_range(*target) {
                            e.insert((name.as_str().to_string(), range));
                        }
                    }
                }
            }
        }
    }

    // Check if implicit variables are unused - OPTIMIZED with batch union
    // Instead of O(V × B) iteration, we create a union of all live sets ONCE: O(B)
    // Then check each variable against this union: O(V)
    // Total: O(B + V) instead of O(V × B)
    let implicit_start = Instant::now();
    let implicit_var_count = implicit_vars.len();
    let mut blocks_counted = 0u64;

    // Create union of all live sets ONCE (O(B) - iterate all blocks once)
    let mut all_live_union = fixedbitset::FixedBitSet::with_capacity(var_index.size());
    for (_, in_state, out_state) in liveness_result.blocks() {
        blocks_counted += 1;
        all_live_union.union_with(in_state.live_vars());
        all_live_union.union_with(out_state.live_vars());
    }

    // Check each implicit variable against union (O(V) - one lookup per variable)
    for (lowercase_name, (original_name, range)) in implicit_vars {
        IMPLICIT_VARS_CHECKED.fetch_add(1, Ordering::Relaxed);

        // Fast path: check against pre-computed union (O(1) per variable)
        let is_live_anywhere = if let Some(idx) = var_index.get_index(&lowercase_name) {
            all_live_union.contains(idx)
        } else {
            // Variable not in index - can't be live
            false
        };

        if !is_live_anywhere {
            diagnostics.push(create_diagnostic(&original_name, range));
        }
    }

    BLOCKS_ITERATED.fetch_add(blocks_counted, Ordering::Relaxed);
    let implicit_elapsed = implicit_start.elapsed();
    IMPLICIT_CHECK_TIME_NS.fetch_add(implicit_elapsed.as_nanos() as u64, Ordering::Relaxed);

    // Log if this method's implicit check is slow
    if implicit_elapsed.as_micros() > 100 && implicit_var_count > 0 {
        tracing::debug!(
            implicit_vars = implicit_var_count,
            blocks_per_var = blocks_counted / implicit_var_count.max(1) as u64,
            elapsed_us = implicit_elapsed.as_micros(),
            "Slow implicit var check"
        );
    }

    diagnostics
}

/// Check module-level code for unused variables.
///
/// Analyzes code outside procedures/functions (module initialization code).
/// Uses liveness analysis to detect variables that are assigned but never read.
fn check_module_level_code(
    db: &dyn RootDatabase,
    module_id: ModuleId,
    _ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get module bodies
    let module_bodies = db.module_bodies(module_id);

    // Get module-level code result (body + source_map)
    let lower_result = match module_bodies.module_code_result() {
        Some(result) => result,
        None => return diagnostics, // No module-level code
    };

    let body = &lower_result.body;
    let source_map = &lower_result.source_map;

    // Run liveness analysis (cached by Salsa)
    let liveness_result = match db.module_level_liveness_analysis(module_id) {
        Some(result) => result,
        None => {
            tracing::warn!("Liveness analysis failed for module-level code: {:?}", module_id);
            return diagnostics;
        }
    };

    // Module-level code typically has implicit variables (no Перем declarations).
    // We need to find all implicit variable assignments.
    let mut implicit_vars: rustc_hash::FxHashMap<String, (String, ide_db::TextRange)> =
        rustc_hash::FxHashMap::default();

    for stmt_id in body.body_stmts.iter() {
        if let hir_def::hir::Stmt::Assign { target, .. } = &body.stmts[*stmt_id] {
            // Check if target is a simple path (variable assignment)
            if let hir_def::hir::Expr::Path(name) = &body.exprs[*target] {
                let lowercase_name = name.as_str().to_lowercase();

                // Save first assignment location for each variable
                if let std::collections::hash_map::Entry::Vacant(e) =
                    implicit_vars.entry(lowercase_name)
                {
                    if let Some(range) = source_map.expr_range(*target) {
                        e.insert((name.as_str().to_string(), range));
                    }
                }
            }
        }
    }

    // Check if implicit variables are unused
    // For implicit variables, check if they're ever live in any block
    for (lowercase_name, (original_name, range)) in implicit_vars {
        // Check if variable is live in any block (either IN or OUT state)
        let is_live_anywhere = liveness_result.blocks().any(|(_, in_state, out_state)| {
            in_state.is_live(&lowercase_name) || out_state.is_live(&lowercase_name)
        });

        if !is_live_anywhere {
            diagnostics.push(create_diagnostic(&original_name, range));
        }
    }

    diagnostics
}

/// Create diagnostic for an unused variable.
fn create_diagnostic(name: &str, range: TextRange) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::UnusedLocalVariable,
        message: format!("Переменная \"{}\" объявлена, но не используется", name),
        severity: Severity::Information,
        range,
        tags: vec![DiagnosticTag::Unnecessary],
        fixes: vec![],
    }
}

/// Creates diagnostic from HIR BodyDiagnostic (legacy path).
///
/// Called from lib.rs dispatch when `BodyDiagnostic::UnusedVariable` is encountered.
/// This is the OLD detection path that will be removed in Phase 5.
/// The NEW detection path is via `check()` function above.
#[allow(dead_code)]
pub fn from_hir(name: &str, range: TextRange, ctx: &DiagnosticsContext) -> Option<Diagnostic> {
    if ctx.config.is_disabled(DiagnosticCode::UnusedLocalVariable) {
        return None;
    }
    Some(create_diagnostic(name, range))
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range, check_hir_diagnostic};
    use crate::DiagnosticCode;

    #[test]
    fn test_unused_var_in_procedure() {
        let code = r#"Процедура Тест()
    Перем НеИспользуется;
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Expected 1 UnusedLocalVariable diagnostic");
        // Check position: variable name "НеИспользуется" on line 1
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 24);
    }

    #[test]
    fn test_used_var_no_diagnostic() {
        let code = r#"Процедура Тест()
    Перем Сообщение;
    Сообщение = "Привет";
    Сообщить(Сообщение);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused loop variable should trigger diagnostic");
        // "Индекс" on line 1, col 8-14 (after "    Для ")
        assert_diagnostic_range(code, unused_diags[0], 1, 8, 14);
    }

    #[test]
    fn test_used_loop_variable() {
        let code = r#"Процедура Тест()
    Для Индекс = 1 По 10 Цикл
        Сообщить(Индекс);
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used loop variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_unused_foreach_variable() {
        let code = r#"Процедура Тест()
    Для Каждого Элемент Из Коллекция Цикл
        Сообщить("Итерация");
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused foreach variable should trigger diagnostic");
        // "Элемент" on line 1, col 16-23 (after "    Для Каждого ")
        assert_diagnostic_range(code, unused_diags[0], 1, 16, 23);
    }

    #[test]
    fn test_multiple_unused_vars() {
        let code = r#"Процедура Тест()
    Перем А, Б, В;
    Сообщить(Б);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // А and В are unused, Б is used
        assert_eq!(unused_diags.len(), 2, "Expected 2 unused variables (А and В)");
        // Check positions: "А" at col 10-11, "В" at col 16-17 on line 1
        assert!(
            unused_diags.iter().any(|d| d.message.contains("А")),
            "Should detect unused variable А"
        );
        assert!(
            unused_diags.iter().any(|d| d.message.contains("В")),
            "Should detect unused variable В"
        );
    }

    #[test]
    fn test_case_insensitive_usage() {
        let code = r#"Процедура Тест()
    Перем Переменная;
    ПЕРЕМЕННАЯ = 10;
    Сообщить(переменная);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Case-insensitive usage should count as used"
        );
    }

    #[test]
    fn test_assigned_but_never_read() {
        // Variable is assigned but its value is never read - should trigger diagnostic
        let code = r#"Процедура Тест()
    Перем ТолькоПрисвоение;
    ТолькоПрисвоение = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Variable assigned but never read should trigger diagnostic"
        );
        // "ТолькоПрисвоение" on line 1, col 10-26 (after "    Перем ")
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 26);
    }

    #[test]
    fn test_assigned_and_read() {
        // Variable is assigned AND read - should NOT trigger
        let code = r#"Процедура Тест()
    Перем Значение;
    Значение = 10;
    Сообщить(Значение);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable that is read should not trigger diagnostic"
        );
    }

    #[test]
    fn test_multiple_assignments_no_read() {
        // Multiple assignments but never read
        let code = r#"Процедура Тест()
    Перем Результат;
    Результат = ПервоеДействие();
    Результат = ВтороеДействие();
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Variable never read should trigger diagnostic");
        // "Результат" on line 1, col 10-19 (after "    Перем ")
        assert_diagnostic_range(code, unused_diags[0], 1, 10, 19);
    }

    #[test]
    fn test_field_assignment_base_is_read() {
        // When assigning to Obj.Field, Obj IS read (we access it)
        let code = r#"Процедура Тест()
    Перем Структура;
    Структура = Новый Структура;
    Структура.Поле = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable used as base for field assignment should count as read"
        );
    }

    #[test]
    fn test_index_assignment_base_is_read() {
        // When assigning to Arr[i], Arr IS read
        let code = r#"Процедура Тест()
    Перем Массив;
    Массив = Новый Массив;
    Массив[0] = 10;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Variable used as base for index assignment should count as read"
        );
    }

    /// Test based on Java fixture content (UnusedLocalVariableDiagnostic.bsl)
    ///
    /// Java test expects 5 diagnostics total:
    /// - Line 1, col 6-36: module variable (NOT YET IMPLEMENTED - module-level)
    /// - Line 19, col 10-35: `ЛокальнаяБезИспользования` - declared but never used ✓
    /// - Line 19, col 37-63: `ТолькоСПрисвоениемЗначения` - assigned but never read ✓
    /// - Line 24, col 4-28: `ВПроцедуреНеИспользуемая` - assigned but never read ✓
    /// - Line 83, col 0-25: module-level code (NOT YET IMPLEMENTED - module-level)
    ///
    /// This test covers only the local variables we currently handle (3 of 5).
    #[test]
    fn test_fixture_local_variables_in_function() {
        // Excerpt from Java fixture: function Вторая()
        let code = r#"Функция Вторая()
    Перем ЛокальнаяБезИспользования, ТолькоСПрисвоениемЗначения, ЛокальнаяСИспользованием;

    ЛокальнаяСИспользованием = 40;
    ТолькоСПрисвоениемЗначения = ВыполнитьДействие(ЛокальнаяСИспользованием);
    ВПроцедуреИспользуемая = Проверка();
    ВПроцедуреНеИспользуемая = Проверка();

    Если ВПроцедуреИспользуемая = Истина Тогда

       ТолькоСПрисвоениемЗначения = 39;

    КонецЕсли;

    ПеременнаяОбъектСИспользованием = Обработки.Проверка.Создать();
    ПеременнаяОбъектСИспользованием.Выполнить();

    ВПроцедуреИспользуемая2 = Новый Файл(ОбъединитьПути(".", "test_versions.mxl"));
    Ожидаем.Что(ВПроцедуреИспользуемая2.Существует(), "Файл отчета не был создан").ЭтоИстина();

КонецФункции"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Expected diagnostics for local variables within this function:
        // 1. ЛокальнаяБезИспользования - declared but never used
        // 2. ТолькоСПрисвоениемЗначения - assigned but never read
        // 3. ВПроцедуреНеИспользуемая - assigned but never read
        //
        // Note: Variables without Перем (implicit) are currently tracked the same way
        assert_eq!(
            unused_diags.len(),
            3,
            "Expected 3 unused local variables, got {}. Diagnostics: {:?}",
            unused_diags.len(),
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Test module-level variable tracking.
    ///
    /// Module-level variables should be flagged if unused, unless exported.
    #[test]
    fn test_module_level_unused_variable() {
        // Module-level variable that is never used
        let code = r#"Перем НеИспользуемая;

Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(unused_diags.len(), 1, "Unused module variable should trigger diagnostic");
        // "НеИспользуемая" on line 0, col 6-20 (after "Перем ")
        assert_diagnostic_range(code, unused_diags[0], 0, 6, 20);
    }

    #[test]
    fn test_module_level_export_variable_not_flagged() {
        // Exported module-level variable should NOT be flagged
        let code = r#"Перем ЭкспортнаяПеременная Экспорт;

Процедура Тест()
    Сообщить("Привет");
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Exported variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_module_level_used_variable() {
        // Module-level variable used in a method should NOT be flagged
        let code = r#"Перем ИспользуемаяПеременная;

Процедура Тест()
    Сообщить(ИспользуемаяПеременная);
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        assert!(
            diagnostics.iter().all(|d| d.code != DiagnosticCode::UnusedLocalVariable),
            "Used module variable should not trigger diagnostic"
        );
    }

    #[test]
    fn test_module_level_code_unused_variable() {
        // Module-level code: variable assigned but never read
        let code = r#"НеИспользуемаяВМодуле = 30;
ИспользуемаяВМодуле = 40;
Сообщить(ИспользуемаяВМодуле);"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Module-level code should detect unused implicit variable"
        );
        // "НеИспользуемаяВМодуле" on line 0, col 0-21
        assert_diagnostic_range(code, unused_diags[0], 0, 0, 21);
    }

    #[test]
    fn test_var_used_in_while_condition() {
        // Variable used in While condition should NOT trigger diagnostic
        // Real-world case from user: loop control variable
        let code = r#"Процедура ЗапускПроцессовДО()
    ЕстьЗадания = Истина;
    Пока ЕстьЗадания Цикл
        ВыполнитьДействие();
        ЕстьЗадания = ПроверитьУсловие();
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            0,
            "Variable used in While condition should not trigger diagnostic, got: {:?}",
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// Full Java fixture test.
    ///
    /// Java test expects 5 diagnostics:
    /// - hasRange(1, 6, 36): Line 1, `ПеременнаяМодуляНеИспользуемая` with `&НаКлиенте`
    /// - hasRange(19, 10, 35): Line 19, `ЛокальнаяБезИспользования`
    /// - hasRange(19, 37, 63): Line 19, `ТолькоСПрисвоениемЗначения`
    /// - hasRange(24, 4, 28): Line 24, `ВПроцедуреНеИспользуемая`
    /// - hasRange(83, 0, 25): Line 83, `ВнеПроцедурНеИспользуемая`
    ///
    /// Note: Java uses 0-indexed lines. Our implementation may differ because:
    /// - We don't handle `&НаКлиенте`/`&НаСервере` annotations (both vars with same name flagged)
    /// - We may detect additional unused variables Java doesn't flag
    #[test]
    fn test_java_fixture_full() {
        let code = include_str!("../../tests/fixtures/UnusedLocalVariableDiagnostic.bsl");

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Print all diagnostics for debugging
        println!("Found {} UnusedLocalVariable diagnostics:", unused_diags.len());
        for (i, diag) in unused_diags.iter().enumerate() {
            println!("  {}: {}", i + 1, diag.message);
        }
        println!("\nAll diagnostics ({}):", diagnostics.len());
        for (i, diag) in diagnostics.iter().enumerate() {
            println!("  {}: [{:?}] {}", i + 1, diag.code, diag.message);
        }

        // Check that we detect the key cases from Java test
        let messages: Vec<&str> = unused_diags.iter().map(|d| d.message.as_str()).collect();

        // These should be detected (matching Java):
        assert!(
            messages.iter().any(|m| m.contains("ЛокальнаяБезИспользования")),
            "Should detect ЛокальнаяБезИспользования"
        );
        assert!(
            messages.iter().any(|m| m.contains("ТолькоСПрисвоениемЗначения")),
            "Should detect ТолькоСПрисвоениемЗначения"
        );
        assert!(
            messages.iter().any(|m| m.contains("ВПроцедуреНеИспользуемая")),
            "Should detect ВПроцедуреНеИспользуемая"
        );
        assert!(
            messages.iter().any(|m| m.contains("ВнеПроцедурНеИспользуемая")),
            "Should detect ВнеПроцедурНеИспользуемая"
        );
        assert!(
            messages.iter().any(|m| m.contains("ПеременнаяМодуляНеИспользуемая")),
            "Should detect ПеременнаяМодуляНеИспользуемая"
        );

        // Java expects exactly 5 diagnostics.
        // Note: ПеременнаяМодуляНеИспользуемая appears twice in fixture:
        // - Line 2: &НаКлиенте Перем ПеременнаяМодуляНеИспользуемая; // Error (first declaration)
        // - Line 5: &НаСервере Перем ПеременнаяМодуляНеИспользуемая; // Ignored (duplicate name)
        //
        // Java ignores duplicate module variable declarations (VariableSymbolComputer.visitModuleVarDeclaration:88-89).
        // We now match this behavior by skipping duplicates in SymbolTreeBuilder.add_variable.
        assert_eq!(unused_diags.len(), 5, "Should detect 5 unused variables (matching Java)");

        // Verify exact positions (matching Java test)
        // Sort diagnostics by line number for consistent comparison
        use crate::test_utils::{assert_diagnostic_range, range_to_line_col};
        let mut sorted_diags = unused_diags.clone();
        sorted_diags.sort_by_key(|d| {
            let (line, col, _, _) = range_to_line_col(code, d.range);
            (line, col)
        });

        // Java expectations (sorted by line):
        // 1. hasRange(1, 6, 36): Line 1, `ПеременнаяМодуляНеИспользуемая` with `&НаКлиенте`
        assert_diagnostic_range(code, sorted_diags[0], 1, 6, 36);

        // 2. hasRange(19, 10, 35): Line 19, `ЛокальнаяБезИспользования`
        assert_diagnostic_range(code, sorted_diags[1], 19, 10, 35);

        // 3. hasRange(19, 37, 63): Line 19, `ТолькоСПрисвоениемЗначения`
        assert_diagnostic_range(code, sorted_diags[2], 19, 37, 63);

        // 4. hasRange(24, 4, 28): Line 24, `ВПроцедуреНеИспользуемая`
        assert_diagnostic_range(code, sorted_diags[3], 24, 4, 28);

        // 5. hasRange(83, 0, 25): Line 83, `ВнеПроцедурНеИспользуемая`
        assert_diagnostic_range(code, sorted_diags[4], 83, 0, 25);
    }

    #[test]
    fn test_foreach_collection_variable_is_used() {
        // Regression test for: implicit variable used as ForEach collection
        // should NOT trigger unused variable diagnostic
        // Real-world example from user report
        let code = r#"Процедура ОжидатьЗавершенияВыполненияЗадания(КлючЗадания)
    Отбор = Новый Структура;
    Отбор.Вставить("Ключ", КлючЗадания);
    НайденныеФоновыеЗадания = ФоновыеЗадания.ПолучитьФоновыеЗадания(Отбор);

    Для Каждого ФоновоеЗадание Из НайденныеФоновыеЗадания Цикл
        Если ФоновоеЗадание.Состояние = СостояниеФоновогоЗадания.Активно
            ИЛИ ФоновоеЗадание.Состояние <> СостояниеФоновогоЗадания.Завершено Тогда
                ФоновоеЗадание.ОжидатьЗавершения();
        КонецЕсли;
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Print diagnostics for debugging
        if !unused_diags.is_empty() {
            println!("Found {} UnusedLocalVariable diagnostics:", unused_diags.len());
            for (i, diag) in unused_diags.iter().enumerate() {
                println!("  {}: {}", i + 1, diag.message);
            }
        }

        // НайденныеФоновыеЗадания is used in ForEach loop - should NOT trigger diagnostic
        assert!(
            !unused_diags.iter().any(|d| d.message.contains("НайденныеФоновыеЗадания")),
            "НайденныеФоновыеЗадания is used in ForEach loop and should not be flagged as unused"
        );

        // All other implicit variables (Отбор, ФоновоеЗадание) are also used
        assert_eq!(unused_diags.len(), 0, "No variables should be flagged as unused in this code");
    }
}
