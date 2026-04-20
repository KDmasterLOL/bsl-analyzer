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
//! 1. Resolve CommonModule name → FileId via module_index (path-based, cheap)
//! 2. Find Method in CommonModule's SymbolTree
//! 3. Check export flag
//! 4. Return MethodResolution or error kind
//! ```
//!
//! ## Shadowing
//!
//! Shadowing (a local variable named identically to a CommonModule) is
//! handled **before** this function is ever called:
//! `maybe_lower_as_qualified_call` in `crates/hir-def/src/body/lower/expr.rs`
//! refuses to promote the call into `Expr::QualifiedPath` when the receiver
//! IDENT is a known local/parameter, so inference keeps it as
//! `Expr::Call { callee: Expr::Field, .. }` and this function does not run
//! for the shadowed call. The resolver passed in by
//! `InferenceContext::get_resolver` is `Resolver::with_workspace_scope`
//! (no expression scopes), so a defensive `resolver.resolve_local` check
//! here would silently not fire anyway.
//!
//! ## Why module_index, not workspace_symbols
//!
//! Both indexes map CommonModule names to FileIds, but `workspace_symbols`
//! forces `symbol_tree` on every file in the source root to build the
//! methods list — a ~50 s workspace-wide scan on cold start for a 12k file
//! project. `module_index` is built purely from VFS paths (no BSL parsing),
//! so lookup is O(1) and the hit is followed by a single
//! `symbol_tree(target)` call, which is already prewarmed by
//! `preload_dependencies` for files that the open file depends on. This
//! aligns the inference path with the diagnostics path
//! (`ctx.resolve_qualified_path` in `ide-diagnostics`), which has always
//! used `module_index`.
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
use hir_def::{DefDatabase, MethodId, ModuleId, Name};
#[cfg(test)]
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
/// - `_resolver`: unused (see module-level docs on shadowing)
///
/// # Returns
///
/// - `Ok(MethodResolution)`: Method found and resolved
/// - `Err(UnresolvedMethodKind)`: Method not found, reason specified
///
/// # Shadowing
///
/// Shadowing of a CommonModule name by a local variable or parameter is
/// handled during HIR lowering (`maybe_lower_as_qualified_call`), before
/// this function is invoked. See the module-level docs.
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
    _resolver: &Resolver,
    source_root_id: base_db::SourceRootId,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    // Shadowing is resolved earlier during HIR lowering (see module-level
    // docs). The resolver argument is kept for API stability and for future
    // phases that may need it (e.g. ThisObject / three-level calls where
    // body-local context matters).

    // 1. Resolve CommonModule name → FileId via module_index
    //
    // module_index is built from VFS paths only (no BSL parsing), so the
    // lookup is O(1) and doesn't trigger a workspace-wide scan. Previously
    // this went through db.workspace_symbols() which forces symbol_tree on
    // every file in the source root.
    let module_index = db.module_index(source_root_id);
    let target_file_id = module_index
        .resolve_common_module(module_name)
        .ok_or(UnresolvedMethodKind::MethodNotFound)?;

    // 2. Get SymbolTree for the resolved CommonModule (single-file query,
    //    prewarmed by preload_dependencies for open-file neighbours).
    let symbol_tree = db.symbol_tree(ModuleId::new(target_file_id));

    // 3. Find method in SymbolTree
    let method_symbol =
        symbol_tree.find_method(method_name).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    // 4. Check export flag
    //
    // Phase 3: We still resolve non-exported methods (for type inference)
    // but mark them so caller can emit diagnostic
    let is_export = method_symbol.is_export;

    // 5. Build function signature
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
