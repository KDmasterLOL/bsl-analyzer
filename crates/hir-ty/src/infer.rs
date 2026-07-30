use base_db::FileIdInput;
use bsl_platform::deprecation::{self, ElementKind, Lookup};
use bsl_types::builders::Builders;
use bsl_types::facet::{DateComponent, FormDataFacet, MdoRefFacet, ProjectionSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{ConfigId, Projection, TypeId, TypeKind};
use bsl_types::testing::RootConfigCtx;
use cfg_types::{BindingId, IdConversion};
use hir_def::body::Body;
use hir_def::effective_module::EffectiveModuleId;
use hir_def::hir::{BinaryOp, Expr, ExprIdx, Literal, Stmt, StmtIdx, UnaryOp};
use hir_def::resolver::Resolver;
use hir_def::symbol_tree::SymbolTree;
use hir_def::{
    sdbl_hir_for_file_query, DefWithBodyId, ExprId, MethodIdInput, Name, SdblExprId, StmtId,
};
use intern::NormName;
use once_cell::sync::Lazy;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::Arc;
use stdx::case::CaseExt;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
use crate::method_resolution;
use crate::platform_manager_lookup::{resolve_platform_manager_method, PlatformMethodResolution};

static DEPRECATED_PLATFORM_MEMBER_OWNERS: Lazy<FxHashSet<String>> = Lazy::new(|| {
    let mut owners = FxHashSet::default();
    for entry in deprecation::registry().entries() {
        if !matches!(entry.element_kind, ElementKind::Method | ElementKind::Property) {
            continue;
        }
        if let Some(owner) = entry.owner {
            owners.insert(owner.ru.fold_lower());
            if !owner.en.is_empty() {
                owners.insert(owner.en.fold_lower());
            }
        }
    }
    owners
});

/// Heap-size estimators wired into salsa's `heap_size` hook (the `salsa_unstable`
/// memory report). salsa's default reports only the fixed slot stack size — the
/// `Arc` pointer — so the collections behind these memoised results are otherwise
/// invisible. These return an approximate live-heap byte count: hashbrown table
/// capacity is derived from length (load factor 7/8, rounded to a power of two),
/// owned `String`/`Vec` payloads are summed, and small `Copy` ids are counted by
/// `size_of`. Over-approximate by design; the goal is a per-ingredient memory map,
/// not exact accounting.
pub(crate) mod heap_estimate {
    use super::*;
    use std::mem::size_of;

    pub(crate) use stdx::heap::{map_table_bytes, vec_bytes};

    /// Heap of the `expr`/`binding`-keyed maps plus their owned-string and
    /// nested-vec payloads, shared by all three inference-result shapes.
    fn body_maps_heap(
        var_types: &FxHashMap<String, TypeId>,
        implicit_locals: &FxHashMap<String, ImplicitLocalInfo>,
        binding_types: &FxHashMap<BindingId, TypeId>,
        expr_types: &FxHashMap<ExprId, TypeId>,
        diagnostics_len: usize,
        call_arg_bindings_len: usize,
    ) -> usize {
        let mut b = map_table_bytes::<ExprId, TypeId>(expr_types.len());
        b += map_table_bytes::<BindingId, TypeId>(binding_types.len());
        b += map_table_bytes::<String, TypeId>(var_types.len());
        for k in var_types.keys() {
            b += k.capacity();
        }
        b += map_table_bytes::<String, ImplicitLocalInfo>(implicit_locals.len());
        for (k, info) in implicit_locals {
            b += k.capacity() + vec_bytes::<ImplicitLocalAssignment>(info.assignments.len());
        }
        b += vec_bytes::<InferenceDiagnostic>(diagnostics_len);
        b += vec_bytes::<CallArgBinding>(call_arg_bindings_len);
        b
    }

    pub(crate) fn inference_result_heap(v: &Arc<InferenceResult>) -> usize {
        let r = &**v;
        let mut b = size_of::<InferenceResult>();
        b +=
            map_table_bytes::<DefWithBodyId, FxHashMap<ExprId, TypeId>>(r.expr_types_by_body.len());
        for inner in r.expr_types_by_body.values() {
            b += map_table_bytes::<ExprId, TypeId>(inner.len());
        }
        b += map_table_bytes::<DefWithBodyId, FxHashMap<BindingId, TypeId>>(
            r.binding_types_by_body.len(),
        );
        for inner in r.binding_types_by_body.values() {
            b += map_table_bytes::<BindingId, TypeId>(inner.len());
        }
        b += map_table_bytes::<String, TypeId>(r.var_types.len());
        for k in r.var_types.keys() {
            b += k.capacity();
        }
        b += map_table_bytes::<DefWithBodyId, FxHashMap<String, ImplicitLocalInfo>>(
            r.implicit_locals_by_body.len(),
        );
        for inner in r.implicit_locals_by_body.values() {
            b += map_table_bytes::<String, ImplicitLocalInfo>(inner.len());
            for (k, info) in inner {
                b += k.capacity() + vec_bytes::<ImplicitLocalAssignment>(info.assignments.len());
            }
        }
        b += vec_bytes::<(DefWithBodyId, InferenceDiagnostic)>(r.diagnostics.len());
        b += vec_bytes::<CallArgBinding>(r.call_arg_bindings.len());
        b
    }

    pub(crate) fn body_inference_result_heap(v: &Arc<BodyInferenceResult>) -> usize {
        let r = &**v;
        size_of::<BodyInferenceResult>()
            + body_maps_heap(
                &r.var_types,
                &r.implicit_locals,
                &r.binding_types,
                &r.expr_types,
                r.diagnostics.len(),
                r.call_arg_bindings.len(),
            )
            + vec_bytes::<ExprId>(r.return_expr_ids.len())
    }

    pub(crate) fn module_code_inference_result_heap(v: &Arc<ModuleCodeInferenceResult>) -> usize {
        let r = &**v;
        size_of::<ModuleCodeInferenceResult>()
            + body_maps_heap(
                &r.var_types,
                &r.implicit_locals,
                &r.binding_types,
                &r.expr_types,
                r.diagnostics.len(),
                r.call_arg_bindings.len(),
            )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InferenceResult {
    pub expr_types_by_body: FxHashMap<DefWithBodyId, FxHashMap<ExprId, TypeId>>,

    pub var_types: FxHashMap<String, TypeId>,

    pub binding_types_by_body: FxHashMap<DefWithBodyId, FxHashMap<BindingId, TypeId>>,

    pub implicit_locals_by_body: FxHashMap<DefWithBodyId, FxHashMap<String, ImplicitLocalInfo>>,

    pub diagnostics: Vec<(DefWithBodyId, InferenceDiagnostic)>,

    pub call_arg_bindings: Vec<CallArgBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplicitLocalInfo {
    pub name: Name,
    pub first_assignment: ExprId,
    pub ty: TypeId,
    pub assignments: Vec<ImplicitLocalAssignment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplicitLocalAssignment {
    pub target: ExprId,
    pub ty: TypeId,
}

impl InferenceResult {
    pub fn type_id_of_expr_in(&self, owner: DefWithBodyId, expr: ExprId) -> Option<TypeId> {
        self.expr_types_by_body.get(&owner)?.get(&expr).copied()
    }

    pub fn type_id_of_binding_in(&self, owner: DefWithBodyId, id: BindingId) -> Option<TypeId> {
        self.binding_types_by_body.get(&owner)?.get(&id).copied()
    }

    pub fn has_diagnostics(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceDiagnostic {
    UnresolvedMethodCall {
        expr: ExprId,
        receiver_name: Name,
        method_name: Name,
        kind: UnresolvedMethodKind,
    },

    MismatchedArgCount {
        call_expr: ExprId,
        required_count: usize,
        total_count: usize,
        found: usize,
    },

    TypeMismatch {
        expr: ExprId,
        expected: TypeId,
        actual: TypeId,
        from_doc_comment: bool,
    },

    UnresolvedField {
        expr: ExprId,
        receiver_ty: TypeId,
        field_name: Name,
    },

    ReadOnlyPropertyAssignment {
        lhs: ExprId,
        receiver_ty: TypeId,
        field_name: Name,
    },

    /// Assignment to a bare Global-context property (`Справочники = …`). The
    /// platform refuses the write and declares no local, so the statement has no
    /// effect a reader could rely on — and the name keeps denoting the global.
    GlobalPropertyNotWritable {
        lhs: ExprId,
        name: Name,
    },

    DeprecatedPlatformMember {
        expr: ExprId,
        type_name: Name,
        member_name: Name,
        is_property: bool,
    },

    RedundantAccessToObjectTwoLevel {
        expr: ExprId,
        module: Name,
    },

    MissedRequiredParameterCommonModule {
        expr: ExprId,
        callee: Name,
        module: Name,
        args: Vec<bool>,
    },

    /// A resolved platform member is not available in some of the execution
    /// environments this body runs in (`missing` — the EDT-style
    /// "[Web client]" qualifier set).
    UnavailableInEnvironment {
        expr: ExprId,
        name: Name,
        member_kind: EnvMemberKind,
        missing: hir_def::execution_env::EnvFlags,
    },

    /// User code (a common module or a same-module method) is called from an
    /// execution environment it is not compiled for: a server-only common
    /// module without `ВызовСервера` called from client code, or a
    /// `&НаКлиенте` form method called from server-side code.
    ModuleAccessibility {
        expr: ExprId,
        name: Name,
        callee_kind: EnvCalleeKind,
        missing: hir_def::execution_env::EnvFlags,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvMemberKind {
    Method,
    Property,
    GlobalFunction,
    GlobalProperty,
    Type,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvCalleeKind {
    CommonModule,
    LocalMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnresolvedMethodKind {
    MethodNotFound,
    MethodNotExport,
    CommonModuleNoSource,
    ReceiverNotResolved,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgBinding {
    pub owner: DefWithBodyId,

    pub call_expr: ExprId,

    pub args: Vec<ExprId>,

    pub candidate: CandidateCallBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateCallBinding {
    pub candidates: crate::call_resolution::CallCandidateSet,

    pub resolution: crate::call_resolution::CallResolution,
}

pub struct InferenceContext<'db> {
    db: &'db dyn HirDatabase,

    /// File whose configuration/metadata/cross-module context governs resolution.
    /// For an ordinary module this is the module's own file; for an effective
    /// `&ИзменениеИКонтроль` module it stays the BASE file (the effective module *is*
    /// the base module with the extension's edits applied). Same-module method/variable
    /// lookups are NOT keyed on this — they go through `local_symbols`.
    context_file_id: FileId,

    /// Effective module's symbol tree, present only when inferring a spliced
    /// `&ИзменениеИКонтроль` module. `None` for every ordinary module, in which case
    /// same-module lookups resolve through `db.symbol_tree(ModuleId{context_file_id})` —
    /// the byte-identical default.
    local_symbols: Option<Arc<SymbolTree>>,

    /// Effective return types of the module's own methods, keyed by `MethodId.local_id`,
    /// present only on the SECOND pass of effective inference. A bare same-module call to
    /// a `&ИзменениеИКонтроль` target must see its *changed* body's return, not the base
    /// body's (which `materialise_signature_enriched` would re-derive via the base-keyed
    /// `method_return_type_query`). `None` for ordinary modules and the first effective
    /// pass → byte-identical default behavior.
    local_effective_returns: Option<Arc<FxHashMap<u32, TypeId>>>,

    /// Paired base module of a configuration-*extension* module, present only when
    /// inferring an extension module's OWN bodies under weaving (`&Вместо`/`&Перед`/
    /// `&После`). A bare same-module call/variable that misses in the extension's own
    /// symbols retries against this base module (the extension shadows the base). `None`
    /// for ordinary and effective modules → byte-identical default resolution.
    weaving_base: Option<hir_def::ModuleId>,

    /// Return type a bare `ПродолжитьВызов(...)` / `ProceedWithCall(...)` call yields in this
    /// body, present only when inferring a `&Вместо` (Around) interceptor under weaving. The
    /// call invokes the original base method `M`, so its result is `M`'s return type, not the
    /// platform global `ПродолжитьВызов`'s generic return. `None` for ordinary, effective,
    /// `&Перед`/`&После`, and module-code bodies → byte-identical platform-default typing.
    proceed_return: Option<TypeId>,

    /// Arity `(required_count, total_count)` of the base method a bare `ПродолжитьВызов(...)`
    /// re-enters, present only when inferring a `&Вместо` (Around) interceptor under weaving.
    /// The call must pass a valid argument count for the base method; unlike
    /// [`Self::proceed_return`] this is set whenever the base method resolves (a procedure base
    /// has no informative return but its arity is still checked). `None` everywhere else.
    proceed_arity: Option<(usize, usize)>,

    owner: DefWithBodyId,

    body: Arc<Body>,

    return_expr_ids: Vec<ExprId>,

    var_types: FxHashMap<NormName, TypeId>,

    implicit_locals: FxHashMap<String, ImplicitLocalInfo>,

    assigned_var_names: rustc_hash::FxHashSet<NormName>,

    /// Memoised `bare_global_name_claim` verdicts. That lookup reaches past the
    /// body into the module and the workspace while the body itself does not
    /// change during a run, and the same collection root recurs throughout a
    /// body, so it is asked once per name.
    shadowed_root_names: FxHashMap<NormName, bool>,

    binding_types: FxHashMap<BindingId, TypeId>,

    expr_types: FxHashMap<ExprId, TypeId>,

    diagnostics: Vec<InferenceDiagnostic>,

    call_arg_bindings: Vec<CallArgBinding>,

    /// Execution environments this body runs in (module base ∩ compilation
    /// directive). Empty = unknown → availability checks are skipped.
    body_env: hir_def::execution_env::EnvFlags,

    /// Environments availability diagnostics may report on
    /// ([`EnvOptions::checked_environments`]) — the missing set is clipped to
    /// this mask, keeping opt-in environments (external connection, mobile)
    /// out of default verdicts.
    checked_env: hir_def::execution_env::EnvFlags,

    /// Lazily-built lookup of unqualified-callable exports of the GLOBAL common modules
    /// visible to this body: lowercased method name → owning `MethodId` (first global
    /// module wins on a name collision). Built once per inference run on first bare-call
    /// miss and reused, so a global-util-heavy body does not re-enumerate global modules
    /// per call. `None` until first consulted. See [`Self::global_export_map`].
    global_exports: Option<Arc<FxHashMap<NormName, hir_def::MethodId>>>,

    /// Per-body literal `Структура` shapes (keys built via `Новый Структура` / `.Вставить`), keyed
    /// by lowercased local name. Collected once at the start of [`Self::infer_all`] and used to
    /// enrich a structure local's type with its keys on each read. Empty for bodies with no such
    /// construction → byte-identical default typing. See [`crate::structure_keys`].
    structure_shapes: FxHashMap<String, crate::structure_keys::StructureShape>,

    /// Physical-flag accessibility verdict per callee common module, so a body
    /// calling the same module many times reads its metadata once instead of
    /// recording a salsa dependency per call. `None` — not a common module
    /// (skip); `Some((env, server_call))` — feed the environment compare.
    callee_module_env: FxHashMap<FileId, Option<(hir_def::execution_env::EnvFlags, bool)>>,

    /// [`method_body_env`] per same-module callee, for the local cross-directive
    /// accessibility check.
    local_callee_env: FxHashMap<u32, hir_def::execution_env::EnvFlags>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyInferenceResult {
    pub owner: DefWithBodyId,
    pub var_types: FxHashMap<String, TypeId>,
    pub implicit_locals: FxHashMap<String, ImplicitLocalInfo>,
    pub binding_types: FxHashMap<BindingId, TypeId>,
    pub expr_types: FxHashMap<ExprId, TypeId>,
    pub diagnostics: Vec<InferenceDiagnostic>,
    pub call_arg_bindings: Vec<CallArgBinding>,

    pub(crate) return_expr_ids: Vec<ExprId>,
}

impl BodyInferenceResult {
    pub fn empty_for(owner: DefWithBodyId) -> Self {
        Self {
            owner,
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
            return_expr_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCodeInferenceResult {
    pub owner: DefWithBodyId,
    pub var_types: FxHashMap<String, TypeId>,
    pub implicit_locals: FxHashMap<String, ImplicitLocalInfo>,
    pub binding_types: FxHashMap<BindingId, TypeId>,
    pub expr_types: FxHashMap<ExprId, TypeId>,
    pub diagnostics: Vec<InferenceDiagnostic>,
    pub call_arg_bindings: Vec<CallArgBinding>,
}

impl Default for ModuleCodeInferenceResult {
    fn default() -> Self {
        Self {
            owner: DefWithBodyId::ModuleCode,
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
        }
    }
}

impl ModuleCodeInferenceResult {
    pub fn from_body(body: BodyInferenceResult) -> Self {
        debug_assert!(
            matches!(body.owner, DefWithBodyId::ModuleCode),
            "ModuleCodeInferenceResult::from_body requires DefWithBodyId::ModuleCode owner"
        );
        Self {
            owner: body.owner,
            var_types: body.var_types,
            implicit_locals: body.implicit_locals,
            binding_types: body.binding_types,
            expr_types: body.expr_types,
            diagnostics: body.diagnostics,
            call_arg_bindings: body.call_arg_bindings,
        }
    }
}

#[derive(Debug, Clone)]
pub enum InferOwnerResult {
    Method(Arc<BodyInferenceResult>),
    ModuleCode(Arc<ModuleCodeInferenceResult>),
}

impl InferOwnerResult {
    pub fn owner(&self) -> DefWithBodyId {
        match self {
            InferOwnerResult::Method(r) => r.owner,
            InferOwnerResult::ModuleCode(r) => r.owner,
        }
    }

    pub fn type_id_of_expr(&self, expr: ExprId) -> Option<TypeId> {
        match self {
            InferOwnerResult::Method(r) => r.expr_types.get(&expr).copied(),
            InferOwnerResult::ModuleCode(r) => r.expr_types.get(&expr).copied(),
        }
    }

    pub fn expr_types(&self) -> &FxHashMap<ExprId, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.expr_types,
            InferOwnerResult::ModuleCode(r) => &r.expr_types,
        }
    }

    pub fn type_id_of_binding(&self, binding: BindingId) -> Option<TypeId> {
        match self {
            InferOwnerResult::Method(r) => r.binding_types.get(&binding).copied(),
            InferOwnerResult::ModuleCode(r) => r.binding_types.get(&binding).copied(),
        }
    }

    pub fn var_types(&self) -> &FxHashMap<String, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.var_types,
            InferOwnerResult::ModuleCode(r) => &r.var_types,
        }
    }

    pub fn implicit_locals(&self) -> &FxHashMap<String, ImplicitLocalInfo> {
        match self {
            InferOwnerResult::Method(r) => &r.implicit_locals,
            InferOwnerResult::ModuleCode(r) => &r.implicit_locals,
        }
    }

    pub fn binding_types(&self) -> &FxHashMap<BindingId, TypeId> {
        match self {
            InferOwnerResult::Method(r) => &r.binding_types,
            InferOwnerResult::ModuleCode(r) => &r.binding_types,
        }
    }

    pub fn call_arg_bindings(&self) -> &[CallArgBinding] {
        match self {
            InferOwnerResult::Method(r) => &r.call_arg_bindings,
            InferOwnerResult::ModuleCode(r) => &r.call_arg_bindings,
        }
    }
}

pub fn infer_owner(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
) -> InferOwnerResult {
    match owner {
        DefWithBodyId::Method(local_id) => {
            let module_id = hir_def::ModuleId { file_id };
            let method_id = hir_def::MethodId { module: module_id, local_id };
            let method_input = MethodIdInput::new(db, method_id);
            InferOwnerResult::Method(db.infer_method(method_input))
        }
        DefWithBodyId::ModuleCode => InferOwnerResult::ModuleCode(db.infer_module_code(file_id)),
    }
}

impl<'db> InferenceContext<'db> {
    pub fn new(
        db: &'db dyn HirDatabase,
        file_id: FileId,
        owner: DefWithBodyId,
        body: &Arc<Body>,
    ) -> Self {
        Self::new_impl(db, file_id, owner, body, true)
    }

    fn new_impl(
        db: &'db dyn HirDatabase,
        file_id: FileId,
        owner: DefWithBodyId,
        body: &Arc<Body>,
        with_body_env: bool,
    ) -> Self {
        let opts = db.env_options();
        let body_env = if !with_body_env {
            hir_def::execution_env::EnvFlags::EMPTY
        } else {
            match owner {
                DefWithBodyId::Method(local_id) => crate::method_environment::effective_method_env(
                    db,
                    hir_def::MethodId { module: hir_def::ModuleId { file_id }, local_id },
                    &opts,
                ),
                DefWithBodyId::ModuleCode => {
                    let metadata = db.module_metadata(hir_def::ModuleId { file_id });
                    hir_def::execution_env::module_code_env(&metadata, &opts)
                }
            }
        };
        Self {
            db,
            context_file_id: file_id,
            local_symbols: None,
            local_effective_returns: None,
            weaving_base: None,
            proceed_return: None,
            proceed_arity: None,
            owner,
            body: Arc::clone(body),
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            assigned_var_names: rustc_hash::FxHashSet::default(),
            shadowed_root_names: FxHashMap::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
            return_expr_ids: Vec::new(),
            body_env,
            checked_env: opts.checked_environments,
            global_exports: None,
            structure_shapes: FxHashMap::default(),
            callee_module_env: FxHashMap::default(),
            local_callee_env: FxHashMap::default(),
        }
    }

    pub fn new_for_method(
        db: &'db dyn HirDatabase,
        method: MethodIdInput<'db>,
        body: &Arc<Body>,
    ) -> Self {
        let mid = method.method_id(db);
        Self::new(db, mid.module.file_id, DefWithBodyId::Method(mid.local_id), body)
    }

    /// Inference over an effective `&ИзменениеИКонтроль` module: local bodies/symbols come
    /// from the spliced effective text (`local_symbols`), while configuration / metadata /
    /// cross-module resolution keeps `context_file_id = base_file`. Identical to [`Self::new`]
    /// apart from same-module method/variable lookups, which route to `local_symbols`.
    pub fn new_effective(
        db: &'db dyn HirDatabase,
        base_file_id: FileId,
        local_symbols: Arc<SymbolTree>,
        owner: DefWithBodyId,
        body: &Arc<Body>,
    ) -> Self {
        // The effective module's local_ids do not match the base file's item
        // tree, so a directive computed against it may belong to another
        // method — disable availability checks rather than misattribute them.
        let mut ctx = Self::new_impl(db, base_file_id, owner, body, false);
        ctx.local_symbols = Some(local_symbols);
        ctx
    }

    /// Inference over an extension module's OWN bodies under *weaving* (`&Вместо` /
    /// `&Перед` / `&После`): `context_file_id` stays the extension file (configuration /
    /// metadata resolution must key on the ext file), while bare same-module lookups that
    /// miss in the extension's own symbols fall back to `base_module`. Identical to
    /// [`Self::new`] apart from that fallback, threaded through [`Self::get_resolver`].
    pub fn new_weaving(
        db: &'db dyn HirDatabase,
        ext_file_id: FileId,
        base_module: hir_def::ModuleId,
        owner: DefWithBodyId,
        body: &Arc<Body>,
    ) -> Self {
        // A weaving interceptor's effective directive comes from the base
        // method it intercepts, unknown at this layer — no availability checks.
        let mut ctx = Self::new_impl(db, ext_file_id, owner, body, false);
        ctx.weaving_base = Some(base_module);
        ctx
    }

    /// Second-pass effective inference: supply the effective return types of the module's
    /// own methods so bare same-module calls to changed methods resolve their *changed*
    /// body's return. See [`Self::local_effective_returns`].
    pub fn set_local_effective_returns(&mut self, returns: Arc<FxHashMap<u32, TypeId>>) {
        self.local_effective_returns = Some(returns);
    }

    /// Weaving inference of a `&Вместо` interceptor: supply the base method's return type so a
    /// bare `ПродолжитьВызов(...)` in this body types as the original method's result rather
    /// than the platform global's generic return. See [`Self::proceed_return`].
    pub fn set_proceed_return(&mut self, ty: TypeId) {
        self.proceed_return = Some(ty);
    }

    /// Weaving inference of a `&Вместо` interceptor: supply the base method's arity so a bare
    /// `ПродолжитьВызов(...)` in this body is validated as a call to that method (it re-enters
    /// it). See [`Self::proceed_arity`].
    pub fn set_proceed_arity(&mut self, required_count: usize, total_count: usize) {
        self.proceed_arity = Some((required_count, total_count));
    }

    /// In effective inference, prefer a LOCAL method's CHANGED-body return over the value
    /// materialised by the base-keyed `resolve_*` paths (which re-derive the base body).
    /// A no-op for cross-module resolutions (the file guard fails) and for ordinary modules
    /// (`local_effective_returns == None`), so it is safe to apply at every local-capable
    /// `MethodResolution` return site.
    fn effective_local_return(&self, method_id: hir_def::MethodId, resolved: TypeId) -> TypeId {
        if method_id.module.file_id == self.context_file_id {
            if let Some(ret) = self
                .local_effective_returns
                .as_ref()
                .and_then(|m| m.get(&method_id.local_id).copied())
            {
                if !self.is_unknown(ret) {
                    return ret;
                }
            }
        }
        resolved
    }

    /// Lazily build (and cache) the unqualified-export lookup of the global common
    /// modules visible to this body. See [`Self::global_exports`].
    fn global_export_map(&mut self) -> Arc<FxHashMap<NormName, hir_def::MethodId>> {
        if let Some(map) = &self.global_exports {
            return Arc::clone(map);
        }
        let resolver = self.get_resolver();
        let mut map: FxHashMap<NormName, hir_def::MethodId> = FxHashMap::default();
        for (_module, method_name, method_id) in resolver.global_common_module_exports(self.db) {
            map.entry(NormName::intern(method_name.as_str())).or_insert(method_id);
        }
        let arc = Arc::new(map);
        self.global_exports = Some(Arc::clone(&arc));
        arc
    }

    /// Whether `name` is a method of the current module (or, under weaving, its paired
    /// base) — a same-module method takes precedence over the global context, so the
    /// bare global-export path must defer to it. Mirrors the same-module lookup the
    /// `TypeKind::Unknown` call arm performs.
    fn bare_module_method_exists(&self, name: &Name) -> bool {
        let symbol_tree = match &self.local_symbols {
            Some(symbols) => symbols,
            None => self.db.symbol_tree_ref(hir_def::ModuleId::new(self.context_file_id)),
        };
        if symbol_tree.find_method(name).is_some() {
            return true;
        }
        self.weaving_base
            .is_some_and(|base| self.db.symbol_tree_ref(base).find_method(name).is_some())
    }

    /// Resolve a bare `Имя(...)` call against the exported methods of the visible global
    /// common modules. Returns the call's result type when `name` is such an export,
    /// running the full call contract — argument-count validation and parameter bindings —
    /// exactly as a qualified `Модуль.Имя(...)` call would, but without the qualified-only
    /// `RedundantAccessToObjectTwoLevel` lint (there is no redundant receiver to flag).
    fn resolve_bare_global_export(
        &mut self,
        name: &Name,
        args: &[ExprId],
        callee: ExprId,
    ) -> Option<TypeId> {
        let method_id = self.global_export_map().get(&NormName::intern(name.as_str())).copied()?;

        self.check_common_module_callee_env(callee, None, method_id.module);

        for arg in args {
            self.infer_expr(*arg);
        }

        let symbol_tree = self.db.symbol_tree_ref(method_id.module);
        let method_symbol = symbol_tree.find_method_by_id(method_id)?;
        let signature = crate::method_resolution::materialise_signature_enriched(
            self.db,
            method_id,
            method_symbol,
        );

        self.expr_types.insert(callee, self.db.unknown());
        let return_ty = self.effective_local_return(method_id, signature.ret);
        let candidates =
            crate::user_call_candidates::for_resolved_method(self.db, name, method_id, return_ty)
                .ok()?;
        Some(self.record_candidate_call_arg_binding(callee, args, candidates))
    }

    fn push_inference_diagnostic(&mut self, diag: InferenceDiagnostic) {
        let key = match &diag {
            InferenceDiagnostic::UnresolvedMethodCall { expr, .. } => *expr,
            InferenceDiagnostic::MismatchedArgCount { call_expr, .. } => *call_expr,
            InferenceDiagnostic::TypeMismatch { expr, .. } => *expr,
            InferenceDiagnostic::UnresolvedField { expr, .. } => *expr,
            InferenceDiagnostic::ReadOnlyPropertyAssignment { lhs, .. } => *lhs,
            InferenceDiagnostic::GlobalPropertyNotWritable { lhs, .. } => *lhs,
            InferenceDiagnostic::DeprecatedPlatformMember { expr, .. } => *expr,
            InferenceDiagnostic::RedundantAccessToObjectTwoLevel { expr, .. } => *expr,
            InferenceDiagnostic::MissedRequiredParameterCommonModule { expr, .. } => *expr,
            InferenceDiagnostic::UnavailableInEnvironment { expr, .. } => *expr,
            InferenceDiagnostic::ModuleAccessibility { expr, .. } => *expr,
        };
        if self.body.is_recovered(key) {
            return;
        }
        self.diagnostics.push(diag);
    }

    fn is_unknown(&self, id: TypeId) -> bool {
        id == self.db.unknown()
    }

    fn is_undefined(&self, id: TypeId) -> bool {
        id == self.db.undefined()
    }

    fn exact_platform_owner(&self, ty: TypeId) -> Option<Name> {
        let TypeKind::PlatformObject(facet) = self.db.lookup_type(ty) else {
            return None;
        };
        Some(Name::new(facet.name.as_str()))
    }

    /// Report a resolved platform member that is missing from some of the
    /// environments this body runs in. Free when nothing is wrong: one u8
    /// mask compare against availability already carried by the lookup
    /// result. `#Если` branches narrow the body mask per their condition
    /// (see the `Stmt::PreprocIf` arm); the check is skipped when either
    /// side is unknown.
    fn check_member_env(
        &mut self,
        expr: ExprId,
        name: &Name,
        member_env: hir_def::execution_env::EnvFlags,
        member_kind: EnvMemberKind,
    ) {
        if self.body_env.is_empty() || member_env.is_empty() {
            return;
        }
        let missing = self.body_env.without(member_env) & self.checked_env;
        if missing.is_empty() {
            return;
        }
        self.push_inference_diagnostic(InferenceDiagnostic::UnavailableInEnvironment {
            expr,
            name: name.clone(),
            member_kind,
            missing,
        });
    }

    /// True when a bare manager-collection name is shadowed by a user symbol
    /// and thus does not denote the platform global: a body binding (`Перем`,
    /// parameter, loop variable), a module-level variable or method, a form
    /// attribute or form-self property, an implicit `ЭтотОбъект`/record-set
    /// member, or a workspace common module.
    ///
    /// A plain assignment does NOT shadow. `Справочники = Новый Структура` does
    /// not declare a local: the name belongs to a Global-context property, and
    /// the platform refuses the write rather than creating a variable, so the
    /// name keeps denoting the collection throughout the body. Only a DECLARED
    /// owner takes it. Shadowing is preprocessor-blind, like every other
    /// shadowing decision in inference.
    fn manager_collection_shadowed(&mut self, name: &Name) -> bool {
        let key = NormName::intern(name.as_str());
        if self.body_declares_binding(name) {
            return true;
        }
        if let Some(cached) = self.shadowed_root_names.get(&key) {
            return *cached;
        }
        let resolver = self.get_resolver();
        let claimed =
            crate::platform_global_lookup::bare_global_name_claim(self.db, &resolver, None, name)
                .is_some();
        self.shadowed_root_names.insert(key, claimed);
        claimed
    }

    /// Environment check for a bare manager-collection root (`Справочники`,
    /// `Перечисления`, …). The mask compare runs first, so on the available
    /// path — every server-side body — no shadowing lookups happen at all;
    /// the resolver is consulted only when a verdict is about to be issued.
    fn check_manager_collection_env(
        &mut self,
        expr: ExprId,
        name: &Name,
        mdo_type: bsl_metadata::MdoType,
    ) {
        let member_env = crate::platform_global_lookup::manager_collection_env(mdo_type);
        if self.body_env.is_empty() || member_env.is_empty() {
            return;
        }
        if (self.body_env.without(member_env) & self.checked_env).is_empty() {
            return;
        }
        if self.manager_collection_shadowed(name) {
            return;
        }
        self.check_member_env(expr, name, member_env, EnvMemberKind::GlobalProperty);
    }

    /// The type name a `Новый(...)` names by value, trimmed once here so every
    /// consumer below sees the same spelling. A `Тип(...)` wrapper counts only
    /// when it really is the platform function: a nearer declaration takes the
    /// name over, in the Local → Module → Global-CM → Platform order the call
    /// path itself applies.
    fn constructed_type_name<'b>(&mut self, body: &'b Body, arg: ExprIdx) -> Option<&'b str> {
        if let Some(text) = crate::type_literal::bare_string_literal(body, arg) {
            return Some(text.trim());
        }
        let (callee, text) = crate::type_literal::type_ctor_literal(body, arg)?;
        if self.is_call_name_shadowed(callee) {
            return None;
        }
        Some(text.trim())
    }

    fn is_call_name_shadowed(&mut self, name: &Name) -> bool {
        let key = NormName::intern(name.as_str());
        if self.body_declares_binding(name) || self.assigned_var_names.contains(&key) {
            return true;
        }
        // A module-level `Перем` takes the name over just as a module method
        // does, so both are asked. Not through `resolve_name`: it answers
        // `Builtin` first, and every name worth asking about here — `Тип` above
        // all — is precisely a builtin, so the module would never be consulted.
        let resolver = self.get_resolver();
        if resolver.resolve_module_method(self.db, name).is_some()
            || resolver.resolve_module_variable(self.db, name).is_some()
        {
            return true;
        }
        self.global_export_map().contains_key(&key)
    }

    /// The same type-availability check as the syntactic `Новый X`, for a name
    /// that arrived as a string value (`Новый("X")`, `Новый(Тип("X"))`). A
    /// qualified name denotes a configuration object rather than a platform
    /// type, and configuration objects carry no per-environment availability.
    fn check_constructed_type_env(&mut self, expr: ExprId, name: &str) {
        if name.contains('.') {
            return;
        }
        let platform = bsl_platform::PlatformDataInner::instance();
        let Some(platform_type) = platform.get_type(name) else {
            return;
        };
        if platform.is_ambiguous_type_name(name) {
            return;
        }
        let environment =
            hir_def::execution_env::EnvFlags::from_platform_context(platform_type.context.as_ref());
        self.check_member_env(expr, &Name::new(name), environment, EnvMemberKind::Type);
    }

    /// Accessibility of a cross-module call to `callee_module` (a common
    /// module): every environment this body runs in must either be one the
    /// callee is compiled for, or — for `ВызовСервера` modules — a client
    /// environment making a remote server call.
    /// `module_name` is the name as written at the call site; `None` (a bare
    /// call to a global module's export) falls back to the module's metadata
    /// name.
    fn check_common_module_callee_env(
        &mut self,
        expr: ExprId,
        module_name: Option<&Name>,
        callee_module: hir_def::ModuleId,
    ) {
        use hir_def::execution_env::{self, EnvFlags};
        if self.body_env.is_empty() || callee_module.file_id == self.context_file_id {
            return;
        }
        // Physical flags per callee module are cached per body: a body calling
        // the same module many times must not record a salsa dependency per
        // call.
        let cached = match self.callee_module_env.get(&callee_module.file_id) {
            Some(v) => *v,
            None => {
                let opts = self.db.env_options();
                let metadata = self.db.module_metadata(callee_module);
                let v = metadata
                    .common_module
                    .as_ref()
                    .map(|cm| (execution_env::common_module_env(cm, &opts), cm.is_server_call()));
                self.callee_module_env.insert(callee_module.file_id, v);
                v
            }
        };
        let Some((physical_env, physical_server_call)) = cached else { return };

        let missing_with = |env: EnvFlags, server_call: bool| {
            if env.is_empty() {
                return EnvFlags::EMPTY;
            }
            let mut missing = self.body_env.without(env) & self.checked_env;
            if server_call {
                missing = missing.without(EnvFlags::ALL_CLIENTS);
            }
            missing
        };

        // Fast path on the resolved body's physical flags. An extension that
        // NARROWS an adopted module's flags slips through here as a missed
        // diagnostic — accepted: adoption in practice widens access
        // (`ВызовСервера`, client contexts), and the trade keeps the
        // configuration-scoped lookup off the hot path entirely.
        if missing_with(physical_env, physical_server_call).is_empty() {
            return;
        }

        let metadata = self.db.module_metadata(callee_module);
        let Some(physical) = metadata.common_module.as_ref() else { return };
        let name = match module_name {
            Some(name) => name.clone(),
            None => Name::new(bsl_metadata::MdObject::name(physical.as_ref())),
        };
        // The caller's own extension may adopt the module and replace its
        // flags wholesale (enable `ВызовСервера` or client contexts), so
        // before reporting, re-judge against the flags visible TO THE CALLER.
        // The physical flags stand when the provider has no configuration
        // scoping.
        let opts = self.db.env_options();
        let caller_scoped = self.db.resolve_common_module(self.context_file_id, name.as_str());
        let cm = caller_scoped.as_deref().unwrap_or(physical);
        let missing =
            missing_with(execution_env::common_module_env(cm, &opts), cm.is_server_call());
        if missing.is_empty() {
            return;
        }
        self.push_inference_diagnostic(InferenceDiagnostic::ModuleAccessibility {
            expr,
            name,
            callee_kind: EnvCalleeKind::CommonModule,
            missing,
        });
    }

    /// Accessibility of a same-module call across compilation directives.
    /// A client-side caller invoking a server method is the form's regular
    /// remote server call, so only the server side is ever a violation: code
    /// compiled for the server cannot reach a method that exists only on the
    /// client.
    fn check_local_callee_env(&mut self, expr: ExprId, name: &Name, callee_local_id: u32) {
        use hir_def::execution_env::EnvFlags;
        if self.body_env.is_empty() {
            return;
        }
        let callee_env = match self.local_callee_env.get(&callee_local_id) {
            Some(&env) => env,
            None => {
                let opts = self.db.env_options();
                let env = crate::method_environment::effective_method_env(
                    self.db,
                    hir_def::MethodId {
                        module: hir_def::ModuleId { file_id: self.context_file_id },
                        local_id: callee_local_id,
                    },
                    &opts,
                );
                self.local_callee_env.insert(callee_local_id, env);
                env
            }
        };
        if callee_env.is_empty() {
            return;
        }
        let missing = self.body_env.without(callee_env) & self.checked_env & EnvFlags::SERVER_SIDE;
        if missing.is_empty() {
            return;
        }
        self.push_inference_diagnostic(InferenceDiagnostic::ModuleAccessibility {
            expr,
            name: name.clone(),
            callee_kind: EnvCalleeKind::LocalMethod,
            missing,
        });
    }

    fn push_deprecated_platform_member_diagnostic(
        &mut self,
        expr: ExprId,
        receiver_ty: TypeId,
        member_name: &Name,
        is_property: bool,
    ) {
        let Some(type_name) = self.exact_platform_owner(receiver_ty) else {
            return;
        };

        let owner_key = type_name.as_str().fold_lower();
        if !DEPRECATED_PLATFORM_MEMBER_OWNERS.contains(owner_key.as_str()) {
            return;
        }

        let lookup = if is_property {
            Lookup::property(type_name.as_str(), member_name.as_str())
        } else {
            Lookup::method(type_name.as_str(), member_name.as_str())
        };
        if deprecation::registry().lookup(lookup).is_none() {
            return;
        }

        self.push_inference_diagnostic(InferenceDiagnostic::DeprecatedPlatformMember {
            expr,
            type_name,
            member_name: member_name.clone(),
            is_property,
        });
    }

    pub fn finish(self) -> BodyInferenceResult {
        BodyInferenceResult {
            owner: self.owner,
            // `var_types` is `NormName`-keyed internally (the hot lookup path during
            // inference); the public result stays `String`-keyed for its many
            // consumers across crate boundaries, so materialise once here — a
            // single allocation per body, replacing a fold per lookup.
            var_types: self.var_types.iter().map(|(k, v)| (k.as_str().to_string(), *v)).collect(),
            implicit_locals: self.implicit_locals,
            binding_types: self.binding_types,
            expr_types: self.expr_types,
            diagnostics: self.diagnostics,
            call_arg_bindings: self.call_arg_bindings,
            return_expr_ids: self.return_expr_ids,
        }
    }

    fn record_candidate_call_arg_binding(
        &mut self,
        call_expr: ExprId,
        args: &[ExprId],
        candidates: crate::call_resolution::CallCandidateSet,
    ) -> TypeId {
        let argument_types = args
            .iter()
            .map(|arg| self.expr_types.get(arg).copied().unwrap_or_else(|| self.db.unknown()))
            .collect::<Vec<_>>();
        let projection = crate::call_binding::resolve_binding(self.db, candidates, &argument_types);
        let return_ty = projection.semantic.resolution.return_ty;
        if !self.body.is_recovered(call_expr) {
            self.call_arg_bindings.push(CallArgBinding {
                owner: self.owner,
                call_expr,
                args: args.to_vec(),
                candidate: projection.semantic,
            });
        }
        return_ty
    }

    fn get_resolver(&self) -> Resolver {
        let module_id = hir_def::ModuleId { file_id: self.context_file_id };
        match &self.local_symbols {
            Some(symbols) => {
                Resolver::with_builtins_and_workspace_effective(module_id, Arc::clone(symbols))
            }
            None => match self.weaving_base {
                Some(base) => Resolver::with_builtins_and_workspace_weaving(module_id, base),
                None => Resolver::with_builtins_and_workspace(module_id),
            },
        }
    }

    fn body_declares_binding(&self, name: &hir_def::Name) -> bool {
        let target = NormName::intern(name.as_str());
        self.body.bindings_iter().any(|(_, b)| NormName::intern(b.name.as_str()) == target)
    }

    /// True when the receiver is a reassigned local variable and a definition
    /// that actually reaches this use either resolves the method or cannot be
    /// typed. Sequential inference records the textually-last assignment type,
    /// so at a use inside a sibling branch the receiver type may be a
    /// cross-branch artefact; reaching definitions restore the flow facts and
    /// keep the diagnostic alive when the stale type is the only one that
    /// reaches (e.g. straight-line reassignment).
    fn method_resolves_on_alternate_assignment(
        &self,
        receiver_expr: ExprId,
        receiver_ty: TypeId,
        method_name: &hir_def::Name,
    ) -> bool {
        let Expr::Path(name) = self.body.expr(receiver_expr) else {
            return false;
        };
        let key = name.as_str().fold_lower();
        let Some(info) = self.implicit_locals.get(&key) else {
            return false;
        };
        if info.assignments.len() < 2 {
            return false;
        }

        let vouches = |ty: TypeId| {
            ty != receiver_ty
                && (self.is_unknown(ty)
                    || crate::method_lookup::lookup_method(self.db, ty, method_name).is_some())
        };

        // Without flow facts a non-reaching assignment must not vouch for the
        // call (it would hide a real error after a straight-line
        // reassignment), so no reaching definitions — no suppression.
        let Some(reaching) = self.reaching_assignment_types(receiver_expr, &key) else {
            return false;
        };
        reaching.iter().any(|ty| match ty {
            Some(ty) => vouches(*ty),
            // A reaching definition inference could not type (e.g. a loop
            // back-edge not seen yet) — "method not found" is unprovable.
            None => true,
        })
    }

    /// Types of the assignments reaching `use_expr` for variable `var_key`.
    /// `None` per entry when the defining statement's value has no recorded
    /// type; `None` overall when reaching definitions are unavailable for the
    /// owner.
    fn reaching_assignment_types(
        &self,
        use_expr: ExprId,
        var_key: &str,
    ) -> Option<Vec<Option<TypeId>>> {
        let DefWithBodyId::Method(local_id) = self.owner else {
            return None;
        };
        let stmt_id = self.body.enclosing_stmt(use_expr)?;
        let module_defs = self.db.module_reaching_definitions(self.context_file_id);
        let method_defs = module_defs.get(local_id)?;
        let defs = method_defs.defs_for_var_at_stmt(var_key, stmt_id)?;
        Some(
            defs.into_iter()
                .map(|def| match def.def_site {
                    dataflow::reaching_defs::DefSite::Assignment(stmt_raw) => {
                        match self.body.stmt(StmtId::from_raw(stmt_raw)) {
                            Stmt::Assign { value, .. } => {
                                self.expr_types.get(&ExprId::from_idx(*value)).copied()
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                })
                .collect(),
        )
    }

    pub fn infer_all(&mut self) {
        let _p = tracing::debug_span!("infer_all").entered();

        self.structure_shapes = if !crate::structure_keys::body_constructs_structure(&self.body) {
            // No `Новый Структура` in this body → no tracked root → skip collection and the Stage-2
            // forwarder entirely (keeps the feature off the hot path for most bodies).
            FxHashMap::default()
        } else {
            use crate::structure_keys::{collect_structure_shapes, SeedRoots};
            // Stage 2 interprocedural forwarding is ordinary-modules only — effective
            // (`&ИзменениеИКонтроль`) and weaving inference use separate symbol contexts and are
            // out of scope, so no forwarder is built there (byte-identical Stage-1 behaviour).
            let ordinary = self.local_symbols.is_none() && self.weaving_base.is_none();
            if ordinary {
                let resolver = self.get_resolver();
                let module = hir_def::ModuleId::new(self.context_file_id);
                // Callee summaries come from the memoised (acyclic) query, so reading them here does
                // not couple inference's fixpoint with a second one.
                let summarize = |mid: hir_def::MethodId| {
                    crate::structure_param_keys::structure_param_keys_query(
                        self.db,
                        hir_def::MethodIdInput::new(self.db, mid),
                    )
                    .clone()
                };
                let forwarder = crate::structure_param_keys::Forwarder::new(
                    self.db, &resolver, module, &self.body, &summarize,
                );
                collect_structure_shapes(&self.body, SeedRoots::NewLiterals, Some(&forwarder))
            } else {
                collect_structure_shapes(&self.body, SeedRoots::NewLiterals, None)
            }
        };

        let stmts: Vec<StmtIdx> = self.body.body_stmts_typed().to_vec();
        self.infer_stmts(&stmts);

        // Exprs the statement walk never reached are lowering leftovers — e.g.
        // the original callee chain a qualified-call rewrite replaced. They
        // are still typed here for the IDE layer (hover/goto resolve through
        // the source map), but they carry no execution context: statement-level
        // `#Если` narrowing never saw them, so an environment verdict from
        // this sweep would double or contradict the one issued during the
        // walk. An empty body mask disables every environment check.
        let walk_env =
            std::mem::replace(&mut self.body_env, hir_def::execution_env::EnvFlags::EMPTY);
        let expr_ids: Vec<ExprId> = self.body.exprs_iter().map(|(id, _)| id).collect();
        for expr_id in expr_ids {
            self.infer_expr(expr_id);
        }
        self.body_env = walk_env;

        debug!(
            "inferred {} expression types, {} var types, {} diagnostics",
            self.expr_types.len(),
            self.var_types.len(),
            self.diagnostics.len()
        );
    }

    fn infer_stmts(&mut self, stmts: &[StmtIdx]) {
        for &stmt_idx in stmts {
            self.infer_stmt(stmt_idx);
        }
    }

    fn infer_stmt(&mut self, stmt_idx: StmtIdx) {
        let stmt = self.body.stmt_idx(stmt_idx).clone();
        match &stmt {
            Stmt::Assign { target, value } => {
                let value_ty = self.infer_expr(ExprId::from_idx(*value));
                let target_id = ExprId::from_idx(*target);
                let mut infer_target = true;

                let target_expr = self.body.expr_idx(*target).clone();
                match &target_expr {
                    Expr::Path(name) => {
                        let resolver = self.get_resolver();
                        let resolved_name = resolver.resolve_name(self.db, name);
                        let existing_module_variable = matches!(
                            resolved_name,
                            Some(hir_def::resolver::Resolution::Variable(_))
                        );
                        let user_shadows = matches!(
                            resolved_name,
                            Some(hir_def::resolver::Resolution::Method(_))
                                | Some(hir_def::resolver::Resolution::Variable(_))
                        ) || self.body_declares_binding(name);
                        let form_self_resolution = if user_shadows {
                            None
                        } else {
                            crate::form_self::resolve_form_self_property(self.db, &resolver, name)
                        };
                        let unheld_collection = bsl_metadata::MdoType::from_plural(name.as_str())
                            .filter(|mdo| mdo.manager_type_prefix().is_some())
                            .filter(|_| !self.manager_collection_shadowed(name));
                        match form_self_resolution {
                            Some(prop) => {
                                if prop.is_readonly {
                                    let form_ty = self.db.platform_object(
                                        crate::form_self::FORM_TYPE_NAME.to_string(),
                                    );
                                    self.push_inference_diagnostic(
                                        InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                            lhs: ExprId::from_idx(*target),
                                            receiver_ty: form_ty,
                                            field_name: name.clone(),
                                        },
                                    );
                                }
                            }
                            None if existing_module_variable => {}
                            // Writing to an unheld metadata-collection name targets a
                            // Global-context property, which the platform refuses — no
                            // local is declared. Type the target as the collection and
                            // stop: the availability check belongs to READING a member,
                            // and the illegal write is a different defect, not yet
                            // reported.
                            None if unheld_collection.is_some() => {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::GlobalPropertyNotWritable {
                                        lhs: target_id,
                                        name: name.clone(),
                                    },
                                );
                                let collection = self
                                    .db
                                    .manager_collection(unheld_collection.expect("just checked"));
                                self.expr_types.insert(target_id, collection);
                                infer_target = false;
                            }
                            None => {
                                let key = name.as_str().fold_lower();
                                let norm_key = NormName::intern(name.as_str());
                                self.assigned_var_names.insert(norm_key);
                                let unknown = self.db.unknown();
                                self.implicit_locals
                                    .entry(key.clone())
                                    .and_modify(|info| {
                                        info.assignments.push(ImplicitLocalAssignment {
                                            target: target_id,
                                            ty: value_ty,
                                        });
                                        if info.ty == unknown && value_ty != unknown {
                                            info.ty = value_ty;
                                        }
                                    })
                                    .or_insert_with(|| ImplicitLocalInfo {
                                        name: name.clone(),
                                        first_assignment: target_id,
                                        ty: value_ty,
                                        assignments: vec![ImplicitLocalAssignment {
                                            target: target_id,
                                            ty: value_ty,
                                        }],
                                    });
                                if !self.is_unknown(value_ty) {
                                    self.var_types.insert(norm_key, value_ty);
                                }
                            }
                        }
                    }
                    Expr::Field { base, field } => {
                        let base_ty = self.infer_expr(ExprId::from_idx(*base));
                        let obj_resolver = crate::object_resolver::DbObjectResolver::new(
                            self.db,
                            self.context_file_id,
                        );
                        let resolver = self.get_resolver();
                        if let Some(info) = crate::form_items::lookup_form_item_field(
                            self.db, &resolver, base_ty, field,
                        ) {
                            if info.is_readonly {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                        lhs: target_id,
                                        receiver_ty: base_ty,
                                        field_name: field.clone(),
                                    },
                                );
                            }
                            self.expr_types.insert(target_id, info.ty);
                            infer_target = false;
                        } else if let Some(info) = crate::field_lookup::lookup_field(
                            self.db,
                            &obj_resolver,
                            base_ty,
                            field,
                        ) {
                            self.push_deprecated_platform_member_diagnostic(
                                target_id, base_ty, field, true,
                            );
                            if let crate::field_enum::FieldOrigin::PlatformProperty { env } =
                                info.origin
                            {
                                self.check_member_env(
                                    target_id,
                                    field,
                                    env,
                                    EnvMemberKind::Property,
                                );
                            }
                            if info.is_readonly {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                        lhs: target_id,
                                        receiver_ty: base_ty,
                                        field_name: field.clone(),
                                    },
                                );
                            }
                            self.expr_types.insert(target_id, info.ty);
                            infer_target = false;
                        }
                    }
                    _ => {}
                }

                if infer_target {
                    self.infer_expr(target_id);
                }
            }

            Stmt::Expr(expr_idx) => {
                self.infer_expr(ExprId::from_idx(*expr_idx));
            }

            Stmt::If(if_stmt) => {
                self.infer_expr(ExprId::from_idx(if_stmt.condition));
                self.infer_stmts(&if_stmt.then_branch);
                for (cond, branch) in if_stmt.elsif_branches.iter() {
                    self.infer_expr(ExprId::from_idx(*cond));
                    self.infer_stmts(branch);
                }
                if let Some(else_branch) = &if_stmt.else_branch {
                    self.infer_stmts(else_branch);
                }
            }

            Stmt::PreprocIf(preproc) => {
                // Availability checks inside a branch see only the
                // environments its condition compiles for; nesting works
                // because each frame restores its own parent mask.
                let parent = self.body_env;
                let mut remaining = parent;
                self.body_env = preproc.condition.narrow_branch(&mut remaining);
                self.infer_stmts(&preproc.then_branch);
                for (idx, (_, _, branch)) in preproc.elsif_branches.iter().enumerate() {
                    self.body_env = match preproc.elsif_conditions.get(idx) {
                        Some(cond) => cond.narrow_branch(&mut remaining),
                        // A branch without a lowered condition (broken
                        // alignment) poisons the rest of the chain,
                        // including #Иначе.
                        None => {
                            remaining = hir_def::execution_env::EnvFlags::EMPTY;
                            remaining
                        }
                    };
                    self.infer_stmts(branch);
                }
                if let Some(else_branch) = &preproc.else_branch {
                    self.body_env = remaining;
                    self.infer_stmts(else_branch);
                }
                self.body_env = parent;
            }

            Stmt::While { condition, body } => {
                self.infer_expr(ExprId::from_idx(*condition));
                self.infer_stmts(body);
            }

            Stmt::For { var, from, to, body } => {
                self.infer_expr(ExprId::from_idx(*from));
                self.infer_expr(ExprId::from_idx(*to));
                let var_name = NormName::intern(self.body.binding_idx(*var).name.as_str());
                let number = self.db.number(None, None);
                self.var_types.insert(var_name, number);
                self.binding_types.insert(BindingId::from_idx(*var), number);
                self.infer_stmts(body);
            }

            Stmt::ForEach { var, collection, body } => {
                let coll_ty = self.infer_expr(ExprId::from_idx(*collection));
                if let Some(elem_ty) =
                    crate::iteration_lookup::resolve_iter_element_ty(self.db, coll_ty)
                {
                    let var_name = NormName::intern(self.body.binding_idx(*var).name.as_str());
                    self.var_types.insert(var_name, elem_ty);
                    self.binding_types.insert(BindingId::from_idx(*var), elem_ty);
                }
                self.infer_stmts(body);
            }

            Stmt::Try { body, except } => {
                self.infer_stmts(body);
                self.infer_stmts(except);
            }

            Stmt::Return { value } => {
                if let Some(expr_idx) = value {
                    let expr_id = ExprId::from_idx(*expr_idx);
                    self.infer_expr(expr_id);
                    self.return_expr_ids.push(expr_id);
                }
            }

            Stmt::Raise { value } => {
                if let Some(expr_idx) = value {
                    self.infer_expr(ExprId::from_idx(*expr_idx));
                }
            }

            Stmt::Execute { expr } => {
                self.infer_expr(ExprId::from_idx(*expr));
            }

            Stmt::AddHandler { event, handler } | Stmt::RemoveHandler { event, handler } => {
                self.infer_expr(ExprId::from_idx(*event));
                self.infer_expr(ExprId::from_idx(*handler));
            }

            Stmt::VarDecl { .. }
            | Stmt::Break
            | Stmt::Continue
            | Stmt::Goto(_)
            | Stmt::Label(_) => {}
        }
    }

    fn try_synthesise_query_projections(&self, args: &[ExprIdx]) -> Arc<[Option<Arc<Projection>>]> {
        let Some(arg_idx) = args.first().copied() else {
            return Arc::from([]);
        };
        let arg_id = ExprId::from_idx(arg_idx);
        if !matches!(self.body.expr(arg_id), Expr::Literal(Literal::String(_))) {
            return Arc::from([]);
        }
        let sdbl_expr_id = SdblExprId { owner: self.owner, expr_id: arg_id };
        let file_id_input = FileIdInput::new(self.db, self.context_file_id);
        let entries = sdbl_hir_for_file_query(self.db, file_id_input);
        let Some((_, pkg)) = entries.iter().find(|(id, _)| *id == sdbl_expr_id) else {
            return Arc::from([]);
        };
        crate::sdbl_bridge::package_to_projections(self.db, pkg).into()
    }

    fn infer_expr(&mut self, expr_id: ExprId) -> TypeId {
        if let Some(ty) = self.expr_types.get(&expr_id) {
            return *ty;
        }

        let expr = self.body.expr(expr_id).clone();
        trace!("inferring expr {:?}: {:?}", expr_id, expr);

        let ty = match &expr {
            Expr::Missing => self.db.unknown(),

            Expr::Literal(lit) => self.infer_literal(lit),

            Expr::Path(name) => self.infer_path_name(name, expr_id),

            Expr::QualifiedPath(_path) => self.db.unknown(),

            Expr::BinaryOp { lhs, rhs, op } => {
                self.infer_binary_op(ExprId::from_idx(*lhs), ExprId::from_idx(*rhs), *op)
            }

            Expr::UnaryOp { expr, op } => self.infer_unary_op(ExprId::from_idx(*expr), *op),

            Expr::Ternary { condition, then_expr, else_expr } => {
                self.infer_expr(ExprId::from_idx(*condition));
                let then_ty = self.infer_expr(ExprId::from_idx(*then_expr));
                let else_ty = self.infer_expr(ExprId::from_idx(*else_expr));

                if then_ty == else_ty {
                    then_ty
                } else {
                    self.db.unknown()
                }
            }

            Expr::Call { callee, args } => {
                let converted_args: Vec<ExprId> =
                    args.iter().map(|&arg| ExprId::from_idx(arg)).collect();
                self.infer_call(ExprId::from_idx(*callee), &converted_args)
            }

            Expr::MethodCall { receiver, method, args } => {
                let receiver_ty = self.infer_expr(ExprId::from_idx(*receiver));
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                match crate::method_lookup::lookup_method(self.db, receiver_ty, method) {
                    Some(info) => {
                        self.check_member_env(expr_id, method, info.env, EnvMemberKind::Method);
                        info.return_ty
                    }
                    None => self.db.unknown(),
                }
            }

            Expr::Index { base, index } => {
                let base_ty = self.infer_expr(ExprId::from_idx(*base));
                self.infer_expr(ExprId::from_idx(*index));

                let base_kind = self.db.lookup_type(base_ty);
                match base_kind {
                    TypeKind::Array(facet) => facet.element.unwrap_or_else(|| self.db.unknown()),
                    TypeKind::QueryBatchResult { per_query } => {
                        let index_expr = self.body.expr(ExprId::from_idx(*index));
                        let projection = const_eval_literal_index(index_expr)
                            .and_then(|i| per_query.get(i).cloned())
                            .flatten();
                        self.db.query_result(projection, ProjectionSource::Unknown)
                    }
                    _ => self.db.unknown(),
                }
            }

            Expr::Field { base, field } => {
                let base_ty = self.infer_expr(ExprId::from_idx(*base));

                let resolver = self.get_resolver();
                let obj_resolver =
                    crate::object_resolver::DbObjectResolver::new(self.db, self.context_file_id);
                if let Some(info) =
                    crate::form_items::lookup_form_item_field(self.db, &resolver, base_ty, field)
                {
                    info.ty
                } else if let Some(info) =
                    crate::field_lookup::lookup_field(self.db, &obj_resolver, base_ty, field)
                {
                    self.push_deprecated_platform_member_diagnostic(expr_id, base_ty, field, true);
                    if let crate::field_enum::FieldOrigin::PlatformProperty { env } = info.origin {
                        self.check_member_env(expr_id, field, env, EnvMemberKind::Property);
                    }
                    info.ty
                } else if let Some(info) = crate::manager_lookup::lookup_manager_field(
                    self.db,
                    &obj_resolver,
                    base_ty,
                    field,
                ) {
                    info.ty
                } else {
                    let base_kind = self.db.lookup_type(base_ty);
                    // A metadata collection is as closed a receiver as an object: its
                    // members are exactly the configuration's objects of that kind, so a
                    // miss is a real defect and not an unknown shape. Reporting it here is
                    // what keeps `Справочники.НетТакого.Метод()` from going silent now that
                    // the chain is no longer folded into one node.
                    //
                    // A name the author has not finished writing (`Справочники.`) is not a
                    // miss: lowering keeps the incomplete field as the missing placeholder,
                    // and accusing the configuration on every keystroke would be noise.
                    if !field.is_missing()
                        && matches!(
                            base_kind,
                            TypeKind::MetadataRef(_)
                                | TypeKind::ThisObject { .. }
                                | TypeKind::ManagerCollection(_)
                        )
                    {
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedField {
                            expr: expr_id,
                            receiver_ty: base_ty,
                            field_name: field.clone(),
                        });
                    }
                    self.db.unknown()
                }
            }

            Expr::New { type_name, args } => {
                for &arg in args.iter() {
                    self.infer_expr(ExprId::from_idx(arg));
                }

                // `Новый("X")` / `Новый(Тип("X"))`: имя типа приходит значением.
                // Список аргументов у этой формы — `(<Тип>, <МассивПараметров>)`,
                // то есть НЕ позиционные аргументы конструктора, поэтому ниже она
                // не участвует ни в привязке к перегрузкам, ни в синтезе проекций
                // запроса — иначе имя типа было бы разобрано как первый параметр.
                let body = Arc::clone(&self.body);
                let dynamic_name = match (type_name, args.first()) {
                    (None, Some(&first)) => self.constructed_type_name(&body, first),
                    _ => None,
                };

                if let Some(name) = type_name {
                    let platform = bsl_platform::PlatformDataInner::instance();
                    let platform_type = platform.get_type(name.as_str());
                    let environment = platform_type.map_or(
                        hir_def::execution_env::EnvFlags::ALL,
                        |platform_type| {
                            hir_def::execution_env::EnvFlags::from_platform_context(
                                platform_type.context.as_ref(),
                            )
                        },
                    );
                    if !platform.is_ambiguous_type_name(name.as_str()) && platform_type.is_some() {
                        self.check_member_env(expr_id, name, environment, EnvMemberKind::Type);
                    }
                    let ctors = platform.get_constructors(name.as_str());
                    if !ctors.is_empty() {
                        if let Ok(candidates) = builtin::candidates::constructor_candidates(
                            self.db,
                            &ctors,
                            environment,
                        ) {
                            let arg_ids: Vec<ExprId> =
                                args.iter().copied().map(ExprId::from_idx).collect();
                            let argument_types = arg_ids
                                .iter()
                                .map(|arg| {
                                    self.expr_types
                                        .get(arg)
                                        .copied()
                                        .unwrap_or_else(|| self.db.unknown())
                                })
                                .collect::<Vec<_>>();
                            let projection = crate::call_binding::resolve_binding(
                                self.db,
                                candidates,
                                &argument_types,
                            );
                            if !self.body.is_recovered(expr_id) {
                                self.call_arg_bindings.push(CallArgBinding {
                                    owner: self.owner,
                                    call_expr: expr_id,
                                    args: arg_ids,
                                    candidate: projection.semantic,
                                });
                            }
                        }
                    }
                }

                if let Some(raw) = dynamic_name {
                    self.check_constructed_type_env(expr_id, raw);
                }

                // Обе формы обязаны сходиться на одном типе, иначе `Новый Запрос`
                // и `Новый("Запрос")` начали бы конфликтовать между собой.
                let ctor_name = type_name.as_ref().map(Name::as_str).or(dynamic_name);
                let is_query_ctor = ctor_name.is_some_and(|name| {
                    crate::method_lookup::is_platform_name_str(name, "Запрос", "Query")
                });
                if is_query_ctor {
                    let projections = match type_name {
                        Some(_) => self.try_synthesise_query_projections(args),
                        None => Arc::from([]),
                    };
                    self.db.query(projections.iter().cloned().collect())
                } else {
                    match (type_name, dynamic_name) {
                        (Some(name), _) => {
                            TyLoweringContext::new().lower_bare_name_id(self.db, name)
                        }
                        (None, Some(raw)) => {
                            crate::lower::type_string::lower_constructed_type_name_typeid(
                                self.db, raw,
                            )
                        }
                        (None, None) => self.db.unknown(),
                    }
                }
            }

            Expr::Array(elements) => {
                for &elem in elements.iter() {
                    self.infer_expr(ExprId::from_idx(elem));
                }

                self.db.array(None)
            }

            Expr::Await { expr } => self.infer_expr(ExprId::from_idx(*expr)),
        };

        self.expr_types.insert(expr_id, ty);
        ty
    }

    fn infer_path_name(&mut self, name: &hir_def::Name, expr_id: ExprId) -> TypeId {
        use hir_def::resolver::Resolution;

        let resolver = self.get_resolver();

        let name_lower = name.as_str().fold_lower();
        let self_name_wins =
            is_self_name(&name_lower) && !self.self_name_is_shadowed(name, &resolver);
        if self_name_wins && (name_lower == "этотобъект" || name_lower == "thisobject") {
            if let Some(owner) = crate::this_object::resolve_this_object_owner(self.db, &resolver) {
                trace!("resolved {} as ThisObject {{ owner: {:?} }}", name, owner);
                return self.db.mk_this_object(
                    ConfigId::Root,
                    MdoRefFacet::new(owner.0, owner.1.as_str().to_string()),
                );
            }
            if let Some(owner) = crate::this_object::resolve_this_manager_owner(self.db, &resolver)
            {
                trace!("resolved {} as ThisManager {{ owner: {:?} }}", name, owner);
                return self.db.mk_this_manager(
                    ConfigId::Root,
                    MdoRefFacet::new(owner.0, owner.1.as_str().to_string()),
                );
            }
            if let Some((mdo, name)) =
                crate::this_object::resolve_this_record_set_owner(self.db, &resolver)
            {
                if let Some(kind) = hir_def::ty::MetadataKind::record_set_kind_for(mdo) {
                    trace!(
                        "resolved {} as record-set MetadataRef {{ {:?}, {} }}",
                        name.as_str(),
                        kind,
                        name
                    );
                    return self.db.metadata_ref(kind, name.as_str().to_string(), &RootConfigCtx);
                }
            }
        }
        // `ЭтаФорма` denotes the form itself just as `ЭтотОбъект` does — the
        // deprecated spelling only earns the `UsingThisForm` hint, not a worse
        // type. The platform's own `ЭтаФорма` entry belongs to the ORDINARY form
        // (`Форма`), so a managed form module finds nothing there and would end
        // up treating the receiver as an unresolved module.
        if self_name_wins && crate::this_object::is_managed_form_module(self.db, &resolver) {
            trace!("resolved {} as managed form Self", name);
            return self.db.platform_object(crate::form_self::FORM_TYPE_NAME.to_string());
        }

        let resolved = resolver.resolve_name(self.db, name);

        if let Some(ty) = self.var_types.get(&NormName::intern(name.as_str())) {
            trace!("resolved {} via var_types = {:?}", name, ty);
            let ty_id = *ty;
            // Structure-literal key enrichment: a local typed as a `Структура` gets its collected
            // literal keys (and value types known up to this read) surfaced for completion/hover.
            // Soft — never feeds a diagnostic.
            if matches!(self.db.lookup_type(ty_id), TypeKind::Structure(_)) {
                let key = name.as_str().fold_lower();
                if let Some(rich) =
                    crate::structure_keys::materialize(self.db, &self.structure_shapes, &key)
                {
                    return rich;
                }
            }
            if crate::method_lookup::receiver_needs_refinement_id(self.db, ty_id) {
                if let Some(projections) = crate::query_text_dataflow::refine_query_at_use_site(
                    self.db,
                    self.context_file_id,
                    self.owner,
                    expr_id,
                    name,
                    &self.body,
                ) {
                    let refined = self.db.query(projections.iter().cloned().collect());
                    trace!("Phase F refined {} to {:?}", name, refined);
                    return refined;
                }
            }
            return ty_id;
        }

        let user_shadows =
            matches!(resolved, Some(Resolution::Method(_)) | Some(Resolution::Variable(_)));
        let body_binding_shadows = self.body_declares_binding(name);

        if user_shadows || body_binding_shadows {
            return self.db.unknown();
        }

        let resolver_says_builtin = matches!(resolved, Some(Resolution::Builtin(_)));
        let hir_sig = builtin::builtin_functions().get(name.as_str());
        if resolver_says_builtin || hir_sig.is_some() {
            return self.db.unknown();
        }

        if !user_shadows && !body_binding_shadows {
            if let Some(resolution) =
                crate::form_self::resolve_form_self_property(self.db, &resolver, name)
            {
                trace!("resolved {} as managed-form Self property", name);
                return resolution.return_ty;
            }
        }

        if !user_shadows && !body_binding_shadows {
            if let Some(ty) = crate::form_attr::resolve_form_attribute(self.db, &resolver, name) {
                trace!("resolved {} as managed-form attribute", name);
                return ty;
            }
        }

        let workspace_owns_common_module = resolver.user_common_module_exists(self.db, name);

        // Implicit `ЭтотОбъект` members (attributes, tabular sections, record-set fields) of the
        // current object/record-set module win over a same-named common module: in that module
        // the bare name denotes the object's own member, not the workspace module. Resolution
        // returns `None` outside an object/record-set module, so a plain common-module name (no
        // colliding member) falls through to the common-module typing below.
        if !user_shadows && !body_binding_shadows {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_object_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit ЭтотОбъект.{} member", name, name);
                return ty;
            }
        }

        if !user_shadows && !body_binding_shadows {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_record_set_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit record-set ЭтотОбъект.{} member", name, name);
                return ty;
            }
        }

        // A bare common-module reference (`ОбщегоНазначения`, used directly) is a value of that
        // module's type, so member calls / hover / completion resolve against its API. Object
        // members above already won; the remaining shadow guard mirrors
        // `dispatch_bare_ident_field_call` (local assignment, declared binding, user
        // method/variable, form attribute / form-self resolved earlier). A module named like a
        // builtin global bailed to Unknown at the builtin check and is resolved by name dispatch.
        if workspace_owns_common_module
            && !user_shadows
            && !body_binding_shadows
            && !self.assigned_var_names.contains(&NormName::intern(name.as_str()))
        {
            let source_root_id =
                self.db.file_source_root_input(self.context_file_id).source_root_id(self.db);
            if let Some(canonical) =
                self.db.module_index(source_root_id).canonical_common_module_name(name)
            {
                trace!("resolved {} as common module type", name);
                return self.db.common_module(canonical.to_string(), ConfigId::Root);
            }
        }

        // Everything below denotes a PLATFORM global, so a user symbol holding the
        // name rules them all out. Declared bindings, module variables/methods,
        // form and `ЭтотОбъект` members have already returned above; what remains
        // is an implicit local written earlier in this body and a workspace common
        // module.
        let assigned_in_body = self.assigned_var_names.contains(&NormName::intern(name.as_str()));
        let user_holds_name = workspace_owns_common_module || assigned_in_body;

        // Metadata collections are the exception, and it is not a stylistic one: a
        // collection name is a Global-context PROPERTY, and assigning to it does not
        // declare a local — the platform refuses the write ("property is not
        // writable") — so the name keeps denoting the collection throughout the body.
        // Only a declared owner takes it, and every declared owner has returned
        // above. `manager_collection_shadowed` judges the same way, so this typing
        // and the availability diagnostic stay in agreement.
        if !workspace_owns_common_module {
            if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(name.as_str()) {
                if mdo_type.manager_type_prefix().is_some() {
                    trace!("resolved {} as manager collection {:?}", name, mdo_type);
                    self.check_manager_collection_env(expr_id, name, mdo_type);
                    return self.db.manager_collection(mdo_type);
                }
            }
        }

        if !user_holds_name {
            if let Some((id, env)) =
                crate::platform_global_lookup::resolve_platform_global_property(self.db, name)
            {
                trace!("resolved {} as platform global → {:?}", name, id);
                self.check_member_env(expr_id, name, env, EnvMemberKind::GlobalProperty);
                return id;
            }
        }

        if !user_holds_name {
            if let Some(id) =
                crate::platform_global_lookup::resolve_platform_system_enum_type(self.db, name)
            {
                trace!("resolved {} as platform system enum → {:?}", name, id);
                return id;
            }
        }

        match resolved {
            Some(Resolution::Method(_)) | Some(Resolution::Variable(_)) => self.db.unknown(),
            Some(Resolution::Builtin(_)) | Some(Resolution::Local(_)) | None => self.db.unknown(),
        }
    }

    fn infer_literal(&self, lit: &Literal) -> TypeId {
        match lit {
            Literal::Number(_) => self.db.number(None, None),
            Literal::String(_) => self.db.string(None, false),
            Literal::Date(_) => self.db.date(DateComponent::DateTime),
            Literal::Bool(_) => self.db.boolean(),
            Literal::Undefined => self.db.undefined(),
            Literal::Null => self.db.null(),
        }
    }

    fn infer_binary_op(&mut self, lhs: ExprId, rhs: ExprId, op: BinaryOp) -> TypeId {
        let lhs_ty = self.infer_expr(lhs);
        let rhs_ty = self.infer_expr(rhs);

        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                let lhs_kind = self.db.lookup_type(lhs_ty);
                let rhs_kind = self.db.lookup_type(rhs_ty);
                if op == BinaryOp::Add
                    && (matches!(lhs_kind, TypeKind::String(_))
                        || matches!(rhs_kind, TypeKind::String(_)))
                {
                    self.db.string(None, false)
                } else if matches!(lhs_kind, TypeKind::Number(_))
                    && matches!(rhs_kind, TypeKind::Number(_))
                {
                    self.db.number(None, None)
                } else {
                    self.db.unknown()
                }
            }

            BinaryOp::Eq
            | BinaryOp::Neq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge => self.db.boolean(),

            BinaryOp::And | BinaryOp::Or => self.db.boolean(),
        }
    }

    fn infer_unary_op(&mut self, expr: ExprId, op: UnaryOp) -> TypeId {
        let expr_ty = self.infer_expr(expr);

        match op {
            UnaryOp::Neg | UnaryOp::Plus => {
                let kind = self.db.lookup_type(expr_ty);
                if matches!(kind, TypeKind::Number(_)) {
                    self.db.number(None, None)
                } else {
                    self.db.unknown()
                }
            }
            UnaryOp::Not => self.db.boolean(),
        }
    }

    fn infer_call(&mut self, callee: ExprId, args: &[ExprId]) -> TypeId {
        let callee_expr = self.body.expr(callee);
        if let Expr::QualifiedPath(qualified_path) = callee_expr {
            match qualified_path.segments().len() {
                3 => {
                    let mdo_type_plural = qualified_path.segments()[0].clone();
                    let mdo_name = qualified_path.segments()[1].clone();
                    let method_name = qualified_path.segments()[2].clone();
                    // Lowering folds the whole chain into one node, so the root
                    // never reaches `infer_path_name` and never meets the
                    // barrier that keeps a held name from denoting a platform
                    // global. It is applied here instead, on the only entry
                    // where the root is spelled out in source: the other caller
                    // synthesises the plural from the enclosing manager module,
                    // where no such spelling exists to be shadowed.
                    if self.manager_collection_shadowed(&mdo_type_plural) {
                        for arg in args {
                            self.infer_expr(*arg);
                        }
                        return self.db.unknown();
                    }
                    return self.infer_three_level_call(
                        &mdo_type_plural,
                        &mdo_name,
                        &method_name,
                        args,
                        callee,
                    );
                }
                other => {
                    debug_assert!(
                        false,
                        "unexpected QualifiedPath segment count {other} in infer_call",
                    );
                    tracing::debug!(
                        segments = other,
                        ?qualified_path,
                        "QualifiedPath with unexpected segment count reached infer_call; \
                         falling through to Ty::Unknown"
                    );
                }
            }
        }

        if let Expr::Field { base, field } = callee_expr {
            let base_id = ExprId::from_idx(*base);
            let method_name = field.clone();

            let receiver_ty = self.infer_expr(base_id);

            if self.is_unknown(receiver_ty) {
                let base_expr = self.body.expr(base_id).clone();
                if let Expr::Path(path_name) = base_expr {
                    if let Some(return_ty) =
                        self.dispatch_bare_ident_field_call(&path_name, &method_name, args, callee)
                    {
                        self.expr_types.insert(callee, self.db.unknown());
                        return return_ty;
                    }
                }
            }

            for arg in args {
                self.infer_expr(*arg);
            }

            let workspace_receiver_ty =
                crate::this_object::coerce_to_metadata_ref_id(self.db, receiver_ty)
                    .unwrap_or(receiver_ty);
            let workspace_receiver_kind = self.db.lookup_type(workspace_receiver_ty);
            if let TypeKind::CommonModule(facet) = &workspace_receiver_kind {
                let module = hir_def::Name::new(&facet.name);
                // A bare module name as receiver (not a variable that holds a module) keeps the
                // self-qualified-access and missed-parameter diagnostics that the name dispatch
                // emits — the handlers filter further (TwoLevel only fires when the called module
                // is the current one). A variable is in `assigned_var_names`, so it is excluded.
                let mut static_receiver = false;
                if let Expr::Path(recv) = self.body.expr(base_id) {
                    let recv = recv.clone();
                    if !self.assigned_var_names.contains(&NormName::intern(recv.as_str()))
                        && !self.body_declares_binding(&recv)
                    {
                        static_receiver = true;
                        let arg_presence: Vec<bool> = args
                            .iter()
                            .map(|arg_id| !matches!(self.body.expr(*arg_id), Expr::Missing))
                            .collect();
                        self.push_inference_diagnostic(
                            InferenceDiagnostic::RedundantAccessToObjectTwoLevel {
                                expr: callee,
                                module: module.clone(),
                            },
                        );
                        self.push_inference_diagnostic(
                            InferenceDiagnostic::MissedRequiredParameterCommonModule {
                                expr: callee,
                                callee: method_name.clone(),
                                module: module.clone(),
                                args: arg_presence,
                            },
                        );
                    }
                }
                let return_ty =
                    self.infer_qualified_call(&module, &method_name, args, callee, static_receiver);
                self.expr_types.insert(callee, self.db.unknown());
                return return_ty;
            }
            if let TypeKind::MetadataRef(facet) = workspace_receiver_kind {
                let kind = facet.kind;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_object_module_call(
                    self.db,
                    kind,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, workspace_receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        self.expr_types.insert(callee, self.db.unknown());
                        let return_ty = self
                            .effective_local_return(resolution.method_id, resolution.return_type);
                        let Ok(candidates) = crate::user_call_candidates::for_resolved_method(
                            self.db,
                            &method_name,
                            resolution.method_id,
                            return_ty,
                        ) else {
                            return self.db.unknown();
                        };
                        return self.record_candidate_call_arg_binding(callee, args, candidates);
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {}
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        unreachable!(
                            "resolve_object_module_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            let receiver_kind = self.db.lookup_type(receiver_ty);
            if let TypeKind::MetadataRef(facet) = receiver_kind {
                let kind = facet.kind;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_record_set_module_call(
                    self.db,
                    kind,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        self.expr_types.insert(callee, self.db.unknown());
                        let return_ty = self
                            .effective_local_return(resolution.method_id, resolution.return_type);
                        let Ok(candidates) = crate::user_call_candidates::for_resolved_method(
                            self.db,
                            &method_name,
                            resolution.method_id,
                            return_ty,
                        ) else {
                            return self.db.unknown();
                        };
                        return self.record_candidate_call_arg_binding(callee, args, candidates);
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {}
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        unreachable!(
                            "resolve_record_set_module_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            let manager_receiver_ty =
                crate::this_object::coerce_to_metadata_ref_id(self.db, receiver_ty)
                    .unwrap_or(receiver_ty);
            let manager_receiver_kind = self.db.lookup_type(manager_receiver_ty);
            if let TypeKind::ObjectManager(facet) = manager_receiver_kind {
                let mdo_type = facet.mdo;
                let mdo_name = hir_def::Name::new(&facet.name);
                let resolver = self.get_resolver();
                match crate::method_resolution::resolve_aliased_manager_call(
                    self.db,
                    mdo_type,
                    &mdo_name,
                    &method_name,
                    &resolver,
                ) {
                    Ok(resolution) => {
                        let receiver_name = receiver_display_name(self.db, receiver_ty)
                            .unwrap_or_else(|| mdo_name.clone());
                        if !resolution.is_export {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::UnresolvedMethodCall {
                                    expr: callee,
                                    receiver_name: receiver_name.clone(),
                                    method_name: method_name.clone(),
                                    kind: UnresolvedMethodKind::MethodNotExport,
                                },
                            );
                        }
                        self.expr_types.insert(callee, self.db.unknown());
                        let return_ty = self
                            .effective_local_return(resolution.method_id, resolution.return_type);
                        let Ok(candidates) = crate::user_call_candidates::for_resolved_method(
                            self.db,
                            &method_name,
                            resolution.method_id,
                            return_ty,
                        ) else {
                            return self.db.unknown();
                        };
                        return self.record_candidate_call_arg_binding(callee, args, candidates);
                    }
                    Err(UnresolvedMethodKind::MethodNotFound) => {}
                    Err(
                        kind @ (UnresolvedMethodKind::MethodNotExport
                        | UnresolvedMethodKind::CommonModuleNoSource
                        | UnresolvedMethodKind::ReceiverNotResolved),
                    ) => {
                        unreachable!(
                            "resolve_aliased_manager_call returned unexpected kind: {:?}",
                            kind
                        )
                    }
                }
            }

            let refine_ctx = crate::method_lookup::RefineCtx {
                db: self.db,
                file_id: self.context_file_id,
                owner: self.owner,
                body: &self.body,
                dispatch_expr_id: callee,
                receiver_expr_id: base_id,
                call_args: args,
            };
            let result = match crate::method_lookup::lookup_method_with_refinement(
                self.db,
                receiver_ty,
                &method_name,
                Some(&refine_ctx),
            ) {
                Some(info) => {
                    self.push_deprecated_platform_member_diagnostic(
                        callee,
                        receiver_ty,
                        &method_name,
                        false,
                    );
                    self.check_member_env(callee, &method_name, info.env, EnvMemberKind::Method);
                    let mut candidates = info.candidates;
                    let manager_receiver_kind = self.db.lookup_type(manager_receiver_ty);
                    if let TypeKind::ObjectManager(facet) = manager_receiver_kind {
                        if facet.mdo == bsl_metadata::MdoType::Constant {
                            let mdo_name = hir_def::Name::new(&facet.name);
                            self.refine_constant_candidates(
                                &mdo_name,
                                &method_name,
                                &mut candidates,
                            );
                        }
                    }
                    self.record_candidate_call_arg_binding(callee, args, candidates)
                }
                None => {
                    // `ЭтотОбъект.Метод()` in a managed form module. The form type
                    // carries no such platform member, so the module's own methods
                    // are the receiver's remaining surface — resolve and judge them
                    // exactly as the equivalent bare call, and report the miss:
                    // unlike a bare name, a self receiver has nowhere else to look.
                    if let Some(self_name) = self.form_self_receiver_name(base_id) {
                        let result = self
                            .infer_local_method_call(&method_name, args, callee)
                            .unwrap_or_else(|| {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::UnresolvedMethodCall {
                                        expr: callee,
                                        receiver_name: self_name,
                                        method_name: method_name.clone(),
                                        kind: UnresolvedMethodKind::MethodNotFound,
                                    },
                                );
                                self.db.unknown()
                            });
                        self.expr_types.insert(callee, self.db.unknown());
                        return result;
                    }
                    if self.method_resolves_on_alternate_assignment(
                        base_id,
                        receiver_ty,
                        &method_name,
                    ) {
                        // Inference is sequential, so at this use the variable
                        // carries the type of its textually-last assignment even
                        // when that assignment lives in a sibling branch that
                        // cannot reach this use. If any other recorded assignment
                        // type resolves the method, the receiver type is a
                        // cross-branch artefact — stay silent instead of
                        // reporting a false unresolved call.
                    } else if let Some(receiver_name) = receiver_display_name(self.db, receiver_ty)
                    {
                        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                            expr: callee,
                            receiver_name,
                            method_name: method_name.clone(),
                            kind: UnresolvedMethodKind::MethodNotFound,
                        });
                    } else if matches!(
                        self.db.lookup_type(receiver_ty),
                        TypeKind::PlatformObject(_)
                    ) {
                        let base_expr = self.body.expr(base_id).clone();
                        if let Expr::Path(receiver_path_name) = base_expr {
                            if matches!(
                                crate::platform_global_lookup::try_resolve_platform_global_member(
                                    self.db,
                                    &receiver_path_name,
                                    &method_name,
                                ),
                                crate::platform_global_lookup::PlatformGlobalLookup::KnownContainerMissingMember
                            ) {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::UnresolvedMethodCall {
                                        expr: callee,
                                        receiver_name: receiver_path_name,
                                        method_name: method_name.clone(),
                                        kind: UnresolvedMethodKind::MethodNotFound,
                                    },
                                );
                            }
                        }
                    }
                    self.db.unknown()
                }
            };
            self.expr_types.insert(callee, self.db.unknown());
            return result;
        }

        let bare_callee_name: Option<hir_def::Name> = match callee_expr {
            Expr::Path(n) => Some(n.clone()),
            _ => None,
        };

        // A global common module's exported method is callable unqualified — it extends
        // the global context, so it shadows a same-named platform global function. A
        // same-module method (checked here) still wins, keeping Local → Module → Global-CM
        // → Platform precedence.
        if let Some(name) = &bare_callee_name {
            if !self.body_declares_binding(name)
                && !self.assigned_var_names.contains(&NormName::intern(name.as_str()))
                && !self.bare_module_method_exists(name)
            {
                if let Some(ret) = self.resolve_bare_global_export(name, args, callee) {
                    return ret;
                }
            }
        }

        if let Some(name) = &bare_callee_name {
            let name = name.clone();
            if let Some(sigs) = builtin::builtin_functions().get(name.as_str()) {
                debug_assert!(
                    !sigs.is_empty(),
                    "BuiltinFunctions::get must never return an empty overload set"
                );

                if let Some(first) = sigs.first() {
                    self.check_member_env(
                        callee,
                        &name,
                        first.env(),
                        EnvMemberKind::GlobalFunction,
                    );
                }

                for arg in args {
                    self.infer_expr(*arg);
                }
                let Some(callable) = builtin::builtin_functions().callable_id(name.as_str()) else {
                    return self.db.unknown();
                };
                let Ok(mut candidates) = crate::call_resolution::CallCandidateSet::try_from(
                    sigs.iter()
                        .enumerate()
                        .map(|(ordinal, signature)| {
                            signature.to_call_signature(self.db, callable, ordinal)
                        })
                        .collect::<Vec<_>>(),
                ) else {
                    return self.db.unknown();
                };
                // Weaving `&Вместо`: a bare `ПродолжитьВызов(...)` invokes the original base
                // method, so (a) its result is that method's return type, not the platform
                // global's generic return, and (b) it must pass a valid argument count for that
                // method — the platform global is variadic, so the arity check above never fires
                // and the base-method arity is enforced here instead. Both apply only inside a
                // `&Вместо` interceptor (`proceed_*` are `None` everywhere else → no change).
                if is_proceed_with_call_name(&name) {
                    if let Some(proceed_ty) = self.proceed_return {
                        for candidate in candidates.signatures_mut() {
                            candidate.return_ty = proceed_ty;
                        }
                    }
                }
                let ret = self.record_candidate_call_arg_binding(callee, args, candidates);
                if is_proceed_with_call_name(&name) {
                    if let Some((required, total)) = self.proceed_arity {
                        if args.len() < required || args.len() > total {
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: callee,
                                    required_count: required,
                                    total_count: total,
                                    found: args.len(),
                                },
                            );
                        }
                    }
                }
                self.expr_types.insert(callee, self.db.unknown());
                return ret;
            }
        }

        let callee_ty = self.infer_expr(callee);

        for arg in args {
            self.infer_expr(*arg);
        }

        let callee_kind = self.db.lookup_type(callee_ty);
        match callee_kind {
            TypeKind::Function(facet) => {
                let candidates =
                    crate::call_resolution::CallCandidateSet::from_function_facet(facet);
                self.record_candidate_call_arg_binding(callee, args, candidates);
                facet.returns
            }
            TypeKind::Unknown => {
                if let Some(name) = bare_callee_name.as_ref() {
                    if !self.body_declares_binding(name)
                        && !self.assigned_var_names.contains(&NormName::intern(name.as_str()))
                    {
                        if let Some(return_ty) = self.infer_local_method_call(name, args, callee) {
                            return return_ty;
                        }
                    }
                }
                self.db.unknown()
            }
            _ => self.db.unknown(),
        }
    }

    /// Resolves a call against the methods of the module being inferred, judges it
    /// against the caller's compilation directive and binds its arguments.
    ///
    /// `None` means the module declares no such method — what that means is the
    /// caller's decision: a bare name may still be a global function or a common
    /// module export, while a self-qualified receiver has no surface left.
    fn infer_local_method_call(
        &mut self,
        name: &hir_def::Name,
        args: &[ExprId],
        callee: ExprId,
    ) -> Option<TypeId> {
        let symbol_tree = match &self.local_symbols {
            Some(symbols) => Arc::clone(symbols),
            None => {
                let module_id = hir_def::ModuleId::new(self.context_file_id);
                self.db.symbol_tree(module_id)
            }
        };
        // Weaving: a `&Вместо`/`&Перед`/`&После` interceptor calling a base
        // sibling that the extension does not define falls back to the paired
        // base module's symbols (the extension shadows the base). The base tree
        // is bound here so the borrowed method outlives the lookup.
        let base_tree = self.weaving_base.map(|base| self.db.symbol_tree_ref(base));
        let method = symbol_tree
            .find_method(name)
            .or_else(|| base_tree.as_ref().and_then(|base_tree| base_tree.find_method(name)))?;

        // Weaving-base fallbacks resolve into another file whose item tree does
        // not match `context_file_id` — the local directive check only makes
        // sense for true siblings.
        if method.id.module.file_id == self.context_file_id {
            let method_name = method.name.clone();
            let local_id = method.id.local_id;
            self.check_local_callee_env(callee, &method_name, local_id);
        }
        // In effective (`&ИзменениеИКонтроль`) inference, prefer the CHANGED
        // body's return over the base-keyed query, so inserted code that consumes
        // a changed sibling's result types correctly.
        let effective_ret = self
            .local_effective_returns
            .as_ref()
            .and_then(|m| m.get(&method.id.local_id).copied())
            .filter(|ret| !self.is_unknown(*ret));
        let sig =
            crate::method_resolution::materialise_signature_enriched(self.db, method.id, method);
        let return_ty = effective_ret.unwrap_or(sig.ret);
        let Ok(candidates) =
            crate::user_call_candidates::for_resolved_method(self.db, name, method.id, return_ty)
        else {
            return Some(self.db.unknown());
        };
        Some(self.record_candidate_call_arg_binding(callee, args, candidates))
    }

    /// An ordinary symbol with a self name's spelling shadows the predefined
    /// meaning: a parameter, a `Перем` in the body or at module level, an
    /// assignment target, a user method. Typing such a receiver as the module's
    /// own object or form would invent members it does not have and lose every
    /// check that depends on its real type. Mirrors the `user_shadows` /
    /// `body_binding_shadows` rule the rest of the name cascade uses.
    fn self_name_is_shadowed(&self, name: &hir_def::Name, resolver: &Resolver) -> bool {
        self.body_declares_binding(name)
            || self.assigned_var_names.contains(&NormName::intern(name.as_str()))
            || matches!(
                resolver.resolve_name(self.db, name),
                Some(
                    hir_def::resolver::Resolution::Method(_)
                        | hir_def::resolver::Resolution::Variable(_)
                )
            )
    }

    /// The receiver is the form module's own self reference, written literally.
    ///
    /// The test is on the spelling, not on the inferred type: a parameter or
    /// variable that merely holds a `ФормаКлиентскогоПриложения` came from
    /// somewhere else, and resolving it against THIS module's methods would
    /// invent members it does not have.
    fn form_self_receiver_name(&self, base_id: ExprId) -> Option<hir_def::Name> {
        let Expr::Path(name) = self.body.expr(base_id) else { return None };
        let name = name.clone();
        if !is_self_name(&name.as_str().fold_lower()) {
            return None;
        }
        let resolver = self.get_resolver();
        if self.self_name_is_shadowed(&name, &resolver) {
            return None;
        }
        crate::this_object::is_managed_form_module(self.db, &resolver).then_some(name)
    }

    /// `static_receiver` — the call names the module directly
    /// (`Модуль.Метод()`), as opposed to a variable holding a module value
    /// (`М = ОбщегоНазначения.ОбщийМодуль(...); М.Метод()`). Flow-insensitive
    /// typing keeps only the LAST module assigned to such a variable, so
    /// accessibility verdicts against it would misfire on the common
    /// per-`#Если`-branch module selection idiom.
    fn infer_qualified_call(
        &mut self,
        module_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
        static_receiver: bool,
    ) -> TypeId {
        for arg in args {
            self.infer_expr(*arg);
        }

        let resolver = self.get_resolver();

        match method_resolution::resolve_qualified_call(
            self.db,
            module_name,
            method_name,
            &resolver,
        ) {
            Ok(resolution) => {
                if !resolution.is_export {
                    self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: module_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                if static_receiver && resolution.is_export {
                    self.check_common_module_callee_env(
                        call_expr,
                        Some(module_name),
                        resolution.method_id.module,
                    );
                }

                // `ОбщегоНазначения.ОбщийМодуль("Имя")` returns the named common module as a
                // value. Its own inferred return is uninformative — `Unknown` from `Вычислить`,
                // or `Undefined` because the real БСП body has an `Иначе Модуль = Неопределено`
                // arm that wins under flow-insensitive typing. Narrowing on either gives the
                // receiver a `CommonModule` type for member resolution / hover / completion,
                // without overriding a method that declares a genuinely useful return type.
                let mut return_ty = resolution.return_type;
                if (self.is_unknown(return_ty) || self.is_undefined(resolution.return_type))
                    && method_name.as_str().fold_lower() == "общиймодуль"
                {
                    if let Some(ty) = self.common_module_type_from_args(args) {
                        return_ty = ty;
                    }
                }
                let Ok(candidates) = crate::user_call_candidates::for_resolved_method(
                    self.db,
                    method_name,
                    resolution.method_id,
                    return_ty,
                ) else {
                    return self.db.unknown();
                };
                self.record_candidate_call_arg_binding(call_expr, args, candidates)
            }
            Err(kind) => {
                if matches!(kind, UnresolvedMethodKind::MethodNotFound) {
                    let source_root_id = self
                        .db
                        .file_source_root_input(self.context_file_id)
                        .source_root_id(self.db);
                    let module_in_workspace = self
                        .db
                        .module_index(source_root_id)
                        .resolve_common_module(module_name)
                        .is_some();

                    if !module_in_workspace {
                        if let crate::platform_global_lookup::PlatformGlobalLookup::Resolved {
                            ty: return_ty,
                            env,
                        } = crate::platform_global_lookup::try_resolve_platform_global_member(
                            self.db,
                            module_name,
                            method_name,
                        ) {
                            self.check_member_env(
                                call_expr,
                                method_name,
                                env,
                                EnvMemberKind::Method,
                            );
                            self.expr_types.insert(call_expr, self.db.unknown());
                            return return_ty;
                        }
                    }
                }

                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name: module_name.clone(),
                    method_name: method_name.clone(),
                    kind,
                });

                self.db.unknown()
            }
        }
    }

    /// Type of `ОбщийМодуль("Имя")` when the single argument is a plain (non-dotted) string
    /// literal naming a known common module: `Some(CommonModule{canonical_name})`. The dotted
    /// form (`"Справочники.Имя"`, a manager) and unknown names return `None`.
    fn common_module_type_from_args(&self, args: &[ExprId]) -> Option<TypeId> {
        let [arg] = args else {
            return None;
        };
        let Expr::Literal(Literal::String(literal)) = self.body.expr(*arg) else {
            return None;
        };
        if literal.contains('.') {
            return None;
        }
        let name = Name::new(literal);
        // Only narrow to a module visible from this file's configuration — the same guard the
        // resolver applies, so the type and later member resolution agree on one module rather
        // than typing against a source-root sibling hidden in the current config.
        if !self.get_resolver().user_common_module_exists(self.db, &name) {
            return None;
        }
        let source_root_id =
            self.db.file_source_root_input(self.context_file_id).source_root_id(self.db);
        let module_index = self.db.module_index(source_root_id);
        let canonical = module_index.canonical_common_module_name(&name)?;
        Some(self.db.common_module(canonical.to_string(), ConfigId::Root))
    }

    fn dispatch_bare_ident_field_call(
        &mut self,
        module_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
    ) -> Option<TypeId> {
        use hir_def::resolver::Resolution;

        let resolver = self.get_resolver();

        match resolver.resolve_name(self.db, module_name) {
            Some(Resolution::Local(_) | Resolution::Variable(_) | Resolution::Method(_)) => {
                return None;
            }
            Some(Resolution::Builtin(_)) | None => {}
        }

        if self.body_declares_binding(module_name) {
            return None;
        }
        if self.assigned_var_names.contains(&NormName::intern(module_name.as_str())) {
            return None;
        }

        // A form attribute shadows module and global names for a bare receiver.
        // A typed attribute never reaches this dispatch (its receiver infers to
        // a real type); an untyped one (empty <Type/> in Form.xml) lowers to
        // Unknown and lands here — it is still a form attribute, not an
        // unresolved module, so stay silent per gradual typing.
        if crate::form_attr::resolve_form_attribute(self.db, &resolver, module_name).is_some() {
            for arg in args {
                self.infer_expr(*arg);
            }
            return Some(self.db.unknown());
        }

        if resolver.user_common_module_exists(self.db, module_name) {
            let arg_presence: Vec<bool> = args
                .iter()
                .map(|arg_id| !matches!(self.body.expr(*arg_id), Expr::Missing))
                .collect();

            self.push_inference_diagnostic(InferenceDiagnostic::RedundantAccessToObjectTwoLevel {
                expr: call_expr,
                module: module_name.clone(),
            });

            self.push_inference_diagnostic(
                InferenceDiagnostic::MissedRequiredParameterCommonModule {
                    expr: call_expr,
                    callee: method_name.clone(),
                    module: module_name.clone(),
                    args: arg_presence,
                },
            );

            return Some(self.infer_qualified_call(
                module_name,
                method_name,
                args,
                call_expr,
                true,
            ));
        }

        // A manager module that calls one of its own methods through the object's
        // own name (`ОбъектМетаданных.Метод()` for objects accessed without a
        // collection prefix, e.g. data processors and reports) is a redundant
        // self-qualified access — the method is reachable directly. The handler
        // confirms the name is this module's own and that the metadata kind is one
        // accessed without a collection prefix.
        if let Some((mdo_type, self_name)) =
            crate::this_object::resolve_this_manager_owner(self.db, &resolver)
        {
            if module_name.as_str().eq_ignore_ascii_case(self_name.as_str()) {
                self.push_inference_diagnostic(
                    InferenceDiagnostic::RedundantAccessToObjectTwoLevel {
                        expr: call_expr,
                        module: module_name.clone(),
                    },
                );
                // Resolve the call as the equivalent collection-qualified call so the
                // method is still validated (a misspelled self-method keeps its
                // unresolved-call diagnostic) and the return type stays precise.
                if let Some(plural) = mdo_type.russian_plural() {
                    return Some(self.infer_three_level_call(
                        &Name::new(plural),
                        &self_name,
                        method_name,
                        args,
                        call_expr,
                    ));
                }
                for arg in args {
                    self.infer_expr(*arg);
                }
                return Some(self.db.unknown());
            }
        }

        match crate::platform_global_lookup::try_resolve_platform_global_member(
            self.db,
            module_name,
            method_name,
        ) {
            crate::platform_global_lookup::PlatformGlobalLookup::Resolved {
                ty: return_ty,
                env,
            } => {
                for arg in args {
                    self.infer_expr(*arg);
                }
                self.check_member_env(call_expr, method_name, env, EnvMemberKind::Method);
                self.expr_types.insert(call_expr, self.db.unknown());
                return Some(return_ty);
            }
            crate::platform_global_lookup::PlatformGlobalLookup::KnownContainerMissingMember => {
                for arg in args {
                    self.infer_expr(*arg);
                }
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name: module_name.clone(),
                    method_name: method_name.clone(),
                    kind: UnresolvedMethodKind::MethodNotFound,
                });
                return Some(self.db.unknown());
            }
            crate::platform_global_lookup::PlatformGlobalLookup::NotAContainer => {}
        }

        for arg in args {
            self.infer_expr(*arg);
        }
        self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
            expr: call_expr,
            receiver_name: module_name.clone(),
            method_name: method_name.clone(),
            kind: UnresolvedMethodKind::ReceiverNotResolved,
        });
        Some(self.db.unknown())
    }

    fn infer_three_level_call(
        &mut self,
        mdo_type_plural: &Name,
        mdo_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
    ) -> TypeId {
        for arg in args {
            self.infer_expr(*arg);
        }

        let mdo_type_opt = bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str());

        // The call is lowered as one QualifiedPath, so the collection root
        // (`Справочники`, `Перечисления`, …) never passes through
        // `infer_path_name`; its availability is checked here instead. The
        // verdict does not depend on whether the method resolves — the root
        // itself is what the restricted environments lack.
        if let Some(mdo_type) = mdo_type_opt {
            self.check_manager_collection_env(call_expr, mdo_type_plural, mdo_type);
        }

        let resolver = self.get_resolver();
        let receiver_name =
            Name::new(&format!("{}.{}", mdo_type_plural.as_str(), mdo_name.as_str()));

        match method_resolution::resolve_three_level_call(
            self.db,
            mdo_type_plural,
            mdo_name,
            method_name,
            &resolver,
        ) {
            Ok(resolution) => {
                if !resolution.is_export {
                    self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                        expr: call_expr,
                        receiver_name: receiver_name.clone(),
                        method_name: method_name.clone(),
                        kind: UnresolvedMethodKind::MethodNotExport,
                    });
                }

                let Ok(candidates) = crate::user_call_candidates::for_resolved_method(
                    self.db,
                    method_name,
                    resolution.method_id,
                    resolution.return_type,
                ) else {
                    return self.db.unknown();
                };
                self.record_candidate_call_arg_binding(call_expr, args, candidates)
            }
            Err(UnresolvedMethodKind::MethodNotFound) => {
                let plat_res: Option<PlatformMethodResolution> = mdo_type_opt
                    .filter(|mdo_type| self.mdo_declared(*mdo_type, mdo_name))
                    .and_then(|mdo_type| {
                        resolve_platform_manager_method(self.db, mdo_type, mdo_name, method_name)
                    });
                if let Some(res) = plat_res {
                    let mut candidates = res.candidates;
                    if mdo_type_opt == Some(bsl_metadata::MdoType::Constant) {
                        self.refine_constant_candidates(mdo_name, method_name, &mut candidates);
                    }
                    return self.record_candidate_call_arg_binding(call_expr, args, candidates);
                }

                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind: UnresolvedMethodKind::MethodNotFound,
                });
                self.db.unknown()
            }
            Err(kind) => {
                self.push_inference_diagnostic(InferenceDiagnostic::UnresolvedMethodCall {
                    expr: call_expr,
                    receiver_name,
                    method_name: method_name.clone(),
                    kind,
                });
                self.db.unknown()
            }
        }
    }

    fn mdo_declared(&self, mdo_type: bsl_metadata::MdoType, mdo_name: &Name) -> bool {
        let needle = mdo_name.as_str();
        self.db.resolve_metadata_object(self.context_file_id, mdo_type, needle).is_some()
            || self.db.resolve_register(self.context_file_id, mdo_type, needle).is_some()
    }

    fn resolve_constant_value_type(&self, mdo_name: &Name) -> Option<TypeId> {
        let mdo = self.db.resolve_metadata_object(
            self.context_file_id,
            bsl_metadata::MdoType::Constant,
            mdo_name.as_str(),
        )?;
        mdo.constant_type.as_ref().map(|attr| {
            let type_ref = hir_def::TypeRef::from_attribute_type(attr);
            TyLoweringContext::new().lower_type_ref_id(self.db, &type_ref)
        })
    }

    fn refine_constant_method(
        &self,
        mdo_name: &Name,
        method_name: &Name,
        return_ty: &mut TypeId,
        params: &mut [TypeId],
    ) {
        let lc = method_name.as_str().fold_lower();
        let is_get = lc == "получить" || lc == "get";
        let is_set = lc == "установить" || lc == "set";
        if !is_get && !is_set {
            return;
        }
        // The platform documents Получить/Установить with «Произвольный» — a
        // placeholder the constant's metadata makes precise, so both wildcard
        // lowerings (Unknown and the sticky Any) are refinable here.
        let is_wildcard = |id: TypeId| id == self.db.unknown() || id == self.db.any();
        let needs_override = (is_get && is_wildcard(*return_ty))
            || (is_set && params.first().is_some_and(|id| is_wildcard(*id)));
        if !needs_override {
            return;
        }
        let Some(value_ty) = self.resolve_constant_value_type(mdo_name) else {
            return;
        };
        if is_get {
            *return_ty = value_ty;
        } else if is_set {
            params[0] = value_ty;
        }
    }

    fn refine_constant_candidates(
        &self,
        mdo_name: &Name,
        method_name: &Name,
        candidates: &mut crate::call_resolution::CallCandidateSet,
    ) {
        for candidate in candidates.signatures_mut() {
            let mut params = candidate.params.iter().map(|param| param.ty).collect::<Vec<_>>();
            self.refine_constant_method(
                mdo_name,
                method_name,
                &mut candidate.return_ty,
                &mut params,
            );
            for (param, ty) in candidate.params.iter_mut().zip(params) {
                param.ty = ty;
            }
        }
    }
}

