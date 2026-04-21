//! Method resolution adapter for BSL type inference.
//!
//! Thin bridge that lifts [`Resolver::resolve_qualified_method`] (owned by
//! `hir-def` — the single source of truth for name resolution) into a
//! diagnostic-ready [`MethodResolution`] carrying the [`FunctionSignature`]
//! that inference needs.
//!
//! ## Why this exists as a separate layer
//!
//! `Resolver` returns a method-oriented outcome expressed purely in
//! `hir-def` entities (`QualifiedMethodResolution` / `QualifiedMethodError`)
//! — `hir-def` must not depend on `hir-ty::UnresolvedMethodKind`. This
//! adapter:
//!
//! 1. Delegates resolution to the Resolver so `db.infer()` transitively
//!    depends on `db.configurations(...)` through Salsa: changing the
//!    workspace config set invalidates inference automatically.
//! 2. Maps [`QualifiedMethodError`] to [`UnresolvedMethodKind`] variants.
//! 3. Materialises [`FunctionSignature`] from the target method's symbol
//!    so `infer_qualified_call` can check arg counts and return type.
//!
//! ## Shadowing
//!
//! Shadowing (a local variable named identically to a CommonModule) is
//! handled during HIR lowering (`maybe_lower_as_qualified_call` in
//! `crates/hir-def/src/body/lower/expr.rs`): the call is not promoted to
//! `Expr::QualifiedPath`, so this function is never reached for a
//! shadowed receiver.

use hir_def::resolver::{QualifiedMethodError, Resolver};
use hir_def::ty::{FunctionSignature, Ty};
use hir_def::{ConfigsDatabase, MethodId, Name};
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
/// Thin adapter over [`Resolver::resolve_qualified_method`]: delegates name
/// resolution (with the CFE visibility gate and Salsa invalidation) to
/// `hir-def`, then materialises the [`FunctionSignature`] from the target
/// method's symbol.
///
/// # Parameters
///
/// - `db`: database; must provide [`ConfigsDatabase`] so resolution reads
///   `db.configurations(...)` through Salsa and `db.infer()` transitively
///   depends on the workspace config set.
/// - `module_name`: receiver module name (`ОбщегоНазначения`).
/// - `method_name`: method name (`СтрДлина`).
/// - `resolver`: inference-layer resolver (must include
///   [`Scope::WorkspaceScope`](hir_def::resolver::Scope)).
///
/// # Returns
///
/// - `Ok(MethodResolution)` — method found (may be non-exported; see
///   `is_export`).
/// - `Err(UnresolvedMethodKind::MethodNotFound)` — module not declared in
///   any visible configuration, not indexed, or method absent in the
///   resolved module.
pub fn resolve_qualified_call(
    db: &dyn ConfigsDatabase,
    module_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let resolution =
        resolver.resolve_qualified_method(db, module_name, method_name).map_err(|e| match e {
            // Both the config gate and the path-based lookup collapse to
            // `MethodNotFound` here. The distinction is preserved inside
            // hir-def (`QualifiedMethodError::NotVisibleInConfigs`) for any
            // future consumer that wants to surface a config-specific hint.
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
        })?;

    // Materialise the signature from the resolved method's symbol.
    //
    // Look up **by MethodId** rather than by name: when error recovery
    // leaves two methods with the same name, `find_method` returns the
    // first match, which may not be the symbol the Resolver picked.
    // By-id lookup guarantees the signature matches the resolved
    // `method_id`.
    //
    // The Resolver just read the target `symbol_tree` via the same Salsa
    // revision, so the MethodId must be present. `.expect` documents the
    // invariant loudly — if it ever fires, the symbol_tree is genuinely
    // out of sync with what the Resolver saw (tree corruption, not a
    // recoverable condition).
    let symbol_tree = db.symbol_tree(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let param_types: Vec<Ty> = method_symbol.params.iter().map(|_| Ty::Unknown).collect();
    let signature = FunctionSignature::new(param_types, method_symbol.return_type.clone());
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
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
