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
//! Shadowing (a local variable / parameter / module-level `Перем` named
//! identically to a CommonModule) is handled at inference time in
//! `dispatch_bare_ident_field_call`'s cascade gate (gates 1 and 2 —
//! `Resolver::resolve_name`, `body_declares_binding`,
//! `assigned_var_names`). Those gates short-circuit silent before
//! reaching gate 3, so this function is invoked only when the
//! receiver positively resolves to a workspace CommonModule.

use bsl_types::kind::TypeId;
use hir_def::resolver::{QualifiedMethodError, Resolver};
use hir_def::symbol_tree::MethodSymbol;
use hir_def::ty::{FunctionSignature, FunctionSignatureTy, Ty};
use hir_def::{MethodId, Name};

use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
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
    pub signature: FunctionSignature,

    /// Return type (convenience field, same as signature.ret).
    pub return_type: TypeId,
}

impl MethodResolution {
    /// Create a new method resolution result.
    pub fn new(method_id: MethodId, is_export: bool, signature: FunctionSignature) -> Self {
        let return_type = signature.ret;
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
    db: &dyn HirDatabase,
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

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

/// Resolve a 3-segment manager-chain call like `Документы.ПКО.СоздатьДокумент()`.
///
/// Mirrors [`resolve_qualified_call`] for the manager chain: delegates name
/// resolution to [`Resolver::resolve_three_level_method`] (which owns the
/// CFE visibility gate and Salsa invalidation) and then materialises the
/// [`FunctionSignature`] from the target method's `MethodSymbol`.
///
/// # Parameters
///
/// - `db`: database; must implement [`ConfigsDatabase`] so the resolver
///   can consult `db.configurations(...)` through Salsa (the caller's
///   `infer` query transitively depends on the config set).
/// - `mdo_type_plural`: head segment — the plural collective name
///   (`Документы`, `Справочники`).
/// - `mdo_name`: middle segment — the metadata object identifier as it
///   appears in the configuration (`ПКО`).
/// - `method_name`: tail segment — the exported manager-module method.
/// - `resolver`: inference-layer resolver (must include
///   [`Scope::WorkspaceScope`](hir_def::resolver::Scope)).
///
/// # Returns
///
/// - `Ok(MethodResolution)` when the method exists, even if non-exported;
///   the caller inspects `is_export` to pick between `MethodNotExport` and
///   success.
/// - `Err(UnresolvedMethodKind::MethodNotFound)` for any resolver failure
///   — missing MDO declaration, missing manager module, unknown method.
///   The distinction between `NotVisibleInConfigs` and `NotFound` is
///   preserved inside `hir-def` for a future config-specific hint.
pub fn resolve_three_level_call(
    db: &dyn HirDatabase,
    mdo_type_plural: &Name,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let resolution = resolver
        .resolve_three_level_method(db, mdo_type_plural, mdo_name, method_name)
        .map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
        })?;