fn const_eval_literal_index(expr: &Expr) -> Option<usize> {
    let Expr::Literal(Literal::Number(n)) = expr else {
        return None;
    };
    let f = n.into_inner();
    if !f.is_finite() || f < 0.0 || f.fract() != 0.0 {
        return None;
    }
    Some(f as usize)
}

/// Spellings that denote the module's own object or form, both languages.
fn is_self_name(name_lower: &str) -> bool {
    matches!(name_lower, "этотобъект" | "thisobject" | "этаформа" | "thisform")
}

fn receiver_display_name(db: &dyn TypeKernelDb, receiver_ty: TypeId) -> Option<hir_def::Name> {
    match db.lookup_type(receiver_ty) {
        TypeKind::MetadataRef(facet) => {
            let plural = mdo_kind_to_plural(facet.kind)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::ThisObject { owner, .. } | TypeKind::ThisManager { owner, .. } => {
            let plural = owner.mdo_type.russian_plural()?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, owner.name.as_str())))
        }
        TypeKind::ObjectManager(facet) => {
            let plural = facet.mdo.russian_plural()?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::FormData { kind: FormDataFacet::Collection, underlying: Some(underlying) } => {
            let plural = underlying.mdo_type.russian_plural()?;
            let name = &underlying.name;
            Some(hir_def::Name::new(&format!("{}.{}", plural, name.as_str())))
        }
        _ => None,
    }
}

