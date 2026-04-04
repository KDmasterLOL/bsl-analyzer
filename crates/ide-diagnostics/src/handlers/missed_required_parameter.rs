//! MissedRequiredParameter diagnostic.
//!
//! Detects missing required parameters in method calls.
//!
//! ## Why?
//! BSL (1C:Enterprise) allows omitting parameters in method calls, using commas to skip them.
//! However, parameters without default values are required and must be provided.
//!
//! ## Bad practice
//! ```bsl
//! Функция Сложение(Левый, Правый) Экспорт
//!     Возврат Левый + Правый;
//! КонецФункции
//!
//! Результат = Сложение(, 2);      // ERROR: Missing required parameter 'Левый'
//! Результат = Сложение(5);        // ERROR: Missing required parameter 'Правый'
//! Результат = Сложение();         // ERROR: Missing 'Левый', 'Правый'
//! ```
//!
//! ## Good practice
//! ```bsl
//! Результат = Сложение(5, 2);     // OK: All required parameters provided
//!
//! // With optional parameters:
//! Функция Инкремент(Значение, Приращение = 1)  // Приращение is optional
//!     Возврат Значение + Приращение;
//! КонецФункции
//!
//! Результат = Инкремент(5);       // OK: Optional parameter can be omitted
//! ```
//!
//! ## Configuration
//! - **Enabled by default:** Yes
//! - **Severity:** ERROR (MAJOR)
//! - **Tags:** ERROR
//! - **Minutes to fix:** 1
//! - **No configurable parameters**
//!
//! ## Reference
//! Ported from:
//! - Adapted to use Rowan SyntaxNode and SymbolTree
//!
//! ## HIR-based implementation
//!
//! This diagnostic is now collected during HIR lowering (AST→HIR conversion).
//! The `from_hir()` function validates the call and creates the final diagnostic.
//! This approach is faster because:
//! 1. No separate AST traversal for this diagnostic
//! 2. SymbolTree lookups are Salsa-cached
//! 3. Method resolution happens once during lowering

use crate::define_metadata;
use crate::metadata::*;
use crate::{Diagnostic, DiagnosticCode, DiagnosticsContext};
use hir::{MethodSymbol, ModuleId, Name};
use ide_db::TextRange;
use vfs::FileId;

pub const METADATA: DiagnosticMetadata = define_metadata! {
    diagnostic_type: DiagnosticType::Error,
    severity: DiagnosticSeverityLevel::Major,
    scope: DiagnosticScope::All,
    modules: &[],
    minutes_to_fix: 1,
    activated_by_default: true,
    compatibility_mode: DiagnosticCompatibilityMode::Undefined,
    tags: &[MetadataTag::Error],
    can_locate_on_project: false,
    extra_min_for_complexity: 0.0,
    lsp_severity_override: "",
};

/// Creates diagnostic from HIR BodyDiagnostic.
///
/// Called from lib.rs dispatch when `BodyDiagnostic::MissedRequiredParameter` is encountered.
///
/// This function validates a method call against its definition:
/// 1. Resolves the method using SymbolTree (local, CommonModule, or ManagerModule)
/// 2. Checks which required parameters are missing
/// 3. Returns diagnostic if any required parameters are not provided
///
/// ## Parameters
/// - `callee`: Method name being called
/// - `module`: Optional module name for two-level calls (Module.Method)
/// - `mdo_type`: Optional MDO type keyword for three-level calls (Документы, Справочники)
/// - `mdo_name`: Optional MDO name for three-level calls (ПКО, Товары)
/// - `args`: Boolean array indicating which arguments have values
/// - `range`: Source range for the diagnostic
/// - `ctx`: Diagnostics context with database access
///
/// ## Call patterns
/// - Local: `Method()` → module=None, mdo_type=None
/// - Two-level: `CommonModule.Method()` → module=Some, mdo_type=None
/// - Three-level: `Документы.ПКО.Method()` → module=None, mdo_type=Some, mdo_name=Some
/// - ThisObject: `ЭтотОбъект.Method()` → module=Some("ЭтотОбъект")
pub fn from_hir(
    callee: &str,
    module: Option<&str>,
    mdo_type: Option<&str>,
    mdo_name: Option<&str>,
    args: &[bool],
    range: TextRange,
    ctx: &DiagnosticsContext,
) -> Option<Diagnostic> {
    let code = DiagnosticCode::MissedRequiredParameter;

    if ctx.is_disabled_with_metadata(code) {
        return None;
    }

    // Resolve and check missing parameters based on call type
    let missing = if let (Some(mdo_type_kw), Some(mdo_obj_name)) = (mdo_type, mdo_name) {
        // Three-level call: Документы.ПКО.Method()
        tracing::debug!(
            mdo_type = mdo_type_kw,
            mdo_name = mdo_obj_name,
            callee,
            "Processing three-level call in from_hir"
        );
        check_manager_module_call(ctx, mdo_type_kw, mdo_obj_name, callee, args)?
    } else if let Some(module_name) = module {
        // Two-level call: Module.Method() or ЭтотОбъект.Method()
        check_qualified_call(ctx, module_name, callee, args)?
    } else {
        // Local call: Method()
        check_local_call(ctx, callee, args)?
    };

    if missing.is_empty() {
        return None;
    }

    // Create diagnostic message
    let param_list =
        missing.iter().map(|name| format!("'{}'", name)).collect::<Vec<_>>().join(", ");
    let message = format!("Укажите обязательный параметр {}", param_list);

    Some(Diagnostic {
        code,
        message,
        severity: ctx.severity(code),
        range,
        tags: ctx.tags(code),
        fixes: vec![],
    })
}

