use base_db::FileIdInput;
use bsl_types::builders::Builders;
use bsl_types::facet::{ArgArity, DateComponent, FormDataFacet, MdoRefFacet, ProjectionSource};
use bsl_types::intern::TypeKernelDb;
use bsl_types::kind::{ConfigId, Projection, TypeId, TypeKind};
use bsl_types::testing::RootConfigCtx;
use cfg_types::{BindingId, IdConversion};
use hir_def::body::Body;
use hir_def::hir::{BinaryOp, Expr, ExprIdx, Literal, Stmt, StmtIdx, UnaryOp};
use hir_def::resolver::Resolver;
use hir_def::ty::FunctionSignature;
use hir_def::{sdbl_hir_for_file_query, DefWithBodyId, ExprId, MethodIdInput, Name, SdblExprId};
use rustc_hash::FxHashMap;
use std::sync::Arc;
use tracing::{debug, info, trace};
use vfs::FileId;

use crate::builtin;
use crate::db::HirDatabase;
use crate::lower::TyLoweringContext;
use crate::method_resolution;
use crate::platform_manager_lookup::{resolve_platform_manager_method, PlatformMethodResolution};

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

    /// Approximate live bytes of an `FxHashMap`/hashbrown table holding `len`
    /// entries of `(K, V)`: one control byte plus the `(K, V)` slot per bucket,
    /// with bucket count grown to the next power of two above `len / (7/8)`.
    pub(super) fn map_table_bytes<K, V>(len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        // `checked_*`/`saturating_*` guard the (theoretically) unbounded `len`:
        // `next_power_of_two` panics in debug and wraps to 0 in release near
        // `usize::MAX`. Real inference maps are body-arena-bounded, so this never
        // triggers, but it keeps the estimator total.
        let cap = (len * 8 / 7 + 1).checked_next_power_of_two().unwrap_or(len);
        cap.saturating_mul(size_of::<K>() + size_of::<V>() + 1)
    }

    pub(super) fn vec_bytes<T>(len: usize) -> usize {
        len * size_of::<T>()
    }

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

    pub params: ParamsShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParamsShape {
    Single(Arc<[TypeId]>),

    Overloaded { flat: Arc<[TypeId]>, overloads: Arc<[Arc<[TypeId]>]> },
}