fn mdo_kind_to_plural(kind: hir_def::ty::MetadataKind) -> Option<&'static str> {
    use hir_def::ty::MetadataKind;
    let mdo = match kind {
        MetadataKind::CatalogObject | MetadataKind::CatalogRef => bsl_metadata::MdoType::Catalog,
        MetadataKind::DocumentObject | MetadataKind::DocumentRef => bsl_metadata::MdoType::Document,
        MetadataKind::EnumRef => bsl_metadata::MdoType::Enum,
        MetadataKind::TaskRef | MetadataKind::TaskObject => bsl_metadata::MdoType::Task,
        MetadataKind::BusinessProcessRef | MetadataKind::BusinessProcessObject => {
            bsl_metadata::MdoType::BusinessProcess
        }
        MetadataKind::DataProcessorObject => bsl_metadata::MdoType::DataProcessor,
        MetadataKind::ReportObject => bsl_metadata::MdoType::Report,
        MetadataKind::ExchangePlanRef | MetadataKind::ExchangePlanObject => {
            bsl_metadata::MdoType::ExchangePlan
        }
        MetadataKind::ChartOfAccountsRef | MetadataKind::ChartOfAccountsObject => {
            bsl_metadata::MdoType::ChartOfAccounts
        }
        MetadataKind::ChartOfCharacteristicTypesRef
        | MetadataKind::ChartOfCharacteristicTypesObject => {
            bsl_metadata::MdoType::ChartOfCharacteristicTypes
        }
        MetadataKind::ChartOfCalculationTypesRef | MetadataKind::ChartOfCalculationTypesObject => {
            bsl_metadata::MdoType::ChartOfCalculationTypes
        }
        MetadataKind::InformationRegisterRecordManager
        | MetadataKind::InformationRegisterRecordSet => bsl_metadata::MdoType::InformationRegister,
        MetadataKind::AccumulationRegisterRecordSet => bsl_metadata::MdoType::AccumulationRegister,
        MetadataKind::AccountingRegisterRecordSet => bsl_metadata::MdoType::AccountingRegister,
        MetadataKind::CalculationRegisterRecordSet => bsl_metadata::MdoType::CalculationRegister,
        MetadataKind::InformationRegisterRef
        | MetadataKind::AccumulationRegisterRef
        | MetadataKind::AccountingRegisterRef
        | MetadataKind::CalculationRegisterRef
        | MetadataKind::InformationRegisterRecord
        | MetadataKind::AccumulationRegisterRecord
        | MetadataKind::AccountingRegisterRecord
        | MetadataKind::CalculationRegisterRecord => return None,
        MetadataKind::TabularSection { parent } | MetadataKind::TabularSectionRow { parent } => {
            parent
        }
        MetadataKind::RegisterDimension { .. }
        | MetadataKind::RegisterResource { .. }
        | MetadataKind::RegisterAttribute { .. }
        | MetadataKind::RegisterFilter { .. } => return None,
    };
    mdo.russian_plural()
}