/// Check local method call for missing required parameters.
///
/// Returns Some(missing_params) if method is found, None if method doesn't exist.
fn check_local_call(
    ctx: &DiagnosticsContext,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    let symbol_tree = ctx.symbol_tree();
    let name = Name::new(method_name);

    let method = symbol_tree.find_method(&name)?;
    Some(check_missing_params(method, args))
}

/// Check qualified method call (Module.Method) for missing required parameters.
///
/// Returns Some(missing_params) if method is found and exported, None otherwise.
fn check_qualified_call(
    ctx: &DiagnosticsContext,
    module_name: &str,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    // Load metadata
    let configuration = ctx.load_configuration()?;

    // Find CommonModule in metadata (case-insensitive)
    let common_module = configuration.find_common_module(module_name)?;

    // Resolve CommonModule file
    let module_file_id = ctx.find_common_module_file(common_module)?;

    // Build SymbolTree
    let module_id = ModuleId::new(module_file_id);
    let symbol_tree = ctx.symbol_tree_for(module_id);

    // Lookup method
    let name = Name::new(method_name);
    let method = symbol_tree.find_method(&name)?;

    // Only check exported methods for qualified calls
    if !method.is_export {
        return None;
    }

    Some(check_missing_params(method, args))
}

/// Check three-level method call (MdoType.MdoName.Method) for missing required parameters.
///
/// Handles calls like `Документы.ПКО.Method()` or `Catalogs.Товары.Method()`.
///
/// Returns Some(missing_params) if method is found and exported, None otherwise.
fn check_manager_module_call(
    ctx: &DiagnosticsContext,
    mdo_type_keyword: &str,
    mdo_name: &str,
    method_name: &str,
    args: &[bool],
) -> Option<Vec<String>> {
    let _span =
        tracing::debug_span!("check_manager_module_call", mdo_type_keyword, mdo_name, method_name)
            .entered();

    // Load metadata
    let configuration = match ctx.load_configuration() {
        Some(c) => c,
        None => {
            tracing::debug!(
                mdo_type_keyword,
                mdo_name,
                method_name,
                "No configuration available for manager module call check"
            );
            return None;
        }
    };

    // Parse MDO type from plural form (Документы → Document, Справочники → Catalog)
    let mdo_type = match bsl_metadata::MdoType::from_plural(mdo_type_keyword) {
        Some(t) => t,
        None => {
            tracing::debug!(
                mdo_type_keyword,
                mdo_name,
                method_name,
                "Unknown MDO type keyword, cannot check manager module call"
            );
            return None;
        }
    };

    tracing::debug!(
        mdo_type = ?mdo_type,
        mdo_name,
        method_name,
        "Checking manager module method call"
    );

    // Find Manager Module file
    let manager_file_id = find_manager_module_file(ctx, &configuration, mdo_type, mdo_name)?;

    // Build SymbolTree for Manager Module
    let module_id = ModuleId::new(manager_file_id);
    let manager_symbol_tree = ctx.symbol_tree_for(module_id);

    // Look up method in Manager Module
    let method_name_obj = Name::new(method_name);
    let method = manager_symbol_tree.find_method(&method_name_obj)?;

    // Only check exported methods for qualified calls
    if !method.is_export {
        tracing::debug!(
            mdo_type = ?mdo_type,
            mdo_name,
            method_name,
            "Method is not exported, skipping manager module call validation"
        );
        return None;
    }

    Some(check_missing_params(method, args))
}