pub struct InferenceContext<'db> {
    db: &'db dyn HirDatabase,

    file_id: FileId,

    owner: DefWithBodyId,

    body: Arc<Body>,

    return_expr_ids: Vec<ExprId>,

    var_types: FxHashMap<String, TypeId>,

    implicit_locals: FxHashMap<String, ImplicitLocalInfo>,

    assigned_var_names: rustc_hash::FxHashSet<String>,

    binding_types: FxHashMap<BindingId, TypeId>,

    expr_types: FxHashMap<ExprId, TypeId>,

    diagnostics: Vec<InferenceDiagnostic>,

    call_arg_bindings: Vec<CallArgBinding>,
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
        Self {
            db,
            file_id,
            owner,
            body: Arc::clone(body),
            var_types: FxHashMap::default(),
            implicit_locals: FxHashMap::default(),
            assigned_var_names: rustc_hash::FxHashSet::default(),
            binding_types: FxHashMap::default(),
            expr_types: FxHashMap::default(),
            diagnostics: Vec::new(),
            call_arg_bindings: Vec::new(),
            return_expr_ids: Vec::new(),
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

    fn push_inference_diagnostic(&mut self, diag: InferenceDiagnostic) {
        let key = match &diag {
            InferenceDiagnostic::UnresolvedMethodCall { expr, .. } => *expr,
            InferenceDiagnostic::MismatchedArgCount { call_expr, .. } => *call_expr,
            InferenceDiagnostic::TypeMismatch { expr, .. } => *expr,
            InferenceDiagnostic::UnresolvedField { expr, .. } => *expr,
            InferenceDiagnostic::ReadOnlyPropertyAssignment { lhs, .. } => *lhs,
            InferenceDiagnostic::RedundantAccessToObjectTwoLevel { expr, .. } => *expr,
            InferenceDiagnostic::MissedRequiredParameterCommonModule { expr, .. } => *expr,
        };
        if self.body.is_recovered(key) {
            return;
        }
        self.diagnostics.push(diag);
    }

    fn is_unknown(&self, id: TypeId) -> bool {
        id == self.db.unknown()
    }

    pub fn finish(self) -> BodyInferenceResult {
        BodyInferenceResult {
            owner: self.owner,
            var_types: self.var_types,
            implicit_locals: self.implicit_locals,
            binding_types: self.binding_types,
            expr_types: self.expr_types,
            diagnostics: self.diagnostics,
            call_arg_bindings: self.call_arg_bindings,
            return_expr_ids: self.return_expr_ids,
        }
    }

    fn record_call_arg_binding(&mut self, call_expr: ExprId, args: &[ExprId], params: ParamsShape) {
        if self.body.is_recovered(call_expr) {
            return;
        }
        self.call_arg_bindings.push(CallArgBinding {
            owner: self.owner,
            call_expr,
            args: args.to_vec(),
            params,
        });
    }

    fn get_resolver(&self) -> Resolver {
        let module_id = hir_def::ModuleId { file_id: self.file_id };
        Resolver::with_builtins_and_workspace(module_id)
    }

    fn body_declares_binding(&self, name: &hir_def::Name) -> bool {
        let target = name.as_str().to_lowercase();
        self.body.bindings_iter().any(|(_, b)| b.name.as_str().to_lowercase() == target)
    }

    pub fn infer_all(&mut self) {
        let _p = tracing::debug_span!("infer_all").entered();

        let stmts: Vec<StmtIdx> = self.body.body_stmts_typed().to_vec();
        self.infer_stmts(&stmts);

        let expr_ids: Vec<ExprId> = self.body.exprs_iter().map(|(id, _)| id).collect();
        for expr_id in expr_ids {
            self.infer_expr(expr_id);
        }

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
                            None => {
                                let key = name.as_str().to_lowercase();
                                self.assigned_var_names.insert(key.clone());
                                let target_id = ExprId::from_idx(*target);
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
                                    self.var_types.insert(key, value_ty);
                                }
                            }
                        }
                    }
                    Expr::Field { base, field } => {
                        let base_ty = self.infer_expr(ExprId::from_idx(*base));
                        let obj_resolver =
                            crate::object_resolver::DbObjectResolver::new(self.db, self.file_id);
                        let resolver = self.get_resolver();
                        let info = crate::form_items::lookup_form_item_field(
                            self.db, &resolver, base_ty, field,
                        )
                        .or_else(|| {
                            crate::field_lookup::lookup_field(
                                self.db,
                                &obj_resolver,
                                base_ty,
                                field,
                            )
                        });
                        if let Some(info) = info {
                            if info.is_readonly {
                                self.push_inference_diagnostic(
                                    InferenceDiagnostic::ReadOnlyPropertyAssignment {
                                        lhs: ExprId::from_idx(*target),
                                        receiver_ty: base_ty,
                                        field_name: field.clone(),
                                    },
                                );
                            }
                        }
                    }
                    _ => {}
                }

                self.infer_expr(ExprId::from_idx(*target));
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
                self.infer_stmts(&preproc.then_branch);
                for (_, _, branch) in preproc.elsif_branches.iter() {
                    self.infer_stmts(branch);
                }
                if let Some(else_branch) = &preproc.else_branch {
                    self.infer_stmts(else_branch);
                }
            }

            Stmt::While { condition, body } => {
                self.infer_expr(ExprId::from_idx(*condition));
                self.infer_stmts(body);
            }

            Stmt::For { var, from, to, body } => {
                self.infer_expr(ExprId::from_idx(*from));
                self.infer_expr(ExprId::from_idx(*to));
                let var_name = self.body.binding_idx(*var).name.as_str().to_lowercase();
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
                    let var_name = self.body.binding_idx(*var).name.as_str().to_lowercase();
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
        let file_id_input = FileIdInput::new(self.db, self.file_id);
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

                crate::method_lookup::lookup_method(self.db, receiver_ty, method)
                    .map(|info| info.return_ty)
                    .unwrap_or_else(|| self.db.unknown())
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
                    crate::object_resolver::DbObjectResolver::new(self.db, self.file_id);
                if let Some(info) =
                    crate::form_items::lookup_form_item_field(self.db, &resolver, base_ty, field)
                {
                    info.ty
                } else if let Some(info) =
                    crate::field_lookup::lookup_field(self.db, &obj_resolver, base_ty, field)
                {
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
                    if matches!(base_kind, TypeKind::MetadataRef(_) | TypeKind::ThisObject { .. }) {
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

                if let Some(name) = type_name {
                    let ctors =
                        bsl_platform::PlatformDataInner::instance().get_constructors(name.as_str());
                    if !ctors.is_empty() {
                        let arg_count = args.len();
                        let sigs: Vec<builtin::BuiltinSignature> = ctors
                            .iter()
                            .map(|ctor| {
                                builtin::descriptor_from_params(
                                    &ctor.parameters,
                                    builtin::ReturnTypeSpec::Unknown,
                                )
                            })
                            .collect();

                        let mut arity_match: Option<usize> = None;
                        let mut best_idx = 0usize;
                        let mut best_distance = usize::MAX;
                        for (idx, sig) in sigs.iter().enumerate() {
                            let required = sig
                                .defaults()
                                .iter()
                                .rposition(|has_default| !*has_default)
                                .map_or(0, |i| i + 1);
                            let too_few = arg_count < required;
                            let too_many = sig.max_args().is_some_and(|m| arg_count > m as usize);
                            if !too_few && !too_many {
                                arity_match = Some(idx);
                                break;
                            }
                            let upper = sig.max_args().map_or(arg_count, |m| m as usize);
                            let distance = if too_few {
                                required - arg_count
                            } else {
                                arg_count.saturating_sub(upper)
                            };
                            if distance < best_distance {
                                best_distance = distance;
                                best_idx = idx;
                            }
                        }

                        if arity_match.is_none() {
                            let sig = &sigs[best_idx];
                            let required = sig
                                .defaults()
                                .iter()
                                .rposition(|has_default| !*has_default)
                                .map_or(0, |i| i + 1);
                            self.push_inference_diagnostic(
                                InferenceDiagnostic::MismatchedArgCount {
                                    call_expr: expr_id,
                                    required_count: required,
                                    total_count: sig.param_count(),
                                    found: arg_count,
                                },
                            );
                        }
                    }
                }

                let is_query_ctor = type_name.as_ref().is_some_and(|name| {
                    crate::method_lookup::is_platform_name(name, "Запрос", "Query")
                });
                if is_query_ctor {
                    let projections = self.try_synthesise_query_projections(args);
                    self.db.query(projections.iter().cloned().collect())
                } else {
                    match type_name {
                        Some(name) => TyLoweringContext::new().lower_bare_name_id(self.db, name),
                        None => self.db.unknown(),
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

        let name_lower = name.as_str().to_lowercase();
        if name_lower == "этотобъект" || name_lower == "thisobject" {
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
            if crate::this_object::is_managed_form_module(self.db, &resolver) {
                trace!("resolved {} as managed form Self", name);
                return self.db.platform_object(crate::form_self::FORM_TYPE_NAME.to_string());
            }
        }

        let resolved = resolver.resolve_name(self.db, name);

        if let Some(ty) = self.var_types.get(&name.as_str().to_lowercase()) {
            trace!("resolved {} via var_types = {:?}", name, ty);
            let ty_id = *ty;
            if crate::method_lookup::receiver_needs_refinement_id(self.db, ty_id) {
                if let Some(projections) = crate::query_text_dataflow::refine_query_at_use_site(
                    self.db,
                    self.file_id,
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

        if let Some(mdo_type) = bsl_metadata::MdoType::from_plural(name.as_str()) {
            if mdo_type.manager_type_prefix().is_some() {
                trace!("resolved {} as manager collection {:?}", name, mdo_type);
                return self.db.manager_collection(mdo_type);
            }
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

        if !user_shadows && !body_binding_shadows && !workspace_owns_common_module {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_object_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit ЭтотОбъект.{} member", name, name);
                return ty;
            }
        }

        if !user_shadows && !body_binding_shadows && !workspace_owns_common_module {
            if let Some(ty) =
                crate::this_object_attr::resolve_this_record_set_member(self.db, &resolver, name)
            {
                trace!("resolved {} as implicit record-set ЭтотОбъект.{} member", name, name);
                return ty;
            }
        }

        if !user_shadows && !workspace_owns_common_module {
            if let Some(id) =
                crate::platform_global_lookup::resolve_platform_global_property_type(self.db, name)
            {
                trace!("resolved {} as platform global → {:?}", name, id);
                return id;
            }
        }

        if !user_shadows && !workspace_owns_common_module {
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
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
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
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
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
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
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
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
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
                        let total = resolution.signature.params.len();
                        let required = resolution.signature.required_count();
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
                        self.record_call_arg_binding(
                            callee,
                            args,
                            ParamsShape::Single(
                                resolution.signature.params.iter().copied().collect(),
                            ),
                        );
                        self.expr_types.insert(callee, self.db.unknown());
                        return resolution.return_type;
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
                file_id: self.file_id,
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
                    let mut return_ty = info.return_ty;
                    let mut params: Vec<TypeId> = info.params.to_vec();
                    let overloads: Vec<Arc<[TypeId]>> =
                        info.overloads.iter().cloned().map(Arc::from).collect();
                    let manager_receiver_kind = self.db.lookup_type(manager_receiver_ty);
                    if let TypeKind::ObjectManager(facet) = manager_receiver_kind {
                        if facet.mdo == bsl_metadata::MdoType::Constant {
                            let mdo_name = hir_def::Name::new(&facet.name);
                            self.refine_constant_method(
                                &mdo_name,
                                &method_name,
                                &mut return_ty,
                                &mut params,
                            );
                        }
                    }
                    self.record_call_arg_binding(
                        callee,
                        args,
                        ParamsShape::Overloaded {
                            flat: params.into(),
                            overloads: overloads.into(),
                        },
                    );
                    return_ty
                }
                None => {
                    if let Some(receiver_name) = receiver_display_name(self.db, receiver_ty) {
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

        if let Expr::Path(name) = callee_expr {
            let name = name.clone();
            if let Some(sigs) = builtin::builtin_functions().get(name.as_str()) {
                debug_assert!(
                    !sigs.is_empty(),
                    "BuiltinFunctions::get must never return an empty overload set"
                );

                let sigs: Vec<FunctionSignature> = sigs.iter().map(|s| s.lower(self.db)).collect();

                for arg in args {
                    self.infer_expr(*arg);
                }

                let arg_count = args.len();
                let inferred: Vec<TypeId> = args
                    .iter()
                    .map(|a| self.expr_types.get(a).copied().unwrap_or_else(|| self.db.unknown()))
                    .collect();

                let mut full_match: Option<usize> = None;
                let mut arity_match: Option<usize> = None;
                let mut best_idx = 0usize;
                let mut best_distance = usize::MAX;
                for (idx, sig) in sigs.iter().enumerate() {
                    let required = sig.required_count();
                    let too_few = arg_count < required;
                    let too_many = sig.max_args.is_some_and(|m| arg_count > m as usize);
                    if !too_few && !too_many {
                        if arity_match.is_none() {
                            arity_match = Some(idx);
                        }
                        let types_ok =
                            inferred.iter().zip(sig.params.iter()).all(|(actual, expected)| {
                                crate::subtype::is_assignable(self.db, *actual, *expected)
                            });
                        if types_ok && full_match.is_none() {
                            full_match = Some(idx);
                            break;
                        }
                    }
                    let upper = sig.max_args.map_or(arg_count, |m| m as usize);
                    let distance = if too_few {
                        required - arg_count
                    } else {
                        arg_count.saturating_sub(upper)
                    };
                    if distance < best_distance {
                        best_distance = distance;
                        best_idx = idx;
                    }
                }

                let chosen = match full_match.or(arity_match) {
                    Some(idx) => &sigs[idx],
                    None => {
                        let sig = &sigs[best_idx];
                        let required = sig.required_count();
                        self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                            call_expr: callee,
                            required_count: required,
                            total_count: sig.params.len(),
                            found: arg_count,
                        });
                        sig
                    }
                };

                let overloads_arc: Arc<[Arc<[TypeId]>]> = sigs
                    .iter()
                    .map(|s| s.params.iter().copied().collect())
                    .collect::<Vec<_>>()
                    .into();
                self.record_call_arg_binding(
                    callee,
                    args,
                    ParamsShape::Overloaded {
                        flat: chosen.params.iter().copied().collect(),
                        overloads: overloads_arc,
                    },
                );
                let ret = if sigs.len() == 1 {
                    chosen.ret
                } else {
                    self.db.union(sigs.iter().map(|s| s.ret).collect())
                };
                self.expr_types.insert(callee, self.db.unknown());
                return ret;
            }
        }

        let bare_callee_name: Option<hir_def::Name> = match callee_expr {
            Expr::Path(n) => Some(n.clone()),
            _ => None,
        };

        let callee_ty = self.infer_expr(callee);

        for arg in args {
            self.infer_expr(*arg);
        }

        let callee_kind = self.db.lookup_type(callee_ty);
        match callee_kind {
            TypeKind::Function(facet) => {
                let total = facet.params.len();
                let required = facet.min_args as usize;
                let too_few = args.len() < required;
                let too_many = match facet.max_args {
                    ArgArity::Fixed(n) => args.len() > n as usize,
                    ArgArity::Variadic => false,
                    _ => false,
                };
                if too_few || too_many {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr: callee,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                self.record_call_arg_binding(
                    callee,
                    args,
                    ParamsShape::Single(facet.params.iter().map(|p| p.ty).collect()),
                );

                facet.returns
            }
            TypeKind::Unknown => {
                if let Some(name) = bare_callee_name.as_ref() {
                    if !self.body_declares_binding(name)
                        && !self.assigned_var_names.contains(&name.as_str().to_lowercase())
                    {
                        let module_id = hir_def::ModuleId::new(self.file_id);
                        let symbol_tree = self.db.symbol_tree(module_id);
                        if let Some(method) = symbol_tree.find_method(name) {
                            let sig = crate::method_resolution::materialise_signature_enriched(
                                self.db, method.id, method,
                            );
                            let ret = sig.ret;
                            if !self.is_unknown(ret) {
                                return ret;
                            }
                        }
                    }
                }
                self.db.unknown()
            }
            _ => self.db.unknown(),
        }
    }

    fn infer_qualified_call(
        &mut self,
        module_name: &Name,
        method_name: &Name,
        args: &[ExprId],
        call_expr: ExprId,
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

                let total = resolution.signature.params.len();
                let required = resolution.signature.required_count();
                if args.len() < required || args.len() > total {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                self.record_call_arg_binding(
                    call_expr,
                    args,
                    ParamsShape::Single(resolution.signature.params.iter().copied().collect()),
                );

                resolution.return_type
            }
            Err(kind) => {
                if matches!(kind, UnresolvedMethodKind::MethodNotFound) {
                    let source_root_id =
                        self.db.file_source_root_input(self.file_id).source_root_id(self.db);
                    let module_in_workspace = self
                        .db
                        .module_index(source_root_id)
                        .resolve_common_module(module_name)
                        .is_some();

                    if !module_in_workspace {
                        if let crate::platform_global_lookup::PlatformGlobalLookup::Resolved(
                            return_ty,
                        ) = crate::platform_global_lookup::try_resolve_platform_global_member(
                            self.db,
                            module_name,
                            method_name,
                        ) {
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
        if self.assigned_var_names.contains(&module_name.as_str().to_lowercase()) {
            return None;
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

            return Some(self.infer_qualified_call(module_name, method_name, args, call_expr));
        }

        match crate::platform_global_lookup::try_resolve_platform_global_member(
            self.db,
            module_name,
            method_name,
        ) {
            crate::platform_global_lookup::PlatformGlobalLookup::Resolved(return_ty) => {
                for arg in args {
                    self.infer_expr(*arg);
                }
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

                let total = resolution.signature.params.len();
                let required = resolution.signature.required_count();
                if args.len() < required || args.len() > total {
                    self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                        call_expr,
                        required_count: required,
                        total_count: total,
                        found: args.len(),
                    });
                }

                self.record_call_arg_binding(
                    call_expr,
                    args,
                    ParamsShape::Single(resolution.signature.params.iter().copied().collect()),
                );

                resolution.return_type
            }
            Err(UnresolvedMethodKind::MethodNotFound) => {
                let mdo_type_opt = bsl_metadata::MdoType::from_plural(mdo_type_plural.as_str());
                let plat_res: Option<PlatformMethodResolution> = mdo_type_opt
                    .filter(|mdo_type| self.mdo_declared(*mdo_type, mdo_name))
                    .and_then(|mdo_type| {
                        resolve_platform_manager_method(self.db, mdo_type, mdo_name, method_name)
                    });
                if let Some(mut res) = plat_res {
                    if mdo_type_opt == Some(bsl_metadata::MdoType::Constant) {
                        let mut return_ty = res.return_ty;
                        let mut params: Vec<TypeId> = res.signature.params.to_vec();
                        self.refine_constant_method(
                            mdo_name,
                            method_name,
                            &mut return_ty,
                            &mut params,
                        );
                        res.return_ty = return_ty;
                        res.signature.params = params.into_boxed_slice();
                    }
                    let total = res.signature.params.len();
                    let required = res.signature.required_count();
                    if args.len() < required || args.len() > total {
                        self.push_inference_diagnostic(InferenceDiagnostic::MismatchedArgCount {
                            call_expr,
                            required_count: required,
                            total_count: total,
                            found: args.len(),
                        });
                    }
                    self.record_call_arg_binding(
                        call_expr,
                        args,
                        ParamsShape::Single(res.signature.params.to_vec().into()),
                    );
                    return res.return_ty;
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
        self.db.resolve_metadata_object(self.file_id, mdo_type, needle).is_some()
            || self.db.resolve_register(self.file_id, mdo_type, needle).is_some()
    }

    fn resolve_constant_value_type(&self, mdo_name: &Name) -> Option<TypeId> {
        let mdo = self.db.resolve_metadata_object(
            self.file_id,
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
        let lc = method_name.as_str().to_lowercase();
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

fn receiver_display_name(db: &dyn TypeKernelDb, receiver_ty: TypeId) -> Option<hir_def::Name> {
    match db.lookup_type(receiver_ty) {
        TypeKind::MetadataRef(facet) => {
            let plural = mdo_kind_to_plural(facet.kind)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::ThisObject { owner, .. } | TypeKind::ThisManager { owner, .. } => {
            let plural = mdo_type_to_plural(owner.mdo_type)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, owner.name.as_str())))
        }
        TypeKind::ObjectManager(facet) => {
            let plural = mdo_type_to_plural(facet.mdo)?;
            Some(hir_def::Name::new(&format!("{}.{}", plural, facet.name.as_str())))
        }
        TypeKind::FormData { kind: FormDataFacet::Collection, underlying: Some(underlying) } => {
            let plural = mdo_type_to_plural(underlying.mdo_type)?;
            let name = &underlying.name;
            Some(hir_def::Name::new(&format!("{}.{}", plural, name.as_str())))
        }
        _ => None,
    }
}

fn mdo_type_to_plural(mdo_type: bsl_metadata::MdoType) -> Option<&'static str> {
    use bsl_metadata::MdoType;
    Some(match mdo_type {
        MdoType::Document => "Документы",
        MdoType::Catalog => "Справочники",
        MdoType::InformationRegister => "РегистрыСведений",
        MdoType::AccumulationRegister => "РегистрыНакопления",
        MdoType::AccountingRegister => "РегистрыБухгалтерии",
        MdoType::CalculationRegister => "РегистрыРасчета",
        MdoType::ChartOfCharacteristicTypes => "ПланыВидовХарактеристик",
        MdoType::ChartOfAccounts => "ПланыСчетов",
        MdoType::ChartOfCalculationTypes => "ПланыВидовРасчета",
        MdoType::BusinessProcess => "БизнесПроцессы",
        MdoType::Task => "Задачи",
        MdoType::Enum => "Перечисления",
        MdoType::ExchangePlan => "ПланыОбмена",
        MdoType::DataProcessor => "Обработки",
        MdoType::Report => "Отчеты",
        _ => return None,
    })
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
    mdo_type_to_plural(mdo)
}

#[salsa::tracked(lru = 256, heap_size = heap_estimate::inference_result_heap)]
pub fn infer_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<InferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_query", ?file_id).entered();

    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

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
        let module_code = db.infer_module_code(file_id);
        fold_module_code(&mut result, &module_code);
    }

    for (local_id, _body) in module_bodies.iter_bodies() {
        let _bspan = tracing::info_span!("infer_query.body", kind = "method").entered();
        let method_id = hir_def::MethodId { module: module_id, local_id };
        let method_input = MethodIdInput::new(db, method_id);
        let body_result = db.infer_method(method_input);
        fold_body(&mut result, &body_result);
    }

    info!(
        "type inference complete: {} bodies, {} var types, {} diagnostics",
        result.expr_types_by_body.len(),
        result.var_types.len(),
        result.diagnostics.len()
    );

    Arc::new(result)
}

#[salsa::tracked(lru = 1024, heap_size = heap_estimate::module_code_inference_result_heap)]
pub fn infer_module_code_query<'db>(
    db: &'db dyn HirDatabase,
    file_id_input: FileIdInput<'db>,
) -> Arc<ModuleCodeInferenceResult> {
    let file_id = file_id_input.file_id(db);
    let _p = tracing::info_span!("infer_module_code_query", ?file_id).entered();

    let module_id = hir_def::ModuleId { file_id };
    let module_bodies = db.module_bodies(module_id);

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
    fn params_shape_typeids_round_trip_via_ty() {
        let db = InMemoryDb::new();
        let number = db.number(None, None);
        let string = db.string(None, false);

        let single = ParamsShape::Single(Arc::from([number, string]));
        match single {
            ParamsShape::Single(ids) => {
                assert_eq!(ids.as_ref(), &[number, string]);
            }
            _ => panic!("expected Single"),
        }

        let overloaded = ParamsShape::Overloaded {
            flat: Arc::from([number]),
            overloads: Arc::from([Arc::from([number]) as Arc<[TypeId]>]),
        };
        match overloaded {
            ParamsShape::Overloaded { flat, overloads } => {
                assert_eq!(flat.as_ref(), &[number]);
                assert_eq!(overloads.len(), 1);
                assert_eq!(overloads[0].as_ref(), &[number]);
            }
            _ => panic!("expected Overloaded"),
        }
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
        };

        match diag {
            InferenceDiagnostic::TypeMismatch { expr, expected, actual } => {
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
    ) -> FunctionSignature {
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