#[salsa::tracked(lru = 256, heap_size = heap_estimate::inference_result_heap, returns(clone))]
pub fn infer_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<InferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_query", ?file_id).entered();

    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies_ref(module_id);

    let mut result = InferenceResult::default();

    let fold_body = |result: &mut InferenceResult, body_result: &BodyInferenceResult| {
        let owner = body_result.owner;
        result.expr_types_by_body.insert(owner, body_result.expr_types.clone());
        result.binding_types_by_body.insert(owner, body_result.binding_types.clone());
        result.var_types.extend(body_result.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, body_result.implicit_locals.clone());
        result.diagnostics.extend(body_result.diagnostics.iter().map(|d| (owner, d.clone())));
        result.call_arg_bindings.extend(body_result.call_arg_bindings.iter().cloned());
    };

    let fold_module_code = |result: &mut InferenceResult,
                            module_code: &ModuleCodeInferenceResult| {
        let owner = module_code.owner;
        result.expr_types_by_body.insert(owner, module_code.expr_types.clone());
        result.binding_types_by_body.insert(owner, module_code.binding_types.clone());
        result.var_types.extend(module_code.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, module_code.implicit_locals.clone());
        result.diagnostics.extend(module_code.diagnostics.iter().map(|d| (owner, d.clone())));
        result.call_arg_bindings.extend(module_code.call_arg_bindings.iter().cloned());
    };

    {
        let _bspan = tracing::info_span!("infer_query.body", kind = "module_code").entered();
        let module_code = db.infer_module_code_ref(file_id);
        fold_module_code(&mut result, module_code);
    }

    for (local_id, _body) in module_bodies.iter_bodies() {
        let _bspan = tracing::info_span!("infer_query.body", kind = "method").entered();
        let method_id = hir_def::MethodId { module: module_id, local_id };
        let method_input = MethodIdInput::new(db, method_id);
        let body_result = db.infer_method_ref(method_input);
        fold_body(&mut result, body_result);
    }

    info!(
        "type inference complete: {} bodies, {} var types, {} diagnostics",
        result.expr_types_by_body.len(),
        result.var_types.len(),
        result.diagnostics.len()
    );

    Arc::new(result)
}

