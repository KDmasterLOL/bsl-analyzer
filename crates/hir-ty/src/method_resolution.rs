//! Method resolution for BSL type inference.
//!
//! This module implements method resolution for qualified calls like `CommonModule.Method()`.
//! It handles:
//! - CommonModule lookup in workspace
//! - Method lookup within CommonModule
//! - Export flag validation
//! - Shadowing detection (local variables vs CommonModule names)
//!
//! ## Architecture
//!
//! ```text
//! CommonModule.Method() call
//!        ↓
//! resolve_qualified_call()
//!        ↓
//! 1. Check shadowing (is "CommonModule" a local variable?)
//! 2. Find CommonModule in workspace_symbols
//! 3. Find Method in CommonModule's SymbolTree
//! 4. Check export flag
//! 5. Return MethodResolution or error kind
//! ```
//!
//! ## Phase 3 Scope
//!
//! - Two-level qualified calls: `Module.Method()`
//! - CommonModule resolution only
//! - Export flag validation
//! - Shadowing detection
//!
//! ## Future (Phase 4+)
//!
//! - Three-level calls: `Документы.ПКО.Создать()`
//! - ThisObject resolution: `ЭтотОбъект.Method()`
//! - Metadata-based types

use hir_def::resolver::Resolver;
use hir_def::ty::{FunctionSignature, Ty};
use hir_def::{DefDatabase, MethodId, Name};
use vfs::FileId;

use crate::infer::UnresolvedMethodKind;

/// Result of method resolution.
///
/// Contains all information needed for type inference and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodResolution {
    /// Resolved method ID.
    pub method_id: MethodId,

    /// Is the method exported?
    ///
    /// Non-exported methods should trigger UnresolvedMethodCall diagnostic.
    pub is_export: bool,

    /// Function signature (parameter types + return type).
    ///
    /// Phase 3: Return type is Ty::Unknown for most methods
    /// Phase 4+: Actual return types from JSDoc or inference
    pub signature: FunctionSignature,

    /// Return type (convenience field, same as signature.ret).
    pub return_type: Ty,
}

impl MethodResolution {
    /// Create a new method resolution result.
    pub fn new(method_id: MethodId, is_export: bool, signature: FunctionSignature) -> Self {
        let return_type = (*signature.ret).clone();
        Self { method_id, is_export, signature, return_type }
    }
}

/// Resolve a qualified method call like `CommonModule.Method()`.
///
/// This is the main entry point for method resolution during type inference.
///
/// # Parameters
///
/// - `db`: Database for queries
/// - `module_name`: Name of the module (e.g., "ОбщегоНазначения")
/// - `method_name`: Name of the method (e.g., "СтрДлина")
/// - `resolver`: Current resolver context for shadowing detection
///
/// # Returns
///
/// - `Ok(MethodResolution)`: Method found and resolved
/// - `Err(UnresolvedMethodKind)`: Method not found, reason specified
///
/// # Shadowing Detection
///
/// If `module_name` resolves to a local variable or parameter, this function
/// returns `Err(ReceiverNotResolved)` because the call is not a CommonModule call.
///
/// Example:
/// ```bsl
/// Процедура Test()
///     Перем ОбщегоНазначения; // Local variable shadows CommonModule
///     ОбщегоНазначения = Новый Массив;
///     ОбщегоНазначения.Добавить(1); // Not a CommonModule call!
/// КонецПроцедуры
/// ```
///
/// # Phase 3 Implementation
///
/// Currently only resolves CommonModule calls. Future phases will add:
/// - Metadata manager calls: `Документы.ПКО.Создать()`
/// - ThisObject calls: `ЭтотОбъект.Method()`
pub fn resolve_qualified_call(
    db: &dyn DefDatabase,
    module_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
    all_files: &[FileId],
) -> Result<MethodResolution, UnresolvedMethodKind> {
    // 1. Check shadowing: is module_name a local variable?
    //
    // If the name resolves to a local (parameter or local variable),
    // then this is NOT a CommonModule call.
    if resolver.resolve_local(module_name).is_some() {
        return Err(UnresolvedMethodKind::ReceiverNotResolved);
    }

    // 2. Get workspace symbols (all CommonModules)
    let workspace_symbols = db.workspace_symbols(all_files);

    // 3. Find CommonModule by name
    let common_module = workspace_symbols
        .common_modules
        .get(module_name)
        .ok_or(UnresolvedMethodKind::MethodNotFound)?;

    // 4. Get SymbolTree for the CommonModule
    let symbol_tree = db.symbol_tree(common_module.module_id);

    // 5. Find method in SymbolTree
    let method_symbol =
        symbol_tree.find_method(method_name).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    // 6. Check export flag
    //
    // Phase 3: We still resolve non-exported methods (for type inference)
    // but mark them so caller can emit diagnostic
    let is_export = method_symbol.is_export;

    // 7. Build function signature
    //
    // Phase 3: Use existing return_type from MethodSymbol (Ty::Unknown for most)
    // Phase 4+: Improve with JSDoc parsing and type inference
    let param_types: Vec<Ty> = method_symbol.params.iter().map(|_p| Ty::Unknown).collect();

    let signature = FunctionSignature::new(param_types, method_symbol.return_type.clone());

    Ok(MethodResolution::new(method_symbol.id, is_export, signature))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_method_resolution_new() {
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature = FunctionSignature::new(vec![Ty::String], Ty::Number);

        let resolution = MethodResolution::new(method_id, true, signature.clone());

        assert_eq!(resolution.method_id, method_id);
        assert!(resolution.is_export);
        assert_eq!(resolution.return_type, Ty::Number);
        assert_eq!(resolution.signature, signature);
    }

    #[test]
    fn test_method_resolution_not_export() {
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature = FunctionSignature::new(vec![], Ty::Undefined);

        let resolution = MethodResolution::new(method_id, false, signature);

        assert!(!resolution.is_export);
    }
}