/// Find the FileId for a CommonModule by resolving its URI through VFS.
///
/// Find the FileId for a Manager Module by resolving its path through VFS.
///
/// ## Implementation
///
/// 1. Verify metadata object exists in configuration
/// 2. Build Manager Module path: `{english_plural}/{mdo_name}/Ext/ManagerModule.bsl`
/// 3. Resolve FileId via ctx.file_set (bypasses Salsa for performance)
///
/// ## Example Paths
/// - Document "ПКО" → `Documents/ПКО/Ext/ManagerModule.bsl`
/// - Catalog "Справочник1" → `Catalogs/Справочник1/Ext/ManagerModule.bsl`
/// - InformationRegister "Регистр1" → `InformationRegisters/Регистр1/Ext/ManagerModule.bsl`
///
/// ## Performance
/// - O(1) HashMap lookup in FileSet
fn find_manager_module_file(
    ctx: &DiagnosticsContext,
    configuration: &bsl_metadata::Configuration,
    mdo_type: bsl_metadata::MdoType,
    mdo_name: &str,
) -> Option<FileId> {
    // Verify metadata object exists
    if !configuration.has_metadata_object(mdo_type, mdo_name) {
        tracing::debug!(
            mdo_type = ?mdo_type,
            mdo_name,
            "Metadata object not found in configuration"
        );
        return None;
    }

    // Build Manager Module path using English plural form
    let english_plural = match mdo_type {
        bsl_metadata::MdoType::Document => "Documents",
        bsl_metadata::MdoType::Catalog => "Catalogs",
        bsl_metadata::MdoType::InformationRegister => "InformationRegisters",
        bsl_metadata::MdoType::AccumulationRegister => "AccumulationRegisters",
        bsl_metadata::MdoType::AccountingRegister => "AccountingRegisters",
        bsl_metadata::MdoType::CalculationRegister => "CalculationRegisters",
        bsl_metadata::MdoType::ChartOfCharacteristicTypes => "ChartsOfCharacteristicTypes",
        bsl_metadata::MdoType::ChartOfAccounts => "ChartsOfAccounts",
        bsl_metadata::MdoType::ChartOfCalculationTypes => "ChartsOfCalculationTypes",
        bsl_metadata::MdoType::BusinessProcess => "BusinessProcesses",
        bsl_metadata::MdoType::Task => "Tasks",
        _ => {
            tracing::debug!(
                mdo_type = ?mdo_type,
                "MDO type does not have Manager Module"
            );
            return None;
        }
    };

    let manager_module_path = format!("{}/{}/Ext/ManagerModule.bsl", english_plural, mdo_name);

    let file_id = ctx.resolve_module_file(&manager_module_path);

    if file_id.is_none() {
        tracing::warn!(
            mdo_type = ?mdo_type,
            mdo_name,
            manager_module_path,
            "Manager Module file not found in VFS - ensure file is loaded"
        );
    }

    file_id
}

/// Check which required parameters are missing from a method call.
///
/// Returns a vector of parameter names that are required but not provided.
///
/// ## Rules
/// - Parameters with `has_default == true` are optional (skip)
/// - Parameters with `has_default == false` are required (check)
/// - A parameter is missing if:
///   - Index >= provided_args.len() (not enough arguments), OR
///   - provided_args[i] == false (empty argument like `, ,`)
///
/// ## Example
/// ```bsl
/// Функция Test(A, B = 1, C)
///     // A and C are required (no default)
///     // B is optional (has default)
/// КонецФункции
///
/// Test(5)      // Missing C → returns ["C"]
/// Test(, 2, 3) // Missing A → returns ["A"]
/// Test()       // Missing A, C → returns ["A", "C"]
/// ```
fn check_missing_params(method: &MethodSymbol, provided_args: &[bool]) -> Vec<String> {
    let mut missing = Vec::new();

    for (i, param) in method.params.iter().enumerate() {
        // Skip optional parameters (have default value)
        if param.has_default {
            continue;
        }

        // Check if parameter is missing or empty
        let is_missing = i >= provided_args.len() || !provided_args[i];

        if is_missing {
            missing.push(param.name.as_str().to_string());
        }
    }

    missing
}

#[cfg(test)]
mod tests {
    use crate::test_utils::{assert_diagnostic_range_multiline, check_hir_diagnostic};
    use crate::DiagnosticCode;

    fn filter(diagnostics: &[crate::Diagnostic]) -> Vec<&crate::Diagnostic> {
        diagnostics.iter().filter(|d| d.code == DiagnosticCode::MissedRequiredParameter).collect()
    }