/// Inference over the *effective* module of an `&ИзменениеИКонтроль` extension/base
/// pair. Mirrors [`infer_query`] but runs every body inline through
/// [`InferenceContext::new_effective`] — same-module method/variable lookups resolve
/// against the effective symbol tree (so `#Вставка` code sees base siblings), while
/// metadata / cross-module context stays the base file. It deliberately does NOT
/// reuse `infer_method` / `infer_module_code`: those key on `ModuleId{base_file}` and
/// would collide with the base module's own cached inference.
#[salsa::tracked(lru = 256, heap_size = heap_estimate::inference_result_heap, returns(clone))]
pub fn infer_effective<'db>(
    db: &'db dyn HirDatabase,
    eid: EffectiveModuleId<'db>,
) -> Arc<InferenceResult> {
    let base_file = eid.base_file(db);
    let _p =
        tracing::info_span!("infer_effective", ?base_file, ext_file = ?eid.ext_file(db)).entered();

    let module_bodies = hir_def::effective_module::module_bodies_effective(db, eid);
    let symbol_tree = hir_def::effective_module::symbol_tree_effective(db, eid);

    // Only the `&ИзменениеИКонтроль` methods actually differ from the base module; every other
    // body is copied verbatim. A copied body's diagnostics are dropped by the orchestrator's
    // `#Вставка` remap, and its effective return equals the base return (identical body +
    // base-keyed sibling resolution), so `effective_returns` would hold exactly the value the
    // base fallback already yields for it. Restricting both passes to the changed methods is
    // therefore output-identical, while avoiding a re-inference of the whole — often large —
    // base module twice per extension file (the dominant cost on heavily-extended configs).
    let changed_ids: rustc_hash::FxHashSet<u32> = {
        let ext_parse = db.parse(eid.ext_file(db));
        let changed_targets: rustc_hash::FxHashSet<String> = ext_parse
            .syntax_node()
            .children()
            .filter(|n| {
                matches!(
                    n.kind(),
                    syntax::SyntaxKind::PROCEDURE_DEF | syntax::SyntaxKind::FUNCTION_DEF
                )
            })
            .filter_map(|m| hir_def::extension_merge::extract_change_and_validate(&m))
            .map(|cc| cc.target.fold_lower())
            .collect();
        symbol_tree
            .methods()
            .filter(|m| changed_targets.contains(&m.name.as_str().fold_lower()))
            .map(|m| m.id.local_id)
            .collect()
    };

    // Pass 1: infer each CHANGED method body once to capture its EFFECTIVE return type. A bare
    // same-module call to a `&ИзменениеИКонтроль` target must see its changed body's
    // return, not the base body's. Module code has no return, so only methods contribute.
    // This resolves one level of method→method return dependency; deeper chains keep the
    // base inference's same pragmatic bound (no panic, no infinite loop).
    let mut effective_returns: FxHashMap<u32, TypeId> = FxHashMap::default();
    for (local_id, body) in module_bodies.iter_bodies() {
        if !changed_ids.contains(&local_id) {
            continue;
        }
        let mut ctx = InferenceContext::new_effective(
            db,
            base_file,
            symbol_tree.clone(),
            DefWithBodyId::Method(local_id),
            &Arc::new(body.clone()),
        );
        ctx.infer_all();
        let ret = body_return_type(db, &ctx.finish());
        if ret != db.unknown() {
            effective_returns.insert(local_id, ret);
        }
    }
    let effective_returns = Arc::new(effective_returns);

    // Pass 2: re-infer every body with the effective returns threaded in; these results are
    // authoritative (a changed method's call-expr types and diagnostics are now correct).
    let mut result = InferenceResult::default();

    let fold_body = |result: &mut InferenceResult, body_result: &BodyInferenceResult| {
        let owner = body_result.owner;
        result.expr_types_by_body.insert(owner, body_result.expr_types.clone());
        result.binding_types_by_body.insert(owner, body_result.binding_types.clone());
        result.var_types.extend(body_result.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, body_result.implicit_locals.clone());
        result.diagnostics.extend(body_result.diagnostics.iter().map(|d| (owner, d.clone())));
        result.call_arg_bindings.extend(body_result.call_arg_bindings.iter().cloned());
    };

    if let Some(body) = module_bodies.module_code() {
        let mut ctx = InferenceContext::new_effective(
            db,
            base_file,
            symbol_tree.clone(),
            DefWithBodyId::ModuleCode,
            &Arc::new(body.clone()),
        );
        ctx.set_local_effective_returns(Arc::clone(&effective_returns));
        ctx.infer_all();
        fold_body(&mut result, &ctx.finish());
    }

    for (local_id, body) in module_bodies.iter_bodies() {
        if !changed_ids.contains(&local_id) {
            continue;
        }
        let mut ctx = InferenceContext::new_effective(
            db,
            base_file,
            symbol_tree.clone(),
            DefWithBodyId::Method(local_id),
            &Arc::new(body.clone()),
        );
        ctx.set_local_effective_returns(Arc::clone(&effective_returns));
        ctx.infer_all();
        fold_body(&mut result, &ctx.finish());
    }

    Arc::new(result)
}

