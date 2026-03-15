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

use rustc_hash::FxHashSet;

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{BindingId, IdConversion, ModuleId};
use ide_db::{RootDatabase, TextRange};

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::CodeSmell,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[
        bsl_metadata::ModuleType::CommandModule,
        bsl_metadata::ModuleType::CommonModule,
        bsl_metadata::ModuleType::ManagerModule,
        bsl_metadata::ModuleType::ValueManagerModule,
        bsl_metadata::ModuleType::SessionModule,
        bsl_metadata::ModuleType::Unknown,
    ],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Brainoverload, MetadataTag::Badpractice, MetadataTag::Unused],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Build a set of lowercase attribute names that should not be flagged as unused.
///
/// In ObjectModule, assignments to names like `Дата`, `Номер`, `Автор` set object
/// attributes, not local variables. In FormModule, assignments to form attribute names
/// (e.g. `Замечание = Параметры.Замечание`) write to form attributes displayed in UI.
/// This function collects all such names so the diagnostic can skip them.
fn build_attribute_names_to_skip(ctx: &DiagnosticsContext) -> FxHashSet<String> {
    let metadata = ctx.module_metadata();

    match metadata.module_type {
        bsl_metadata::ModuleType::ObjectModule => {
            let mdo = match &metadata.mdo {
                Some(mdo) => mdo,
                None => return FxHashSet::default(),
            };

            let mut names = FxHashSet::default();

            for attr in &mdo.attributes {
                names.insert(attr.name.to_lowercase());
                if let Some(ref en) = attr.name_en {
                    names.insert(en.to_lowercase());
                }
            }

            for ts in &mdo.tabular_sections {
                names.insert(ts.name().to_lowercase());
                if let Some(en) = ts.name_en() {
                    names.insert(en.to_lowercase());
                }
            }

            names
        }
        bsl_metadata::ModuleType::FormModule => {
            // Standard managed form properties accessible as variables in form module.
            // These are built-in properties of every managed form (ФормаКлиентскогоПриложения).
            const STANDARD_FORM_PROPERTIES: &[&str] = &[
                // Заголовок / Title
                "заголовок",
                "title",
                // АвтоЗаголовок / AutoTitle
                "автозаголовок",
                "autotitle",
                // Модифицированность / Modified
                "модифицированность",
                "modified",
                // ТолькоПросмотр / ReadOnly
                "толькопросмотр",
                "readonly",
                // КлючСохраненияПоложенияОкна / WindowOptionsKey
                "ключсохраненияположенияокна",
                "windowoptionskey",
                // КлючНазначенияИспользования / PurposeUseKey
                "ключназначенияиспользования",
                "purposeusekey",
            ];

            let mut names: FxHashSet<String> =
                STANDARD_FORM_PROPERTIES.iter().map(|s| (*s).to_string()).collect();

            if let Some(form) = &metadata.form {
                for attr_name in form.attributes() {
                    names.insert(attr_name.to_lowercase());
                }
            }

            names
        }
        _ => FxHashSet::default(),
    }
}

/// Collect UnusedLocalVariable diagnostics using liveness analysis.
///
/// This function performs dataflow-based detection of unused local variables:
/// 1. Loads module-level liveness analysis ONCE (batch processed via Salsa)
/// 2. Iterates over all methods and performs cheap HashMap lookups
/// 3. Checks which declared variables are never live
/// 4. Generates diagnostics for unused variables
///
/// **Performance:**
/// - Module-level batch processing: all methods analyzed in one pass
/// - CFG shared across multiple analyses (built once per module)
/// - Expected 3-5x speedup vs per-method queries
pub fn check(ctx: &DiagnosticsContext) -> Vec<Diagnostic> {
    let code = DiagnosticCode::UnusedLocalVariable;

    if ctx.is_disabled_with_metadata(code) {
        return vec![];
    }

    let mut diagnostics = Vec::new();

    // Build object attribute names set once for the entire module
    let skip_attr_names = build_attribute_names_to_skip(ctx);

    // Load module_bodies ONCE for the entire file
    let module_bodies = ctx.module_bodies();

    // Load module-level liveness ONCE (Salsa cached, batch processed)
    let module_liveness = ctx.module_liveness();
    let module_cfgs = ctx.module_cfgs();

    // Check each method for unused local variables
    // Use module-level liveness results (cheap HashMap lookups)
    for (local_id, body) in module_bodies.iter_bodies() {
        diagnostics.extend(check_method_with_module_liveness(
            local_id,
            body,
            &module_bodies,
            &module_liveness,
            &module_cfgs,
            code,
            ctx,
            &skip_attr_names,
        ));
    }

    // Check module-level code for unused variables
    // FIXME: Skip in streaming mode until module_level_liveness_analysis is migrated to provider
    if ctx.provider.is_none() {
        let module_id = hir::ModuleId::new(ctx.file_id);
        diagnostics.extend(check_module_level_code(ctx.db, module_id, code, ctx, &skip_attr_names));
    }

    // Check explicit module-level variable declarations (Перем X;)
    // that are not referenced by any method body or module-level code
    diagnostics.extend(check_module_var_declarations(&module_bodies, code, ctx));

    diagnostics
}