    // Same invariant as `resolve_qualified_call`: Resolver just read the
    // target `symbol_tree` via the same Salsa revision, so the MethodId
    // must be present by id. `.expect` documents the contract loudly.
    let symbol_tree = db.symbol_tree(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

/// Strict map from a [`MetadataKind`] to its parent [`MdoType`] for
/// the [`resolve_object_module_call`] gate.
///
/// Returns `Some(MdoType)` only for `*Object` kinds — the only HIR
/// receiver shapes for which `<MDO>/Ext/ObjectModule.bsl` is the
/// authoritative call surface. `*Ref` kinds (`CatalogRef`,
/// `DocumentRef`, …) NEVER consult ObjectModule: a reference value
/// can only access exported predefined items / attributes, not the
/// `Экспорт`-method surface declared inside the object module.
/// Register kinds, register-part kinds and tabular sections also
/// return `None` — no ObjectModule analogue today.
///
/// Deliberately *not* reusing
/// [`crate::field_lookup::mdo_type_for_kind`]: that helper accepts
/// both `*Object` and `*Ref` to drive attribute lookup, which is the
/// opposite of what the method-resolution gate needs.
///
/// The match is **exhaustive on every [`MetadataKind`] variant** (no
/// wildcard arm) so adding a new `*Object` variant — e.g. a future
/// `BusinessProcessObject` — surfaces as a compiler error here, not a
/// silent fall-through to `None`. The mirror direction
/// ([`MetadataKind::object_kind_for`]) is the canonical
/// `MdoType → *Object` map; this entry inverts it.
fn object_kind_to_mdo(kind: hir_def::ty::MetadataKind) -> Option<bsl_metadata::MdoType> {
    use bsl_metadata::MdoType;
    use hir_def::ty::MetadataKind;
    Some(match kind {
        MetadataKind::CatalogObject => MdoType::Catalog,
        MetadataKind::DocumentObject => MdoType::Document,
        MetadataKind::ExchangePlanObject => MdoType::ExchangePlan,
        MetadataKind::ChartOfAccountsObject => MdoType::ChartOfAccounts,
        MetadataKind::TaskObject => MdoType::Task,
        MetadataKind::BusinessProcessObject => MdoType::BusinessProcess,
        MetadataKind::DataProcessorObject => MdoType::DataProcessor,
        MetadataKind::ReportObject => MdoType::Report,
        // `*Ref` kinds — reference values, no ObjectModule.bsl call surface.
        MetadataKind::CatalogRef
        | MetadataKind::DocumentRef
        | MetadataKind::EnumRef
        | MetadataKind::TaskRef
        | MetadataKind::BusinessProcessRef
        | MetadataKind::ExchangePlanRef
        | MetadataKind::ChartOfAccountsRef
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef => return None,
        // Register record managers / record sets / records — none of
        // these have an ObjectModule.bsl surface. Reject here so the
        // gate stays strict. Records (the per-record element kinds
        // yielded by `Для каждого … Из …` over a record-set) are
        // included for the same reason.
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecordSet
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecordSet
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecordSet
        | MetadataKind::CalculationRegisterRecord => return None,
        // Register parts, the synthetic `RegisterFilter` receiver, and
        // tabular sections have no ObjectModule.bsl surface.
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    })
}

/// Strict map from a [`MetadataKind`] to its parent register
/// [`MdoType`] for the [`resolve_record_set_module_call`] gate.
///
/// Returns `Some(MdoType)` only for register-set kinds — the HIR
/// receiver shape that BSL semantics actually allows to reach
/// `RecordSetModule.bsl`'s exported procedures:
///
/// - [`MetadataKind::InformationRegisterRecordSet`] → `MdoType::InformationRegister`
/// - [`MetadataKind::AccumulationRegisterRecordSet`] → `MdoType::AccumulationRegister`
/// - [`MetadataKind::AccountingRegisterRecordSet`] → `MdoType::AccountingRegister`
/// - [`MetadataKind::CalculationRegisterRecordSet`] → `MdoType::CalculationRegister`
///
/// **`InformationRegisterRecordManager` is NOT in this map.**
/// Per 1С runtime semantics, exported procedures inside
/// `RecordSetModule.bsl` are callable through a record-set receiver,
/// NOT through the record manager. Calling `МЗ.Экспорт()` where
/// `Экспорт()` lives in `RecordSetModule.bsl` is rejected by 1С;
/// wiring the record-manager kind here would false-positive that
/// error path.
///
/// `*Object` kinds belong to [`object_kind_to_mdo`]. `*Ref` kinds and
/// register parts return `None` — no module-level call surface. The
/// match is exhaustive on every `MetadataKind` variant (no wildcard)
/// so adding a new register-record flavour surfaces as a compiler
/// error here, not a silent fall-through to `None`.
fn record_set_kind_to_mdo(kind: hir_def::ty::MetadataKind) -> Option<bsl_metadata::MdoType> {
    use bsl_metadata::MdoType;
    use hir_def::ty::MetadataKind;
    Some(match kind {
        MetadataKind::InformationRegisterRecordSet => MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet => MdoType::AccumulationRegister,
        MetadataKind::AccountingRegisterRecordSet => MdoType::AccountingRegister,
        MetadataKind::CalculationRegisterRecordSet => MdoType::CalculationRegister,
        // `InformationRegisterRecordManager` deliberately rejected —
        // see doc comment above (1С semantics: `RecordSetModule.bsl`
        // procedures need a record-set receiver, not a record-manager).
        MetadataKind::InformationRegisterRecordManager => return None,
        // `*Object` kinds — ObjectModule.bsl, not RecordSetModule.bsl.
        MetadataKind::CatalogObject
        | MetadataKind::DocumentObject
        | MetadataKind::ExchangePlanObject
        | MetadataKind::ChartOfAccountsObject
        | MetadataKind::TaskObject
        | MetadataKind::BusinessProcessObject
        | MetadataKind::DataProcessorObject
        | MetadataKind::ReportObject => return None,
        // `*Ref` kinds — reference values, no module-level call surface.
        MetadataKind::CatalogRef
        | MetadataKind::DocumentRef
        | MetadataKind::EnumRef
        | MetadataKind::TaskRef
        | MetadataKind::BusinessProcessRef
        | MetadataKind::ExchangePlanRef
        | MetadataKind::ChartOfAccountsRef
        | MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef => return None,
        // `*Record` element kinds — yielded by iterating a record-set,
        // but the record itself doesn't reach `RecordSetModule.bsl`
        // (1С runtime: only the set receiver does). Reject like `*Ref`.
        MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecord => return None,
        // Register parts, the synthetic `RegisterFilter` receiver, and
        // tabular sections have no RecordSetModule.bsl surface.
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. }
        | MetadataKind::TabularSection { .. }
        | MetadataKind::TabularSectionRow { .. } => return None,
    })
}

/// Resolve a 2-shape RecordSetModule method call like
/// `НЗ = РегистрыСведений.X.СоздатьМенеджерЗаписи(); НЗ.МойМетод()`
/// where `НЗ` carries
/// [`Ty::MetadataRef { InformationRegisterRecordManager, .. }`][MetadataRef].
///
/// Mirrors [`resolve_object_module_call`] but routes the workspace
/// lookup to [`Resolver::resolve_record_set_module_method`]. The
/// strict register-record filter ([`record_set_kind_to_mdo`]) is
/// applied **inside** the wrapper — the call site can pass any
/// `MetadataKind` without guarding; non-register-record kinds return
/// `Err(MethodNotFound)` immediately and the call site's
/// platform-fallback path takes over.
///
/// [MetadataRef]: hir_def::ty::Ty::MetadataRef
pub fn resolve_record_set_module_call(
    db: &dyn HirDatabase,
    kind: hir_def::ty::MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let mdo_type = record_set_kind_to_mdo(kind).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    let resolution = resolver
        .resolve_record_set_module_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
        })?;

    let symbol_tree = db.symbol_tree(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

/// Resolve a 2-shape ObjectModule method call like
/// `Об = Справочники.X.СоздатьЭлемент(); Об.МойМетод()` where `Об`
/// carries [`Ty::MetadataRef { *Object, .. }`][MetadataRef].
///
/// Mirrors [`resolve_aliased_manager_call`] but routes the workspace
/// lookup to [`Resolver::resolve_object_module_method`]. The strict
/// `*Object` filter ([`object_kind_to_mdo`]) is applied **inside**
/// the wrapper so the call site can pass any [`MetadataKind`] without
/// guarding against `*Ref`/register receivers — a non-`*Object` kind
/// returns `Err(MethodNotFound)` immediately, which the call site's
/// platform-fallback path then takes.
///
/// # Returns
///
/// - `Ok(MethodResolution)` when the method exists in the workspace
///   `ObjectModule.bsl` (may be non-exported).
/// - `Err(UnresolvedMethodKind::MethodNotFound)` for any of:
///   - `kind` is not a `*Object` flavour (strict-filter reject).
///   - MDO not declared in any visible configuration.
///   - No `<MDO>/Ext/ObjectModule.bsl` for `(MdoType, name)`.
///   - Object module exists but does not contain `method_name`.
///
/// [MetadataRef]: hir_def::ty::Ty::MetadataRef
pub fn resolve_object_module_call(
    db: &dyn HirDatabase,
    kind: hir_def::ty::MetadataKind,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let mdo_type = object_kind_to_mdo(kind).ok_or(UnresolvedMethodKind::MethodNotFound)?;

    let resolution = resolver
        .resolve_object_module_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
            QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
                UnresolvedMethodKind::MethodNotFound
            }
        })?;

    let symbol_tree = db.symbol_tree(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

/// Resolve a 2-shape aliased manager method call like
/// `М = Справочники.X; М.МойМетод()` where `М` carries
/// [`Ty::ObjectManager`].
///
/// Mirrors [`resolve_three_level_call`]: delegates the workspace
/// lookup (with the CFE visibility gate and Salsa invalidation) to
/// [`Resolver::resolve_aliased_manager_method`] and materialises the
/// [`FunctionSignature`] from the target method's `MethodSymbol`. The
/// only difference is that the manager-collection plural has already
/// been consumed by type inference (the variable's `Ty::ObjectManager`
/// kind is the parsed `MdoType`), so this entry takes `MdoType`
/// directly instead of a plural `Name`.
///
/// # Returns
///
/// - `Ok(MethodResolution)` when the method exists, even when not
///   exported; the caller inspects `is_export` to pick between
///   `MethodNotExport` and success.
/// - `Err(UnresolvedMethodKind::MethodNotFound)` for any resolver
///   failure (no `ManagerType` for this `MdoType`, MDO not declared in
///   any visible configuration, manager module not indexed, method
///   absent in the module). The caller falls back to the platform
///   `lookup_method` only on this outcome — workspace authority
///   exhausted, platform gets the next consult.
pub fn resolve_aliased_manager_call(
    db: &dyn HirDatabase,
    mdo_type: bsl_metadata::MdoType,
    mdo_name: &Name,
    method_name: &Name,
    resolver: &Resolver,
) -> Result<MethodResolution, UnresolvedMethodKind> {
    let resolution = resolver
        .resolve_aliased_manager_method(db, mdo_type, mdo_name, method_name)
        .map_err(|e| match e {
        QualifiedMethodError::NotVisibleInConfigs | QualifiedMethodError::NotFound => {
            UnresolvedMethodKind::MethodNotFound
        }
    })?;

    // Same invariant as the other adapters: the Resolver just read the
    // target `symbol_tree` via the same Salsa revision, so the
    // `MethodId` must be present by id.
    let symbol_tree = db.symbol_tree(resolution.method_id.module);
    let method_symbol = symbol_tree.find_method_by_id(resolution.method_id).expect(
        "method_id returned by Resolver must exist in symbol_tree — \
         symbol_tree / Resolver are out of sync",
    );

    let signature = materialise_signature_enriched(db, resolution.method_id, method_symbol);
    Ok(MethodResolution::new(resolution.method_id, resolution.is_export, signature))
}

/// Lower a [`MethodSymbol`] into a semantic [`FunctionSignature`].
///
/// Shared by `resolve_qualified_call` (2-segment) and
/// `resolve_three_level_call` (3-segment): both resolve a method by name
/// and then need to hand the caller typed parameters / return type. The
/// cascade walks the JSDoc-derived `TypeRef` first (when present), then
/// falls back to `Ty::Unknown` for parameters and to the
/// `MethodSymbol::return_type` default for the return type — `Ty::Undefined`
/// for procedures and `Ty::Unknown` for functions without a
/// `// Возвращаемое значение:` block.
///
/// Lowering runs through [`TyLoweringContext`] so the JSDoc `TypeRef`
/// lookups share a single path with `Expr::New` and XML metadata: adding
/// a new prefix or a future `Ty::Union` is a one-place edit.
fn materialise_signature(method_symbol: &MethodSymbol) -> FunctionSignatureTy {
    let ctx = TyLoweringContext::new();

    let param_types: Vec<Ty> = method_symbol
        .params
        .iter()
        .map(|p| p.type_ref.as_ref().map(|t| ctx.lower_type_ref(t)).unwrap_or(Ty::Unknown))
        .collect();
    let defaults: Vec<bool> = method_symbol.params.iter().map(|p| p.has_default).collect();

    let ret = method_symbol
        .return_type_ref
        .as_ref()
        .map(|t| ctx.lower_type_ref(t))
        .unwrap_or_else(|| method_symbol.return_type.clone());

    FunctionSignatureTy::new_with_defaults(param_types, defaults, ret)
}

/// Phase O.11 — materialise a method signature with body-inferred
/// return-type enrichment.
///
/// Wraps [`materialise_signature`]. Whenever the docstring-derived
/// signature returns `Ty::Unknown` (no `// Возвращаемое значение:`
/// block + a default that resolves to Unknown), the O.10
/// `method_return_type_query` is consulted. If body inference
/// produces a non-`Unknown` `Ty`, the enriched signature carries it
/// in place of the original Unknown — surfacing cascade-typed
/// returns through hover without invalidating the explicit
/// docstring-wins precedence.
///
/// # Tracking & cycle safety
///
/// This is a plain `fn`, NOT `#[salsa::tracked]`. The cycle is
/// detected and recovered at the inner `method_return_type_query`
/// node; the enriched-materialisation wrapper is transparent to
/// salsa's cycle iteration. Promoting this to a tracked query would
/// introduce a second cycle node and is explicitly deferred until
/// smoke evidence shows excessive re-materialisation.
///
/// # Performance
///
/// The body-walk via `method_return_type_query` happens ONLY in the
/// `Unknown`-fallback branch. Methods with an explicit return
/// docstring short-circuit at the `matches!(*sig.ret, Ty::Unknown)`
/// gate — no salsa work, no cascade.
pub(crate) fn materialise_signature_enriched(
    db: &dyn HirDatabase,
    method_id: hir_def::MethodId,
    method_symbol: &MethodSymbol,
) -> FunctionSignature {
    let mut sig: FunctionSignatureTy = materialise_signature(method_symbol);
    if matches!(*sig.ret, Ty::Unknown) {
        let method_input = hir_def::MethodIdInput::new(db, method_id);
        // `method_return_type_query` is kernel-native (§4.D.3); bridge its
        // `TypeId` back into the still-`Ty` `FunctionSignatureTy.ret`.
        let inferred = crate::ty_bridge::typeid_to_ty(
            db,
            crate::method_graph::method_return_type_query(db, method_input),
        );
        if !matches!(inferred, Ty::Unknown) {
            *sig.ret = inferred;
        }
    }

    crate::ty_bridge::function_signature_ty_to_kernel(db, &sig)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ty_bridge::{function_signature_ty_to_kernel, typeid_to_ty};
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn test_method_resolution_new() {
        let db = InMemoryDb::new();
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature_ty = FunctionSignatureTy::new(vec![Ty::String], Ty::Number);
        let signature = function_signature_ty_to_kernel(&db, &signature_ty);

        let resolution = MethodResolution::new(method_id, true, signature.clone());

        assert_eq!(resolution.method_id, method_id);
        assert!(resolution.is_export);
        assert_eq!(typeid_to_ty(&db, resolution.return_type), Ty::Number);
        assert_eq!(resolution.signature, signature);
    }

    #[test]
    fn test_method_resolution_not_export() {
        let db = InMemoryDb::new();
        let method_id = MethodId { module: hir_def::ModuleId { file_id: FileId(0) }, local_id: 0 };
        let signature_ty = FunctionSignatureTy::new(vec![], Ty::Undefined);
        let signature = function_signature_ty_to_kernel(&db, &signature_ty);

        let resolution = MethodResolution::new(method_id, false, signature);

        assert!(!resolution.is_export);
    }

    #[test]
    fn object_kind_to_mdo_accepts_only_object_variants() {
        use bsl_metadata::MdoType;
        use hir_def::ty::MetadataKind;
        // Phase B strict-filter contract.
        assert_eq!(object_kind_to_mdo(MetadataKind::CatalogObject), Some(MdoType::Catalog));
        assert_eq!(object_kind_to_mdo(MetadataKind::DocumentObject), Some(MdoType::Document));
        assert_eq!(
            object_kind_to_mdo(MetadataKind::ExchangePlanObject),
            Some(MdoType::ExchangePlan),
        );
        assert_eq!(
            object_kind_to_mdo(MetadataKind::ChartOfAccountsObject),
            Some(MdoType::ChartOfAccounts),
        );
        // `*Ref` rejected.
        assert_eq!(object_kind_to_mdo(MetadataKind::CatalogRef), None);
        assert_eq!(object_kind_to_mdo(MetadataKind::DocumentRef), None);
        // Register-record kinds rejected (Phase C territory).
        assert_eq!(object_kind_to_mdo(MetadataKind::InformationRegisterRecordManager), None);
        assert_eq!(object_kind_to_mdo(MetadataKind::AccumulationRegisterRecordSet), None);
    }

    #[test]
    fn record_set_kind_to_mdo_accepts_only_register_set_variants() {
        use bsl_metadata::MdoType;
        use hir_def::ty::MetadataKind;
        // Strict-filter contract: only record-set kinds reach
        // `RecordSetModule.bsl` per 1С semantics.
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::InformationRegisterRecordSet),
            Some(MdoType::InformationRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::AccumulationRegisterRecordSet),
            Some(MdoType::AccumulationRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::AccountingRegisterRecordSet),
            Some(MdoType::AccountingRegister),
        );
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::CalculationRegisterRecordSet),
            Some(MdoType::CalculationRegister),
        );
        // `InformationRegisterRecordManager` is a single-record
        // handle, not a record-set receiver — 1С rejects calls to
        // `RecordSetModule.bsl` exports through it.
        assert_eq!(record_set_kind_to_mdo(MetadataKind::InformationRegisterRecordManager), None);
        // Synthetic `RegisterFilter` is the `.Отбор` receiver, not a
        // record-set; it has no module-level call surface of its own.
        assert_eq!(
            record_set_kind_to_mdo(MetadataKind::RegisterFilter {
                parent: MdoType::InformationRegister,
            }),
            None,
        );
        // `*Object` rejected.
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CatalogObject), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::DocumentObject), None);
        // `*Ref` rejected.
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CatalogRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::InformationRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::AccumulationRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::AccountingRegisterRef), None);
        assert_eq!(record_set_kind_to_mdo(MetadataKind::CalculationRegisterRef), None);
    }
}