/// Whether a bare call name is the platform `ПродолжитьВызов` / `ProceedWithCall` global, the
/// call that re-enters the original base method from a `&Вместо` interceptor. Mirrors the
/// recognition in `hir_def::body::lower::expr` (which is private there), folding the name to
/// lower so the bilingual case-insensitive spellings match.
fn is_proceed_with_call_name(name: &Name) -> bool {
    matches!(name.as_str().fold_lower().as_str(), "продолжитьвызов" | "proceedwithcall")
}

/// Inference over an extension module's OWN bodies under *weaving* (`&Вместо` /
/// `&Перед` / `&После`). Unlike [`infer_effective`] there is no text splice: the
/// extension module keeps its native text and is inferred through
/// [`InferenceContext::new_weaving`], so a bare same-module call that targets a base
/// method resolves via the base fallback (no spurious `UnresolvedMethodCall`) while
/// configuration / metadata context stays the extension file. Single pass — the
/// changed-return threading of effective inference is a later increment.
#[salsa::tracked(lru = 256, heap_size = heap_estimate::inference_result_heap, returns(clone))]
pub fn infer_weaving<'db>(
    db: &'db dyn HirDatabase,
    wid: hir_def::weaving::WeavingModuleId<'db>,
) -> Arc<InferenceResult> {
    let ext_file = wid.ext_file(db);
    let base_file = wid.base_file(db);
    let _p = tracing::info_span!("infer_weaving", ?ext_file, ?base_file).entered();

    let base_module = hir_def::ModuleId::new(base_file);
    let ext_module = hir_def::ModuleId::new(ext_file);
    let module_bodies = db.module_bodies_ref(ext_module);

    // A `&Вместо("M")` interceptor's body may call `ПродолжитьВызов(...)` to re-enter the
    // original base method `M`. Pre-compute, per ext method `local_id`: (a) `M`'s return type so
    // the call types as `M`'s result, and (b) `M`'s arity `(required, total)` so the call is
    // validated as a call to `M`. `&Перед`/`&После` carry no `ПродолжитьВызов`, so they are
    // skipped. The return is dropped when uninformative (Unknown/Undefined) to preserve the
    // platform default, but the arity is kept whenever `M` resolves — a procedure base has no
    // informative return yet its arguments still need checking.
    let ext_symbols = db.symbol_tree_ref(ext_module);
    let base_symbols = db.symbol_tree_ref(base_module);
    let ext_parse = db.parse(ext_file);
    let mut proceed_returns: FxHashMap<u32, TypeId> = FxHashMap::default();
    let mut proceed_arities: FxHashMap<u32, (usize, usize)> = FxHashMap::default();
    for method in ext_symbols.methods() {
        let Some(node) = method.syntax_node(&ext_parse) else {
            continue;
        };
        let Some(interception) = hir_def::weaving::interceptor_target(&node) else {
            continue;
        };
        if interception.kind != hir_def::weaving::InterceptionKind::Around {
            continue;
        }
        let Some(base_method) = base_symbols.find_method(&Name::new(&interception.target)) else {
            continue;
        };
        // `required` mirrors `FunctionSignature::required_count`: one past the last
        // non-defaulted parameter (defaults are trailing in well-formed BSL).
        let total = base_method.params.len();
        let required = base_method.params.iter().rposition(|p| !p.has_default).map_or(0, |i| i + 1);
        proceed_arities.insert(method.id.local_id, (required, total));

        let base_input = hir_def::MethodIdInput::new(db, base_method.id);
        let ret = crate::method_graph::method_return_type_query(db, base_input);
        if ret != db.unknown() && ret != db.undefined() {
            proceed_returns.insert(method.id.local_id, ret);
        }
    }

    let mut result = InferenceResult::default();

    let fold_body = |result: &mut InferenceResult, body_result: &BodyInferenceResult| {
        let owner = body_result.owner;
        result.expr_types_by_body.insert(owner, body_result.expr_types.clone());
        result.binding_types_by_body.insert(owner, body_result.binding_types.clone());
        result.var_types.extend(body_result.var_types.iter().map(|(k, v)| (k.clone(), *v)));
        result.implicit_locals_by_body.insert(owner, body_result.implicit_locals.clone());
        result.diagnostics.extend(body_result.diagnostics.iter().map(|d| (owner, d.clone())));
        result.call_arg_bindings.extend(body_result.call_arg_bindings.iter().cloned());
    };

    if let Some(body) = module_bodies.module_code() {
        let mut ctx = InferenceContext::new_weaving(
            db,
            ext_file,
            base_module,
            DefWithBodyId::ModuleCode,
            &Arc::new(body.clone()),
        );
        ctx.infer_all();
        fold_body(&mut result, &ctx.finish());
    }

    for (local_id, body) in module_bodies.iter_bodies() {
        let mut ctx = InferenceContext::new_weaving(
            db,
            ext_file,
            base_module,
            DefWithBodyId::Method(local_id),
            &Arc::new(body.clone()),
        );
        if let Some(ret) = proceed_returns.get(&local_id) {
            ctx.set_proceed_return(*ret);
        }
        if let Some(&(required, total)) = proceed_arities.get(&local_id) {
            ctx.set_proceed_arity(required, total);
        }
        ctx.infer_all();
        fold_body(&mut result, &ctx.finish());
    }

    Arc::new(result)
}