    #[test]
    fn test_missed_required_parameter_simple() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(, 2);
КонецПроцедуры

Функция Сложение(Левый, Правый)
    Возврат Левый + Правый;
КонецФункции
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 1, "Expected 1 diagnostic");
        assert!(diags[0].message.contains("Левый"));
    }

    #[test]
    fn test_comprehensive() {
        // Inline version of MissedRequiredParameterDiagnostic.bsl.
        // Uses 4-space indentation to match original column positions.
        let code = r#"Процедура Рассчет()

    Результат = Сложение(, 2); // Range(2, 16, 2, 29)
    Сообщить(Результат);

    Инкремент(Результат);
    Сообщить(Результат);

    Результат = Сложение(5); // Range(8, 16, 8, 27)
    Сообщить(Результат);

    Результат = Сложение(5, 4, 3);
    Сообщить(Результат);

    Результат = Сложение(); // 2xRange(14, 16, 14, 26)
    Сообщить(Результат);

    Сообщить(Сложение(,)); // 2хRange(17, 13, 17, 24)
    Сообщить(Менеджер("Справочник")); // Range(18, 13, 18, 35)
КонецПроцедуры

Процедура Версионирование()
    ВерсионированиеПриЗаписи(1);
    Документы.ПКО.ВерсионированиеПриЗаписи(1);
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(1); // Range(24, 22, 24, 49)
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(2,); // Range(25, 22, 25, 50)
    ПервыйОбщийМодуль.ВерсионированиеПриЗаписи(); // 2xRange(26, 22, 26, 48)
    Сообщить(ПервыйОбщийМодуль.ВерсионированиеПриЗаписи()); // 2xRange(27, 31, 27, 57)
    Справочники.Справочник1.Тест(); // Range(28, 28, 28, 34);
    Результат = ЭтотОбъект.Сложение(, 2);

КонецПроцедуры

Функция Сложение(Левый, Правый) Экспорт
    Возврат Левый + Правый;
КонецФункции

Функция Инкремент(Значение, Приращение = 1)
    Значение = Значение + Приращение;
    Возврат Значение;
КонецФункции

Функция Менеджер(Тип = "Справочник", Вид)
    ИмяТипа = СтрШаблон("%1Менеджер.%2", Тип, Вид);
    Возврат Новый(Тип(ИмяТипа));
КонецФункции"#;

        let all = check_hir_diagnostic(code);
        let mut diags = filter(&all);
        diags.sort_by_key(|d| d.range.start());

        // HIR emits MissedRequiredParameter only for local (unqualified) calls.
        // Qualified calls (CommonModule, ThisObject, ManagerModule) are emitted by
        // lower_field_expr but analyze_qualified_call requires module name resolution
        // context that isn't available during lowering in test setup.
        //
        // Local call diagnostics:
        //   Line 2:  Сложение(, 2)           → missing 'Левый'
        //   Line 8:  Сложение(5)             → missing 'Правый'
        //   Line 14: Сложение()              → missing 'Левый', 'Правый'
        //   Line 17: Сложение(,)             → missing 'Левый', 'Правый'
        //   Line 18: Менеджер("Справочник")  → missing 'Вид'
        assert_eq!(
            diags.len(),
            5,
            "Expected 5 local-call diagnostics, got {}.\nMessages: {:?}",
            diags.len(),
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // Verify positions (0-indexed lines)
        assert_diagnostic_range_multiline(code, diags[0], 2, 16, 2, 29);
        assert!(diags[0].message.contains("Левый"));

        assert_diagnostic_range_multiline(code, diags[1], 8, 16, 8, 27);
        assert!(diags[1].message.contains("Правый"));

        assert_diagnostic_range_multiline(code, diags[2], 14, 16, 14, 26);
        assert!(diags[2].message.contains("Левый") && diags[2].message.contains("Правый"));

        assert_diagnostic_range_multiline(code, diags[3], 17, 13, 17, 24);
        assert!(diags[3].message.contains("Левый") && diags[3].message.contains("Правый"));

        assert_diagnostic_range_multiline(code, diags[4], 18, 13, 18, 35);
        assert!(diags[4].message.contains("Вид"));
    }

    #[test]
    fn test_optional_parameters_not_required() {
        let code = r#"
Процедура Тест()
    Инкремент(5);
КонецПроцедуры

Функция Инкремент(Значение, Приращение = 1)
    Возврат Значение + Приращение;
КонецФункции
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0, "Optional parameters should not trigger diagnostic");
    }

    #[test]
    fn test_extra_parameters_allowed() {
        let code = r#"
Процедура Тест()
    Результат = Сложение(1, 2, 3, 4);
КонецПроцедуры

Функция Сложение(A, B)
    Возврат A + B;
КонецФункции
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0, "Extra parameters should be allowed");
    }

    #[test]
    fn test_qualified_calls_skipped_without_metadata() {
        let code = r#"
Процедура Тест()
    ОбщийМодуль.Метод();
    Объект.Метод(1);
КонецПроцедуры

Функция Метод(A, B)
    Возврат A + B;
КонецФункции
"#;
        let all = check_hir_diagnostic(code);
        let diags = filter(&all);
        assert_eq!(diags.len(), 0, "Qualified calls should not trigger without metadata");
    }
}