/// Check a single method for unused variables using module-level liveness (optimized).
///
/// This function uses pre-loaded module-level liveness and CFG results (batch processed
/// via Salsa), avoiding direct construction and leveraging Salsa caching.
#[allow(clippy::too_many_arguments)] // skip_attr_names needed to filter ObjectModule attributes
fn check_method_with_module_liveness(
    local_id: u32,
    body: &hir::Body,
    module_bodies: &hir::ModuleBodies,
    module_liveness: &hir::dataflow::liveness::ModuleLiveness,
    module_cfgs: &hir::cfg::ModuleCfgs,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    skip_attr_names: &FxHashSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get source_map
    let source_map = match module_bodies.source_map(local_id) {
        Some(sm) => sm,
        None => return diagnostics,
    };

    // Get CFG from module-level collection (cheap HashMap lookup)
    let cfg = match module_cfgs.get(local_id) {
        Some(cfg) => cfg,
        None => {
            tracing::warn!("No CFG found for method: local_id={}", local_id);
            return diagnostics;
        }
    };

    // Get liveness result from module-level collection (cheap HashMap lookup)
    let liveness_result = match module_liveness.get(local_id) {
        Some(result) => result,
        None => {
            tracing::warn!("Liveness analysis failed for method: local_id={}", local_id);
            return diagnostics;
        }
    };

    // Get CFG entry point
    let entry = match cfg.entry_point() {
        Some(e) => e,
        None => {
            tracing::warn!("CFG has no entry point for method: local_id={}", local_id);
            return diagnostics;
        }
    };

    // Get live variables at entry point (IN set)
    let live_at_entry = match liveness_result.block_in(entry) {
        Some(live) => live,
        None => {
            tracing::warn!("No liveness data for entry block: local_id={}", local_id);
            return diagnostics;
        }
    };

    // Collect all declared variables (explicit VarDecl, For, ForEach) and parameters
    let mut declared_vars = rustc_hash::FxHashSet::default();

    // Add parameters
    for param_id in body.params() {
        let binding = body.binding(param_id);
        declared_vars.insert(binding.name.as_str().to_lowercase());
    }

    // Get var_index from liveness result (already computed during liveness analysis)
    let var_index = live_at_entry.var_index();

    // Check each declared variable
    // 1. Check VarDecl bindings
    for stmt_id in body.body_stmts() {
        if let hir::Stmt::VarDecl { bindings } = body.stmt(stmt_id) {
            for &binding_id in bindings.iter() {
                let binding_id_opaque = BindingId::from_idx(binding_id);
                let binding = body.binding(binding_id_opaque);
                declared_vars.insert(binding.name.as_str().to_lowercase());

                // Variable is unused if it's not live at entry
                // Fast path: use pre-computed binding index (O(1), no allocation)
                let is_unused = if let Some(idx) = var_index.get_index_by_binding(binding_id_opaque)
                {
                    !live_at_entry.is_live_by_idx(idx)
                } else {
                    // Fallback to string-based check
                    !live_at_entry.is_live(binding.name.as_str())
                };

                if is_unused {
                    // Get source range for the variable name
                    if let Some(range) = source_map.binding_range(binding_id_opaque) {
                        diagnostics.push(create_diagnostic(
                            binding.name.as_str(),
                            range,
                            code,
                            ctx,
                        ));
                    }
                }
            }
        }
    }

    // 2. Check For loop variables
    for stmt_id in body.body_stmts() {
        if let hir::Stmt::For { var, .. } = body.stmt(stmt_id) {
            let var_opaque = BindingId::from_idx(*var);
            let binding = body.binding(var_opaque);
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(var_opaque) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(var_opaque) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range, code, ctx));
                }
            }
        }
    }

    // 3. Check ForEach loop variables
    for stmt_id in body.body_stmts() {
        if let hir::Stmt::ForEach { var, .. } = body.stmt(stmt_id) {
            let var_opaque = BindingId::from_idx(*var);
            let binding = body.binding(var_opaque);
            declared_vars.insert(binding.name.as_str().to_lowercase());

            // Fast path: use pre-computed binding index (O(1), no allocation)
            let is_unused = if let Some(idx) = var_index.get_index_by_binding(var_opaque) {
                !live_at_entry.is_live_by_idx(idx)
            } else {
                !live_at_entry.is_live(binding.name.as_str())
            };

            if is_unused {
                if let Some(range) = source_map.binding_range(var_opaque) {
                    diagnostics.push(create_diagnostic(binding.name.as_str(), range, code, ctx));
                }
            }
        }
    }

    // 4. Check implicit variables (assigned without Перем)
    // These are variables in Assign statements that are not declared
    let mut implicit_vars: rustc_hash::FxHashMap<String, (String, ide_db::TextRange)> =
        rustc_hash::FxHashMap::default();

    for stmt_id in body.body_stmts() {
        if let hir::Stmt::Assign { target, .. } = body.stmt(stmt_id) {
            // Check if target is a simple path (variable assignment)
            let target_opaque = hir::ExprId::from_idx(*target);
            if let hir::Expr::Path(name) = body.expr(target_opaque) {
                let lowercase_name = name.as_str().to_lowercase();

                // Skip if already declared, is a parameter, or is an object attribute
                if !declared_vars.contains(&lowercase_name)
                    && !skip_attr_names.contains(&lowercase_name)
                {
                    // This is an implicit variable - save it with its first assignment location
                    if let std::collections::hash_map::Entry::Vacant(e) =
                        implicit_vars.entry(lowercase_name)
                    {
                        if let Some(range) = source_map.expr_range(target_opaque) {
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

    // Create union of all live sets ONCE (O(B) - iterate all blocks once)
    let mut all_live_union = fixedbitset::FixedBitSet::with_capacity(var_index.size());
    for (_, in_state, out_state) in liveness_result.blocks() {
        all_live_union.union_with(in_state.live_vars());
        all_live_union.union_with(out_state.live_vars());
    }

    // Check each implicit variable against union (O(V) - one lookup per variable)
    for (lowercase_name, (original_name, range)) in implicit_vars {
        // Fast path: check against pre-computed union (O(1) per variable)
        let is_live_anywhere = if let Some(idx) = var_index.get_index(&lowercase_name) {
            all_live_union.contains(idx)
        } else {
            // Variable not in index - can't be live
            false
        };

        if !is_live_anywhere {
            diagnostics.push(create_diagnostic(&original_name, range, code, ctx));
        }
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
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
    skip_attr_names: &FxHashSet<String>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Get module bodies via ctx (works in both LSP and streaming mode)
    let module_bodies = ctx.module_bodies();

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

    for stmt_id in body.body_stmts() {
        if let hir::Stmt::Assign { target, .. } = body.stmt(stmt_id) {
            // Check if target is a simple path (variable assignment)
            let target_opaque = hir::ExprId::from_idx(*target);
            if let hir::Expr::Path(name) = body.expr(target_opaque) {
                let lowercase_name = name.as_str().to_lowercase();

                // Skip object attributes (in ObjectModule, these set object fields)
                if skip_attr_names.contains(&lowercase_name) {
                    continue;
                }

                // Save first assignment location for each variable
                if let std::collections::hash_map::Entry::Vacant(e) =
                    implicit_vars.entry(lowercase_name)
                {
                    if let Some(range) = source_map.expr_range(target_opaque) {
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
            diagnostics.push(create_diagnostic(&original_name, range, code, ctx));
        }
    }

    diagnostics
}

/// Check explicit module-level variable declarations (Перем X;).
///
/// Detects `Перем` declarations that are not referenced by any method body
/// or module-level code. Exported variables are skipped.
fn check_module_var_declarations(
    module_bodies: &hir::ModuleBodies,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Vec<Diagnostic> {
    let module_vars = module_bodies.module_vars();
    if module_vars.is_empty() {
        return Vec::new();
    }

    let mut all_referenced_externals: rustc_hash::FxHashSet<String> =
        rustc_hash::FxHashSet::default();

    for (_local_id, lower_result) in module_bodies.iter_lower_results() {
        all_referenced_externals.extend(lower_result.referenced_externals.iter().cloned());
    }
    if let Some(module_code_result) = module_bodies.module_code_result() {
        all_referenced_externals.extend(module_code_result.referenced_externals.iter().cloned());
    }

    let mut diagnostics = Vec::new();
    for var in module_vars {
        if var.is_export {
            continue;
        }
        let key = var.name.to_lowercase();
        if !all_referenced_externals.contains(&key) {
            diagnostics.push(create_diagnostic(&var.name, var.range, code, ctx));
        }
    }

    diagnostics
}

/// Create diagnostic for an unused variable.
fn create_diagnostic(
    name: &str,
    range: TextRange,
    code: DiagnosticCode,
    ctx: &DiagnosticsContext,
) -> Diagnostic {
    Diagnostic {
        code,
        message: format!("Удалите неиспользуемую переменную {}", name),
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    }
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

    /// Test based on test fixture content (UnusedLocalVariableDiagnostic.bsl)
    ///
    /// Expected 5 diagnostics total:
    /// - Line 1, col 6-36: module variable (NOT YET IMPLEMENTED - module-level)
    /// - Line 19, col 10-35: `ЛокальнаяБезИспользования` - declared but never used ✓
    /// - Line 19, col 37-63: `ТолькоСПрисвоениемЗначения` - assigned but never read ✓
    /// - Line 24, col 4-28: `ВПроцедуреНеИспользуемая` - assigned but never read ✓
    /// - Line 83, col 0-25: module-level code (NOT YET IMPLEMENTED - module-level)
    ///
    /// This test covers only the local variables we currently handle (3 of 5).
    #[test]
    fn test_fixture_local_variables_in_function() {
        // Excerpt from test fixture: function Вторая()
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

    /// Full test fixture test.
    ///
    /// Expected 5 diagnostics:
    /// - hasRange(1, 6, 36): Line 1, `ПеременнаяМодуляНеИспользуемая` with `&НаКлиенте`
    /// - hasRange(19, 10, 35): Line 19, `ЛокальнаяБезИспользования`
    /// - hasRange(19, 37, 63): Line 19, `ТолькоСПрисвоениемЗначения`
    /// - hasRange(24, 4, 28): Line 24, `ВПроцедуреНеИспользуемая`
    /// - hasRange(83, 0, 25): Line 83, `ВнеПроцедурНеИспользуемая`
    ///
    /// Note: Uses 0-indexed lines. Implementation may differ because:
    /// - We don't handle `&НаКлиенте`/`&НаСервере` annotations (both vars with same name flagged)
    /// - We may detect additional unused variables
    #[test]
    fn test_java_fixture_full() {
        let code = r#"&НаКлиенте
Перем ПеременнаяМодуляНеИспользуемая; // Тут ошибка

&НаСервере
Перем ПеременнаяМодуляНеИспользуемая; // Тут без ошибок

Перем ПеременнаяМодуляНеИспользуемаяЭкспортная Экспорт; // Тут думаю ошибка не нужно, возможно ради поддержания интефейса
Перем ПеременнаяМодуляИспользуемая; // Тут без ошибок
Перем ПеременнаяМодуляИспользуемаяЭкспортная Экспорт; // Тут без ошибок

Функция Первая()

    ПеременнаяМодуляИспользуемая = ДействиеСРезультатомЧисло();
    ДействиеСПараметром(ПеременнаяМодуляИспользуемая);
    ДействиеСПараметром2(ПеременнаяМодуляИспользуемаяЭкспортная);

КонецФункции

Функция Вторая()
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

КонецФункции

Функция Третья(ЭтоПараметр)

    ЭтоПараметр = Новый Массив();

    НоваяСтрока                = ГруппаДоступа.ВидыДоступа.Добавить();
    НоваяСтрока.ВидДоступа     = СтрокаВидаДоступа.ВидДоступа;
    НоваяСтрока.ДоступРазрешен = СтрокаВидаДоступа.ДоступРазрешен;

КонецФункции

Процедура ЗаполнитьСвойстваОбъектаОбъектнойМоделиCOMАдминистратораПоОписанию(Объект, Знач Описание, Знач Словарь)

	Для Каждого ФрагментСловаря Из Словарь Цикл

		ИмяСвойства = ФрагментСловаря.Значение;

		ЗначениеСвойства = Описание[ФрагментСловаря.Ключ];

		Объект[ИмяСвойства] = ЗначениеСвойства;

	КонецЦикла;

КонецПроцедуры

Процедура ВывестиШапкуПоВерсии(ТЧОтчета, Знач Текст, Знач НомерСтроки, Знач НомерКолонки)

	Если Не ПустаяСтрока(Текст) Тогда

		ТЧОтчета.Область("C"+Строка(НомерКолонки)).ШиринаКолонки = 50;

		Регион = "R" + Формат(НомерСтроки, "ЧГ=0") + "C" + Формат(НомерКолонки, "ЧГ=0");
		ТЧОтчета.Область(Регион).Текст = Текст;
		ТЧОтчета.Область(Регион).ЦветФона = ЦветаСтиля.ТекстЗапрещеннойЯчейкиЦвет;
		ТЧОтчета.Область(Регион).Шрифт = Новый Шрифт(, 8, Истина, , , );
		ТЧОтчета.Область(Регион).ГраницаСверху = Новый Линия(ТипЛинииЯчейкиТабличногоДокумента.Сплошная);
		ТЧОтчета.Область(Регион).ГраницаСнизу  = Новый Линия(ТипЛинииЯчейкиТабличногоДокумента.Сплошная);
		ТЧОтчета.Область(Регион).ГраницаСлева  = Новый Линия(ТипЛинииЯчейкиТабличногоДокумента.Сплошная);
		ТЧОтчета.Область(Регион).ГраницаСправа = Новый Линия(ТипЛинииЯчейкиТабличногоДокумента.Сплошная);

	КонецЕсли;

КонецПроцедуры

ВнеПроцедурНеИспользуемая = 30;
ВнеПроцедурИспользуемая = 40;
ДействиеСПараметром(ВнеПроцедурИспользуемая);

Комиссия = Источник.Комиссия;

Если Истина Тогда

    Комментарий = "Тест1" + Комиссия;

Иначе

    Комментарий = "Тест2" + Комиссия;

КонецЕсли;

Сообщить(Комментарий);
"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        // Check that we detect the key cases from reference test
        let messages: Vec<&str> = unused_diags.iter().map(|d| d.message.as_str()).collect();

        // These should be detected :
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

        // Expected exactly 5 diagnostics.
        // Note: ПеременнаяМодуляНеИспользуемая appears twice in fixture:
        // - Line 2: &НаКлиенте Перем ПеременнаяМодуляНеИспользуемая; // Error (first declaration)
        // - Line 5: &НаСервере Перем ПеременнаяМодуляНеИспользуемая; // Ignored (duplicate name)
        //
        // Duplicate module variable declarations are skipped (handled in SymbolTreeBuilder.add_variable).
        assert_eq!(unused_diags.len(), 5, "Should detect 5 unused variables ");

        // Verify exact positions (matching reference test)
        // Sort diagnostics by line number for consistent comparison
        use crate::test_utils::{assert_diagnostic_range, range_to_line_col};
        let mut sorted_diags = unused_diags.clone();
        sorted_diags.sort_by_key(|d| {
            let (line, col, _, _) = range_to_line_col(code, d.range);
            (line, col)
        });

        // Expected values (sorted by line):
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

        // НайденныеФоновыеЗадания is used in ForEach loop - should NOT trigger diagnostic
        assert!(
            !unused_diags.iter().any(|d| d.message.contains("НайденныеФоновыеЗадания")),
            "НайденныеФоновыеЗадания is used in ForEach loop and should not be flagged as unused"
        );

        // All other implicit variables (Отбор, ФоновоеЗадание) are also used
        assert_eq!(unused_diags.len(), 0, "No variables should be flagged as unused in this code");
    }

    #[test]
    fn test_for_loop_bound_variable_is_used() {
        // Regression test: variable used as For loop upper bound
        // should NOT trigger unused variable diagnostic
        let code = r#"Процедура Тест()
    КоличествоКолонок = 4;
    Для Сч = 1 По КоличествоКолонок Цикл
        Сообщить(Сч);
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert!(
            !unused_diags.iter().any(|d| d.message.contains("КоличествоКолонок")),
            "КоличествоКолонок is used in For loop bound and should not be flagged as unused"
        );
    }

    #[test]
    fn test_for_loop_from_bound_variable_is_used() {
        // Variable used as For loop lower bound (from)
        let code = r#"Процедура Тест()
    Начало = 1;
    Для Сч = Начало По 10 Цикл
        Сообщить(Сч);
    КонецЦикла;
КонецПроцедуры"#;

        let diagnostics = check_hir_diagnostic(code);
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert!(
            !unused_diags.iter().any(|d| d.message.contains("Начало")),
            "Начало is used in For loop from-bound and should not be flagged as unused"
        );
    }

    /// Helper to create ObjectModule metadata with given MDO.
    fn make_object_module_metadata(mdo: bsl_metadata::MetadataObject) -> hir::ModuleMetadata {
        hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::ObjectModule,
            execution_context: None,
            common_module: None,
            mdo: Some(std::sync::Arc::new(mdo)),
            register: None,
            http_service: None,
            web_service: None,
            form: None,
        }
    }

    #[test]
    fn test_object_attribute_not_flagged_in_object_module() {
        use crate::test_utils::check_metadata_diagnostic;

        let mut mdo =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::BusinessProcess, "Исполнение");
        mdo.add_attribute(bsl_metadata::Attribute {
            name: "Дата".to_string(),
            name_en: Some("Date".to_string()),
            attr_type: bsl_metadata::AttributeType::DateTime,
        });
        mdo.add_attribute(bsl_metadata::Attribute {
            name: "Автор".to_string(),
            name_en: None,
            attr_type: bsl_metadata::AttributeType::Unknown,
        });

        let metadata = make_object_module_metadata(mdo);

        let code = r#"Процедура ПриЗаписи(Отказ)
    Дата = ТекущаяДатаСеанса();
    Автор = ПользователиИнформационнойБазы.ТекущийПользователь();
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            0,
            "Object attributes should not be flagged as unused in ObjectModule, got: {:?}",
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_tabular_section_not_flagged_in_object_module() {
        use crate::test_utils::check_metadata_diagnostic;

        let mut mdo = bsl_metadata::MetadataObject::new(
            bsl_metadata::MdoType::Document,
            "ПриходнаяНакладная",
        );
        let ts = bsl_metadata::TabularSection::new(uuid::Uuid::nil(), "Товары");
        mdo.add_tabular_section(ts);

        let metadata = make_object_module_metadata(mdo);

        let code = r#"Процедура ПриЗаписи(Отказ)
    Товары = ЭтотОбъект.Товары.Выгрузить();
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            0,
            "Tabular section name should not be flagged in ObjectModule, got: {:?}",
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_true_unused_still_flagged_in_object_module() {
        use crate::test_utils::check_metadata_diagnostic;

        let mdo =
            bsl_metadata::MetadataObject::new(bsl_metadata::MdoType::BusinessProcess, "Исполнение");

        let metadata = make_object_module_metadata(mdo);

        let code = r#"Процедура ПриЗаписи(Отказ)
    НеАтрибутОбъекта = 42;
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "True unused variable should still be flagged in ObjectModule"
        );
        assert!(unused_diags[0].message.contains("НеАтрибутОбъекта"));
    }

    /// Helper to create FormModule metadata with given form attributes.
    fn make_form_module_metadata(attribute_names: Vec<&str>) -> hir::ModuleMetadata {
        let mut form = bsl_metadata::Form::new(
            "ТестоваяФорма".to_string(),
            bsl_metadata::FormType::Managed,
            uuid::Uuid::nil(),
        );
        form.attributes = attribute_names.into_iter().map(|s| s.to_string()).collect();

        hir::ModuleMetadata {
            module_type: bsl_metadata::ModuleType::FormModule,
            execution_context: None,
            common_module: None,
            mdo: None,
            register: None,
            http_service: None,
            web_service: None,
            form: Some(std::sync::Arc::new(form)),
        }
    }

    #[test]
    fn test_form_attribute_not_flagged_in_form_module() {
        use crate::test_utils::check_metadata_diagnostic;

        let metadata =
            make_form_module_metadata(vec!["Замечание", "ТекущееОписание", "ИсправленноеОписание"]);

        let code = r#"&НаСервере
Процедура ПриСозданииНаСервере(Отказ, СтандартнаяОбработка)
    Замечание = Параметры.Замечание;
    ТекущееОписание = Параметры.ТекущееОписание;
    ИсправленноеОписание = Параметры.Предложение;
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            0,
            "Form attributes should not be flagged as unused in FormModule, got: {:?}",
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_standard_form_property_not_flagged_in_form_module() {
        use crate::test_utils::check_metadata_diagnostic;

        // Form with no custom attributes — only standard properties should be skipped
        let metadata = make_form_module_metadata(vec![]);

        let code = r#"&НаСервере
Процедура ПриСозданииНаСервере(Отказ, СтандартнаяОбработка)
    Заголовок = "Проверка описания — строка " + Параметры.НомерСтроки;
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            0,
            "Standard form property 'Заголовок' should not be flagged as unused in FormModule, got: {:?}",
            unused_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_true_unused_still_flagged_in_form_module() {
        use crate::test_utils::check_metadata_diagnostic;

        let metadata = make_form_module_metadata(vec!["Замечание"]);

        let code = r#"&НаСервере
Процедура ПриСозданииНаСервере(Отказ, СтандартнаяОбработка)
    Замечание = Параметры.Замечание;
    НеРеквизитФормы = 42;
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "True unused variable should still be flagged in FormModule"
        );
        assert!(unused_diags[0].message.contains("НеРеквизитФормы"));
    }

    #[test]
    fn test_form_attribute_still_flagged_in_common_module() {
        use crate::test_utils::{check_metadata_diagnostic, make_non_common_module_metadata};

        let metadata = make_non_common_module_metadata(bsl_metadata::ModuleType::CommonModule);

        let code = r#"Процедура Тест()
    Замечание = "тест";
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Form attribute name should be flagged in CommonModule (not a form context)"
        );
        assert!(unused_diags[0].message.contains("Замечание"));
    }

    #[test]
    fn test_attribute_name_still_flagged_in_common_module() {
        use crate::test_utils::{check_metadata_diagnostic, make_non_common_module_metadata};

        let metadata = make_non_common_module_metadata(bsl_metadata::ModuleType::CommonModule);

        let code = r#"Процедура Тест()
    Дата = ТекущаяДатаСеанса();
КонецПроцедуры"#;

        let diagnostics = check_metadata_diagnostic(metadata, code, |_meta, ctx| super::check(ctx));
        let unused_diags: Vec<_> =
            diagnostics.iter().filter(|d| d.code == DiagnosticCode::UnusedLocalVariable).collect();

        assert_eq!(
            unused_diags.len(),
            1,
            "Same name should be flagged in CommonModule (not an object attribute)"
        );
        assert!(unused_diags[0].message.contains("Дата"));
    }
}