/// A method body's inferred return type: the union of its non-`Unknown` return-expression
/// types (mirrors `method_graph::method_return_type_query`, reused here so effective
/// inference can capture a changed method's return without the base-keyed query).
fn body_return_type(db: &dyn HirDatabase, body_result: &BodyInferenceResult) -> TypeId {
    let unknown = db.unknown();
    let tys: Vec<TypeId> = body_result
        .return_expr_ids
        .iter()
        .filter_map(|id| body_result.expr_types.get(id).copied())
        .filter(|t| *t != unknown)
        .collect();
    if tys.is_empty() {
        unknown
    } else {
        db.union(tys)
    }
}

#[salsa::tracked(lru = 1024, heap_size = heap_estimate::module_code_inference_result_heap, returns(ref))]
pub fn infer_module_code_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleCodeInferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_module_code_query", ?file_id).entered();

    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies_ref(module_id);

    let Some(body) = module_bodies.module_code() else {
        return Arc::new(ModuleCodeInferenceResult::default());
    };

    let mut ctx =
        InferenceContext::new(db, file_id, DefWithBodyId::ModuleCode, &Arc::new(body.clone()));
    ctx.infer_all();
    let body_result = ctx.finish();
    Arc::new(ModuleCodeInferenceResult::from_body(body_result))
}

pub fn type_of_expr_query(
    db: &dyn HirDatabase,
    file_id: FileId,
    owner: DefWithBodyId,
    expr: ExprId,
) -> TypeId {
    infer_owner(db, file_id, owner).type_id_of_expr(expr).unwrap_or_else(|| db.unknown())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_types::testing::InMemoryDb;

    #[test]
    fn t12_function_facet_synthesizes_candidate_with_argument_diagnostics() {
        use bsl_types::facet::{ArgArity, FunctionFacet, FunctionOrigin, ParamPassing, ParamSpec};

        let db = InMemoryDb::new();
        let number = db.number(None, None);
        let string = db.string(None, false);
        let facet = FunctionFacet::new(
            Arc::from([ParamSpec::new("value".to_string(), number, ParamPassing::ByVal, false)]),
            Arc::from([None]),
            1,
            ArgArity::Fixed(1),
            number,
            FunctionOrigin::Unknown,
        );

        let candidates = crate::call_resolution::CallCandidateSet::from_function_facet(&facet);
        let resolution = crate::call_resolution::resolve_candidates(&db, &candidates, &[string]);

        assert!(matches!(
            resolution.selection,
            crate::call_resolution::CallSelection::Rejected(
                crate::call_resolution::CallRejection::Type
            )
        ));
        assert_eq!(resolution.return_ty, db.unknown());
        assert_eq!(facet.returns, number);
        assert_eq!(candidates.as_slice()[0].params[0].ty, number);
    }

    #[test]
    fn test_inference_result_default() {
        let result = InferenceResult::default();
        assert_eq!(result.expr_types_by_body.len(), 0);
        assert_eq!(result.diagnostics.len(), 0);
        assert!(!result.has_diagnostics());
    }

    #[test]
    fn test_mismatched_arg_count_diagnostic() {
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let diag = InferenceDiagnostic::MismatchedArgCount {
            call_expr: expr_id,
            required_count: 2,
            total_count: 2,
            found: 1,
        };

        match diag {
            InferenceDiagnostic::MismatchedArgCount {
                call_expr,
                required_count,
                total_count,
                found,
            } => {
                assert_eq!(call_expr, expr_id);
                assert_eq!(required_count, 2);
                assert_eq!(total_count, 2);
                assert_eq!(found, 1);
            }
            _ => panic!("Expected MismatchedArgCount"),
        }
    }

    #[test]
    fn test_unresolved_method_call_diagnostic() {
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let receiver_name = Name::new("CommonModule");
        let method_name = Name::new("Method");

        let diag = InferenceDiagnostic::UnresolvedMethodCall {
            expr: expr_id,
            receiver_name: receiver_name.clone(),
            method_name: method_name.clone(),
            kind: UnresolvedMethodKind::MethodNotFound,
        };

        match diag {
            InferenceDiagnostic::UnresolvedMethodCall {
                expr,
                receiver_name: r,
                method_name: m,
                kind,
            } => {
                assert_eq!(expr, expr_id);
                assert_eq!(r, receiver_name);
                assert_eq!(m, method_name);
                assert_eq!(kind, UnresolvedMethodKind::MethodNotFound);
            }
            _ => panic!("Expected UnresolvedMethodCall"),
        }
    }

    #[test]
    fn test_type_mismatch_diagnostic() {
        let db = InMemoryDb::new();
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let expected_ty = db.number(None, None);
        let actual_ty = db.string(None, false);

        let diag = InferenceDiagnostic::TypeMismatch {
            expr: expr_id,
            expected: expected_ty,
            actual: actual_ty,
            from_doc_comment: false,
        };

        match diag {
            InferenceDiagnostic::TypeMismatch { expr, expected, actual, .. } => {
                assert_eq!(expr, expr_id);
                assert_eq!(expected, expected_ty);
                assert_eq!(actual, actual_ty);
            }
            _ => panic!("Expected TypeMismatch"),
        }
    }

    #[test]
    fn test_unresolved_field_diagnostic() {
        let db = InMemoryDb::new();
        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        let receiver_ty = db.metadata_ref(
            hir_def::ty::MetadataKind::CatalogRef,
            "Номенклатура".to_string(),
            &RootConfigCtx,
        );
        let field_name = Name::new("НесуществующееПоле");

        let diag = InferenceDiagnostic::UnresolvedField {
            expr: expr_id,
            receiver_ty,
            field_name: field_name.clone(),
        };

        match diag {
            InferenceDiagnostic::UnresolvedField { expr, receiver_ty: r, field_name: f } => {
                assert_eq!(expr, expr_id);
                assert_eq!(r, receiver_ty);
                assert_eq!(f, field_name);
            }
            _ => panic!("Expected UnresolvedField"),
        }
    }

    #[test]
    fn test_inference_result_with_diagnostics() {
        let mut result = InferenceResult::default();
        assert!(!result.has_diagnostics());

        let expr_id = ExprId::from_raw(la_arena::RawIdx::from_u32(0));
        result.diagnostics.push((
            DefWithBodyId::ModuleCode,
            InferenceDiagnostic::MismatchedArgCount {
                call_expr: expr_id,
                required_count: 2,
                total_count: 2,
                found: 1,
            },
        ));

        assert!(result.has_diagnostics());
        assert_eq!(result.diagnostics.len(), 1);
    }

    fn first_sig(
        db: &dyn bsl_types::intern::TypeKernelDb,
        builtins: &builtin::BuiltinFunctions,
        name: &str,
    ) -> hir_def::ty::FunctionSignature {
        let sigs = builtins.get(name).unwrap_or_else(|| panic!("{name} should exist"));
        sigs[0].lower(db)
    }

    #[test]
    fn test_builtin_function_lookup() {
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        let strlen_sig = first_sig(&db, builtins, "стрдлина");
        assert_eq!(strlen_sig.ret, db.number(None, None));
        assert_eq!(strlen_sig.params.len(), 1);
        assert_eq!(strlen_sig.params[0], db.string(None, false));

        let strlen_en = first_sig(&db, builtins, "strlen");
        assert_eq!(strlen_en.ret, db.number(None, None));

        let upper_case = builtins.get("СТРДЛИНА");
        assert!(upper_case.is_some(), "Lookup should be case-insensitive");
    }

    #[test]
    fn test_builtin_date_function() {
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        let current_date = first_sig(&db, builtins, "текущаядата");
        assert_eq!(current_date.ret, db.date(bsl_types::facet::DateComponent::DateTime));
        assert!(current_date.params.is_empty());

        let year = first_sig(&db, builtins, "год");
        assert_eq!(year.ret, db.number(None, None));
        assert_eq!(year.params.len(), 1);
        assert_eq!(year.params[0], db.date(bsl_types::facet::DateComponent::DateTime));
    }

    #[test]
    fn test_builtin_type_function() {
        let db = bsl_types::testing::InMemoryDb::new();
        let builtins = builtin::builtin_functions();

        let type_of = first_sig(&db, builtins, "типзнч");
        assert_eq!(type_of.ret, db.type_descriptor());
    }

    fn make_expr(idx: u32) -> ExprId {
        ExprId::from_raw(la_arena::RawIdx::from_u32(idx))
    }

    #[test]
    fn body_inference_result_empty_for_preserves_owner() {
        let method_owner = DefWithBodyId::Method(7);
        let method_empty = BodyInferenceResult::empty_for(method_owner);
        assert_eq!(method_empty.owner, method_owner);
        assert!(method_empty.var_types.is_empty());
        assert!(method_empty.implicit_locals.is_empty());
        assert!(method_empty.binding_types.is_empty());
        assert!(method_empty.expr_types.is_empty());
        assert!(method_empty.diagnostics.is_empty());
        assert!(method_empty.call_arg_bindings.is_empty());
        assert!(method_empty.return_expr_ids.is_empty());

        let module_empty = BodyInferenceResult::empty_for(DefWithBodyId::ModuleCode);
        assert_eq!(module_empty.owner, DefWithBodyId::ModuleCode);
    }

    #[test]
    fn module_code_inference_result_default_is_module_owner() {
        let result = ModuleCodeInferenceResult::default();
        assert_eq!(result.owner, DefWithBodyId::ModuleCode);
        assert!(result.var_types.is_empty());
        assert!(result.expr_types.is_empty());
    }

    #[test]
    fn module_code_inference_result_from_body_preserves_fields() {
        let db = InMemoryDb::new();
        let mut body = BodyInferenceResult::empty_for(DefWithBodyId::ModuleCode);
        body.var_types.insert("х".to_string(), db.string(None, false));
        body.expr_types.insert(make_expr(3), db.boolean());
        body.diagnostics.push(InferenceDiagnostic::TypeMismatch {
            expr: make_expr(4),
            expected: db.string(None, false),
            actual: db.number(None, None),
            from_doc_comment: false,
        });

        let lifted = ModuleCodeInferenceResult::from_body(body);

        assert_eq!(lifted.owner, DefWithBodyId::ModuleCode);
        assert_eq!(lifted.var_types.get("х").copied(), Some(db.string(None, false)),);
        assert_eq!(lifted.expr_types.get(&make_expr(3)).copied(), Some(db.boolean()),);
        assert_eq!(lifted.diagnostics.len(), 1);
    }

    #[test]
    fn infer_owner_result_method_accessors_route_to_method_payload() {
        let db = InMemoryDb::new();
        let mut body = BodyInferenceResult::empty_for(DefWithBodyId::Method(2));
        body.var_types.insert("х".to_string(), db.number(None, None));
        body.expr_types.insert(make_expr(5), db.string(None, false));

        let routed = InferOwnerResult::Method(Arc::new(body));

        assert_eq!(routed.owner(), DefWithBodyId::Method(2));
        assert_eq!(routed.type_id_of_expr(make_expr(5)), Some(db.string(None, false)));
        assert_eq!(routed.type_id_of_expr(make_expr(99)), None);
        assert_eq!(routed.var_types().get("х").copied(), Some(db.number(None, None)),);
        assert!(routed.implicit_locals().is_empty());
        assert!(routed.binding_types().is_empty());
    }

    #[test]
    fn infer_owner_result_module_code_accessors_route_to_module_payload() {
        let db = InMemoryDb::new();
        let mut mc = ModuleCodeInferenceResult::default();
        mc.var_types.insert("у".to_string(), db.boolean());
        mc.expr_types.insert(make_expr(1), db.date(DateComponent::DateTime));

        let routed = InferOwnerResult::ModuleCode(Arc::new(mc));

        assert_eq!(routed.owner(), DefWithBodyId::ModuleCode);
        assert_eq!(routed.type_id_of_expr(make_expr(1)), Some(db.date(DateComponent::DateTime)));
        assert_eq!(routed.var_types().get("у").copied(), Some(db.boolean()),);
    }
}
