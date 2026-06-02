use bsl_metadata::MdoType;
use rustc_hash::FxHashMap;
use syntax::{TextRange, TextSize};

use crate::{
    body::{Body, BodySourceMap, ManagerType},
    hir::{Expr, ExprIdx, Literal},
    item_tree::{AnnotationKind, ItemTree, ModItem},
    name::Name,
    MethodId, ModuleBodies, ModuleId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleCallSummary {
    pub methods: Vec<MethodSummary>,
    pub call_edges: Vec<CallEdge>,
    pub notify_regs: Vec<NotifyReg>,
    pub idle_handler_regs: Vec<IdleReg>,
    pub form_entries: Vec<FormEventEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSummary {
    pub local_id: u32,
    pub name: Name,
    pub dispatch: MethodDispatch,
    pub is_export: bool,
}

/// Per-method facts derivable from the item tree alone — declaration name,
/// export flag, annotation dispatch, and the name/source ranges — without
/// lowering any body. This is the compact, resident "Pass A" data a streaming
/// graph build needs: enumerating methods and their durable-id/source coordinates
/// must not force the heavy body HIR that the per-module edge projection does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphMethodEntry {
    pub local_id: u32,
    pub name: Name,
    pub is_export: bool,
    pub dispatch: MethodDispatch,
    /// Range of the declaration's name token — anchors the start of the signature.
    pub name_range: TextRange,
    /// End of the declaration header (closing `)` or export keyword) — anchors the
    /// end of the full, possibly multi-line, signature slice.
    pub sig_end: TextSize,
    /// Range of the whole procedure/function — the `source` slice.
    pub source_range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodDispatch {
    pub can_run_on_client: bool,
    pub can_run_on_server: bool,
    pub no_context: bool,
}

impl MethodDispatch {
    pub fn from_annotation(kind: Option<&AnnotationKind>) -> Self {
        match kind {
            Some(AnnotationKind::AtClient) => {
                Self { can_run_on_client: true, can_run_on_server: false, no_context: false }
            }
            Some(AnnotationKind::AtServer) => {
                Self { can_run_on_client: false, can_run_on_server: true, no_context: false }
            }
            Some(AnnotationKind::AtServerNoContext) => {
                Self { can_run_on_client: false, can_run_on_server: true, no_context: true }
            }
            Some(AnnotationKind::AtClientAtServer) => {
                Self { can_run_on_client: true, can_run_on_server: true, no_context: false }
            }
            Some(AnnotationKind::AtClientAtServerNoContext) => {
                Self { can_run_on_client: true, can_run_on_server: true, no_context: true }
            }
            Some(
                AnnotationKind::Before
                | AnnotationKind::After
                | AnnotationKind::Instead
                | AnnotationKind::ChangeAndValidate,
            )
            | None => Self { can_run_on_client: true, can_run_on_server: false, no_context: false },
        }
    }

    pub fn is_server_only(&self) -> bool {
        self.can_run_on_server && !self.can_run_on_client
    }

    /// Module-level client/server capability from a common module's execution
    /// context (where dispatch is set per-module, not per-method). `None` for
    /// `Unknown` — the caller then falls back to per-method annotation dispatch.
    pub fn from_execution_context(ctx: crate::ExecutionContext) -> Option<Self> {
        use crate::ExecutionContext;
        let d = |can_run_on_client, can_run_on_server| Self {
            can_run_on_client,
            can_run_on_server,
            no_context: false,
        };
        Some(match ctx {
            ExecutionContext::Server
            | ExecutionContext::ServerCall
            | ExecutionContext::ExternalConnection => d(false, true),
            ExecutionContext::Client => d(true, false),
            ExecutionContext::ClientServer => d(true, true),
            ExecutionContext::Unknown => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallEdge {
    pub caller: CallerId,
    pub target: CallTarget,
    pub kind: EdgeKind,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    DirectLocal,
    DirectQualifiedModule,
    /// `Справочники.X.СоздатьЭлемент()` — a manager method that creates an object
    /// of its metadata type. Target is an [`GraphNode::Mdo`].
    ManagerCreates,
    /// Any other touch of an object through its manager (a platform find/select
    /// method, or a bare `Справочники.X` reference). Target is an [`GraphNode::Mdo`].
    ManagerAccess,
    /// The caller runs an SDBL query that reads a metadata object. From a
    /// `Method`/`ModuleCode` node to the read object's [`GraphNode::Mdo`].
    QueryRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CallTarget {
    Local { callee_local_id: u32 },
    QualifiedModule { module_name: Name, method_name: Name },
    ManagerAccess { manager_type: ManagerType, object_name: Name, method_name: Option<Name> },
    ThisObjectMethod { method_name: Name },
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CallerId {
    Method(u32),
    ModuleCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyReg {
    pub caller: CallerId,
    pub callback_name: Name,
    pub target_module: Option<Name>,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdleReg {
    pub caller: CallerId,
    pub handler_name: Name,
    pub one_shot: bool,
    pub range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormEventEntry {
    pub event_type: String,
    pub handler_name: Name,
}

/// A module's call edges with each target resolved to a concrete graph node
/// where possible. Produced by `resolved_module_summary_query`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedModuleSummary {
    pub module: ModuleId,
    pub edges: Vec<ResolvedCallEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCallEdge {
    pub caller: CallerId,
    pub target: ResolvedTarget,
    pub kind: EdgeKind,
    pub range: TextRange,
    pub provenance: EdgeProvenance,
}

/// Resolution outcome for a call edge's target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedTarget {
    /// Resolved to a concrete method node.
    Method(MethodId),
    /// Resolved to a metadata-object node (a manager access that is not a user
    /// manager-module method — a creation/find platform method or bare reference).
    Mdo { mdo_type: MdoType, object_name: Name },
    /// Not resolved to a concrete node; the original target is preserved so
    /// the gap is surfaced honestly rather than dropped.
    Unresolved(CallTarget),
}

/// How much to trust a resolved edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeProvenance {
    /// Target points at a concrete node by direct lookup (local call, or an
    /// exported qualified-module call).
    Resolved,
    /// Target resolved to a concrete node via metadata inference (e.g. a
    /// user-defined method on an object's manager module).
    Inferred,
    /// Target method exists but is not exported — visible-but-unreachable across modules.
    VisibilityBlocked,
    /// Target could not be resolved to a node: missing, or a platform builtin
    /// (e.g. a manager method like `СоздатьЭлемент`, or a `ЭтотОбъект` platform method).
    Unresolved,
}

/// A globally-addressable node in the workspace call graph.
///
/// Not `Copy`: an `Mdo` node carries a `Name`. Clones are cheap (`SmolStr` is
/// inline for short names) and the graph stores nodes by reference internally.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum GraphNode {
    Method(MethodId),
    /// A module's top-level body (the `CallerId::ModuleCode` caller).
    ModuleCode(ModuleId),
    /// A metadata object (catalog, document, register, …) reached through its
    /// manager. The identity is the metadata type plus the object name as it
    /// appears in code, canonicalised to a single spelling per object at fold time.
    Mdo {
        mdo_type: MdoType,
        object_name: Name,
    },
    /// An attribute (or register dimension/resource, tabular-section column, …) of
    /// a metadata object, read by an SDBL query. `object_name` shares the `Mdo`
    /// canonicalisation; `attr_name` is the declared metadata field name.
    Attribute {
        mdo_type: MdoType,
        object_name: Name,
        attr_name: Name,
    },
}

/// A resolved call edge between two workspace graph nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCallEdge {
    pub from: GraphNode,
    pub to: GraphNode,
    pub kind: EdgeKind,
    pub provenance: EdgeProvenance,
    /// A client-capable caller invoking a server-only callee — a client→server
    /// roundtrip. Dispatch comes from the module's execution context (common
    /// modules) or per-method `&НаКлиенте`/`&НаСервере` annotations (form/command
    /// modules). `false` when either endpoint's dispatch is unknown (e.g. a
    /// `ModuleCode` caller, or a module with `Unknown` context and no annotation).
    pub crosses_client_to_server: bool,
}

/// Whole-config call graph: forward (callees) and reverse (callers) adjacency
/// over the resolved edges of every module, plus per-method client/server
/// dispatch. Produced by `workspace_call_graph_query`. Only edges whose target
/// resolved to a concrete node are indexed; unresolved outgoing calls stay
/// visible per-module via `ResolvedModuleSummary`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceCallGraph {
    forward: FxHashMap<GraphNode, Vec<WorkspaceCallEdge>>,
    reverse: FxHashMap<GraphNode, Vec<WorkspaceCallEdge>>,
    node_dispatch: FxHashMap<GraphNode, MethodDispatch>,
}

impl WorkspaceCallGraph {
    pub fn insert(&mut self, edge: WorkspaceCallEdge) {
        self.forward.entry(edge.from.clone()).or_default().push(edge.clone());
        self.reverse.entry(edge.to.clone()).or_default().push(edge);
    }

    pub fn set_dispatch(&mut self, node: GraphNode, dispatch: MethodDispatch) {
        self.node_dispatch.insert(node, dispatch);
    }

    /// Client/server dispatch of a node, if known (method nodes only).
    pub fn dispatch(&self, node: &GraphNode) -> Option<MethodDispatch> {
        self.node_dispatch.get(node).copied()
    }

    /// Outgoing resolved calls from `node` (callees).
    pub fn callees(&self, node: &GraphNode) -> &[WorkspaceCallEdge] {
        self.forward.get(node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Incoming resolved calls to `node` (callers).
    pub fn callers(&self, node: &GraphNode) -> &[WorkspaceCallEdge] {
        self.reverse.get(node).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Every node that participates in the graph: it has an outgoing edge, an
    /// incoming edge, or a known dispatch. Order is unspecified.
    pub fn nodes(&self) -> impl Iterator<Item = GraphNode> + '_ {
        let mut seen = FxHashMap::default();
        self.forward
            .keys()
            .chain(self.reverse.keys())
            .chain(self.node_dispatch.keys())
            .filter(move |node| seen.insert((*node).clone(), ()).is_none())
            .cloned()
    }

    /// Every resolved edge in the graph (each edge appears once). Order is unspecified.
    pub fn edges(&self) -> impl Iterator<Item = &WorkspaceCallEdge> + '_ {
        self.forward.values().flat_map(|edges| edges.iter())
    }

    /// Number of resolved edges.
    pub fn edge_count(&self) -> usize {
        self.forward.values().map(Vec::len).sum()
    }

    /// Caller-in centrality: how many resolved calls target `node`.
    pub fn in_degree(&self, node: &GraphNode) -> usize {
        self.reverse.get(node).map_or(0, Vec::len)
    }
}

/// Enumerate a module's methods from the item tree, in top-level declaration
/// order (so the index is each method's `local_id`). Reads declarations only and
/// never lowers bodies — cheap enough to run over a whole configuration without
/// the RAM cost of body HIR.
pub fn extract_graph_methods(item_tree: &ItemTree) -> Vec<GraphMethodEntry> {
    let mut methods = Vec::new();
    for (top_level_idx, item) in item_tree.top_level_items().iter().enumerate() {
        let local_id = top_level_idx as u32;
        let entry = match item {
            ModItem::Procedure(idx) => {
                let proc = item_tree.procedure(*idx);
                GraphMethodEntry {
                    local_id,
                    name: proc.name.clone(),
                    is_export: proc.is_export,
                    dispatch: MethodDispatch::from_annotation(
                        proc.annotations.first().map(|a| &a.kind),
                    ),
                    name_range: proc.name_range,
                    sig_end: proc.sig_end,
                    source_range: proc.source_range,
                }
            }
            ModItem::Function(idx) => {
                let func = item_tree.function(*idx);
                GraphMethodEntry {
                    local_id,
                    name: func.name.clone(),
                    is_export: func.is_export,
                    dispatch: MethodDispatch::from_annotation(
                        func.annotations.first().map(|a| &a.kind),
                    ),
                    name_range: func.name_range,
                    sig_end: func.sig_end,
                    source_range: func.source_range,
                }
            }
            ModItem::Variable(_) => continue,
        };
        methods.push(entry);
    }
    methods
}

pub fn extract_call_summary(
    item_tree: &ItemTree,
    module_bodies: &ModuleBodies,
    form_event_handlers: &[bsl_metadata::FormEventHandler],
) -> ModuleCallSummary {
    // Method enumeration is the cheap, body-free part — share it with the graph
    // build. The `local_method_ids` map (lowercased name → first local id) is what
    // body extraction uses to bind local calls.
    let graph_methods = extract_graph_methods(item_tree);
    let mut local_method_ids: FxHashMap<String, u32> = FxHashMap::default();
    let mut methods = Vec::with_capacity(graph_methods.len());
    for method in &graph_methods {
        local_method_ids.entry(method.name.as_str().to_lowercase()).or_insert(method.local_id);
        methods.push(MethodSummary {
            local_id: method.local_id,
            name: method.name.clone(),
            dispatch: method.dispatch,
            is_export: method.is_export,
        });
    }

    let mut call_edges = Vec::new();
    let mut notify_regs = Vec::new();
    let mut idle_handler_regs = Vec::new();

    let mut sorted_ids: Vec<u32> = module_bodies.iter_lower_results().map(|(id, _)| id).collect();
    sorted_ids.sort_unstable();

    for local_id in sorted_ids {
        let lower_result = match module_bodies.lower_result(local_id) {
            Some(lr) => lr,
            None => continue,
        };
        extract_from_body(
            &lower_result.body,
            &lower_result.source_map,
            CallerId::Method(local_id),
            &local_method_ids,
            &mut call_edges,
            &mut notify_regs,
            &mut idle_handler_regs,
        );
    }

    if let Some(module_code) = module_bodies.module_code_result() {
        extract_from_body(
            &module_code.body,
            &module_code.source_map,
            CallerId::ModuleCode,
            &local_method_ids,
            &mut call_edges,
            &mut notify_regs,
            &mut idle_handler_regs,
        );
    }

    let form_entries = form_event_handlers
        .iter()
        .map(|h| FormEventEntry {
            event_type: h.event_type.clone(),
            handler_name: Name::new(&h.handler_name),
        })
        .collect();

    ModuleCallSummary { methods, call_edges, notify_regs, idle_handler_regs, form_entries }
}

fn extract_from_body(
    body: &Body,
    source_map: &BodySourceMap,
    caller: CallerId,
    local_method_ids: &FxHashMap<String, u32>,
    call_edges: &mut Vec<CallEdge>,
    notify_regs: &mut Vec<NotifyReg>,
    idle_handler_regs: &mut Vec<IdleReg>,
) {
    for (expr_id, expr) in body.exprs_iter() {
        match expr {
            Expr::Call { callee, args } => {
                let callee_expr = body.expr_idx(*callee);
                let range = source_map.expr_range(expr_id).unwrap_or(TextRange::empty(0.into()));

                match callee_expr {
                    Expr::Path(name) => {
                        let name_lower = name.as_str().to_lowercase();

                        if is_attach_idle_handler(&name_lower) {
                            if let Some(reg) = extract_idle_reg(body, caller, args, range) {
                                idle_handler_regs.push(reg);
                            }
                        } else if let Some(&callee_local_id) = local_method_ids.get(&name_lower) {
                            call_edges.push(CallEdge {
                                caller,
                                target: CallTarget::Local { callee_local_id },
                                kind: EdgeKind::DirectLocal,
                                range,
                            });
                        }
                    }
                    Expr::QualifiedPath(qname) => {
                        if let Some(edge) = qualified_path_to_edge(caller, qname.segments(), range)
                        {
                            call_edges.push(edge);
                        }
                    }
                    Expr::Field { base: field_base, field } => {
                        if let Some(edge) = field_callee_to_edge(
                            body,
                            caller,
                            *field_base,
                            field,
                            range,
                            local_method_ids,
                        ) {
                            call_edges.push(edge);
                        }
                    }
                    _ => {}
                }
            }
            Expr::New { type_name, args } => {
                let offsets = match type_name {
                    Some(tn) if is_notify_description(tn) => Some((0, 1)),
                    None if !args.is_empty() => {
                        if let Expr::Literal(Literal::String(tn)) = body.expr_idx(args[0]) {
                            if is_notify_description_str(tn) {
                                Some((1, 2))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                if let Some((method_idx, target_idx)) = offsets {
                    let range =
                        source_map.expr_range(expr_id).unwrap_or(TextRange::empty(0.into()));
                    if let Some(reg) =
                        extract_notify_reg_at(body, caller, args, method_idx, target_idx, range)
                    {
                        notify_regs.push(reg);
                    }
                }
            }
            _ => {}
        }
    }
}

fn is_attach_idle_handler(name_lower: &str) -> bool {
    name_lower == "подключитьобработчикожидания" || name_lower == "attachidlehandler"
}

fn is_notify_description(name: &Name) -> bool {
    is_notify_description_str(name.as_str())
}

fn is_notify_description_str(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "описаниеоповещения" || lower == "notifydescription"
}

fn is_this_object(name_lower: &str) -> bool {
    name_lower == "этотобъект" || name_lower == "thisobject"
}

fn extract_idle_reg(
    body: &Body,
    caller: CallerId,
    args: &[ExprIdx],
    range: TextRange,
) -> Option<IdleReg> {
    if args.is_empty() {
        return None;
    }
    let handler_name = extract_string_literal(body, args[0])?;
    let one_shot = args
        .get(2)
        .and_then(|&idx| match body.expr_idx(idx) {
            Expr::Literal(Literal::Bool(v)) => Some(*v),
            _ => None,
        })
        .unwrap_or(false);
    Some(IdleReg { caller, handler_name: Name::new(&handler_name), one_shot, range })
}

fn extract_notify_reg_at(
    body: &Body,
    caller: CallerId,
    args: &[ExprIdx],
    method_idx: usize,
    target_idx: usize,
    range: TextRange,
) -> Option<NotifyReg> {
    let callback_name = extract_string_literal(body, *args.get(method_idx)?)?;
    let target_module = args.get(target_idx).and_then(|&idx| match body.expr_idx(idx) {
        Expr::Path(name) if is_this_object(&name.as_str().to_lowercase()) => None,
        Expr::Path(name) => Some(name.clone()),
        _ => None,
    });
    Some(NotifyReg { caller, callback_name: Name::new(&callback_name), target_module, range })
}

fn qualified_path_to_edge(
    caller: CallerId,
    segments: &[Name],
    range: TextRange,
) -> Option<CallEdge> {
    match segments.len() {
        2 => Some(CallEdge {
            caller,
            target: CallTarget::QualifiedModule {
                module_name: segments[0].clone(),
                method_name: segments[1].clone(),
            },
            kind: EdgeKind::DirectQualifiedModule,
            range,
        }),
        3 => {
            let target = if let Some(manager_type) = ManagerType::from_name(segments[0].as_str()) {
                CallTarget::ManagerAccess {
                    manager_type,
                    object_name: segments[1].clone(),
                    method_name: Some(segments[2].clone()),
                }
            } else {
                CallTarget::Unresolved
            };
            Some(CallEdge { caller, target, kind: EdgeKind::DirectQualifiedModule, range })
        }
        _ => None,
    }
}

fn field_callee_to_edge(
    body: &Body,
    caller: CallerId,
    field_base: ExprIdx,
    field: &Name,
    range: TextRange,
    local_method_ids: &FxHashMap<String, u32>,
) -> Option<CallEdge> {
    match body.expr_idx(field_base) {
        Expr::Path(module_name) => {
            let module_name_lower = module_name.as_str().to_lowercase();
            if is_this_object(&module_name_lower) {
                let method_name_lower = field.as_str().to_lowercase();
                let target =
                    if let Some(&callee_local_id) = local_method_ids.get(&method_name_lower) {
                        CallTarget::Local { callee_local_id }
                    } else {
                        tracing::debug!(
                            method_name = field.as_str(),
                            "Unresolved this-object method call in call graph extraction"
                        );
                        CallTarget::ThisObjectMethod { method_name: field.clone() }
                    };
                Some(CallEdge { caller, target, kind: EdgeKind::DirectLocal, range })
            } else {
                Some(CallEdge {
                    caller,
                    target: CallTarget::QualifiedModule {
                        module_name: module_name.clone(),
                        method_name: field.clone(),
                    },
                    kind: EdgeKind::DirectQualifiedModule,
                    range,
                })
            }
        }
        Expr::Field { base: inner_base, field: inner_field } => {
            if let Expr::Path(mdo_type_name) = body.expr_idx(*inner_base) {
                let target =
                    if let Some(manager_type) = ManagerType::from_name(mdo_type_name.as_str()) {
                        CallTarget::ManagerAccess {
                            manager_type,
                            object_name: inner_field.clone(),
                            method_name: Some(field.clone()),
                        }
                    } else {
                        CallTarget::Unresolved
                    };
                Some(CallEdge { caller, target, kind: EdgeKind::DirectQualifiedModule, range })
            } else {
                None
            }
        }
        _ => None,
    }
}

fn extract_string_literal(body: &Body, idx: ExprIdx) -> Option<String> {
    match body.expr_idx(idx) {
        Expr::Literal(Literal::String(s)) => Some(s.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_from_execution_context_maps_each_variant() {
        use crate::ExecutionContext;

        for ctx in [
            ExecutionContext::Server,
            ExecutionContext::ServerCall,
            ExecutionContext::ExternalConnection,
        ] {
            let d = MethodDispatch::from_execution_context(ctx).unwrap();
            assert!(d.is_server_only(), "{ctx:?} is server-only");
        }

        let client = MethodDispatch::from_execution_context(ExecutionContext::Client).unwrap();
        assert!(client.can_run_on_client && !client.can_run_on_server);

        let both = MethodDispatch::from_execution_context(ExecutionContext::ClientServer).unwrap();
        assert!(both.can_run_on_client && both.can_run_on_server);
        assert!(!both.is_server_only());

        // Unknown → no module-level dispatch; caller falls back to annotations.
        assert!(MethodDispatch::from_execution_context(ExecutionContext::Unknown).is_none());
    }

    #[test]
    fn test_dispatch_from_annotation() {
        let d = MethodDispatch::from_annotation(None);
        assert!(d.can_run_on_client);
        assert!(!d.can_run_on_server);
        assert!(!d.is_server_only());

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::AtServer));
        assert!(!d.can_run_on_client);
        assert!(d.can_run_on_server);
        assert!(d.is_server_only());

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::AtClient));
        assert!(d.can_run_on_client);
        assert!(!d.can_run_on_server);
        assert!(!d.is_server_only());

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::AtClientAtServer));
        assert!(d.can_run_on_client);
        assert!(d.can_run_on_server);
        assert!(!d.is_server_only());

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::AtServerNoContext));
        assert!(!d.can_run_on_client);
        assert!(d.can_run_on_server);
        assert!(d.is_server_only());
        assert!(d.no_context);

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::AtClientAtServerNoContext));
        assert!(d.can_run_on_client);
        assert!(d.can_run_on_server);
        assert!(!d.is_server_only());
        assert!(d.no_context);

        let d = MethodDispatch::from_annotation(Some(&AnnotationKind::Before));
        assert!(d.can_run_on_client);
        assert!(!d.can_run_on_server);
    }

    #[test]
    fn test_is_attach_idle_handler() {
        assert!(is_attach_idle_handler("подключитьобработчикожидания"));
        assert!(is_attach_idle_handler("attachidlehandler"));
        assert!(!is_attach_idle_handler("отключитьобработчикожидания"));
    }

    #[test]
    fn test_is_notify_description() {
        assert!(is_notify_description(&Name::new("ОписаниеОповещения")));
        assert!(is_notify_description(&Name::new("NotifyDescription")));
        assert!(is_notify_description(&Name::new("описаниеоповещения")));
        assert!(!is_notify_description(&Name::new("ОписаниеОшибки")));
    }

    #[test]
    fn test_is_this_object() {
        assert!(is_this_object("этотобъект"));
        assert!(is_this_object("thisobject"));
        assert!(!is_this_object("другойобъект"));
    }

    #[test]
    fn test_manager_type_from_name() {
        assert_eq!(ManagerType::from_name("Документы"), Some(ManagerType::Documents));
        assert_eq!(ManagerType::from_name("Documents"), Some(ManagerType::Documents));
        assert_eq!(ManagerType::from_name("документы"), Some(ManagerType::Documents));
        assert_eq!(ManagerType::from_name("Справочники"), Some(ManagerType::Catalogs));
        assert_eq!(ManagerType::from_name("catalogs"), Some(ManagerType::Catalogs));
        assert_eq!(ManagerType::from_name("НеизвестныйТип"), None);
    }

    #[test]
    fn extract_graph_methods_reports_ranges_dispatch_and_export() {
        let code = "&НаСервере\n\
                    Функция Считать() Экспорт\n\
                    Возврат 1;\n\
                    КонецФункции\n\
                    \n\
                    Процедура Делать()\n\
                    КонецПроцедуры";
        let parse = parser::parse(code);
        let item_tree = ItemTree::from_parse(&parse);
        let methods = extract_graph_methods(&item_tree);
        assert_eq!(methods.len(), 2);

        let read = &methods[0];
        assert_eq!(read.local_id, 0);
        assert_eq!(read.name.as_str(), "Считать");
        assert!(read.is_export);
        assert!(read.dispatch.is_server_only());
        // The name range pinpoints the identifier; the source range spans the
        // whole declaration — both index back into the original text.
        let name_slice =
            &code[usize::from(read.name_range.start())..usize::from(read.name_range.end())];
        assert_eq!(name_slice, "Считать");
        let source_slice =
            &code[usize::from(read.source_range.start())..usize::from(read.source_range.end())];
        assert!(source_slice.contains("Функция Считать"));
        assert!(source_slice.contains("КонецФункции"));

        let act = &methods[1];
        assert_eq!(act.name.as_str(), "Делать");
        assert!(!act.is_export);
        assert!(act.dispatch.can_run_on_client && !act.dispatch.can_run_on_server);
    }

    #[test]
    fn extract_graph_methods_matches_call_summary_method_list() {
        // The two enumerations must agree: `extract_call_summary` reuses
        // `extract_graph_methods`, so the body-free index is the source of truth
        // for local ids, names, dispatch, and export.
        let code = "Процедура Альфа() Экспорт\n\
                    КонецПроцедуры\n\
                    Функция Бета()\n\
                    Возврат 0;\n\
                    КонецФункции";
        let parse = parser::parse(code);
        let item_tree = ItemTree::from_parse(&parse);
        let entries = extract_graph_methods(&item_tree);
        let summary = parse_and_extract(code);

        assert_eq!(entries.len(), summary.methods.len());
        for (entry, summary_method) in entries.iter().zip(&summary.methods) {
            assert_eq!(entry.local_id, summary_method.local_id);
            assert_eq!(entry.name, summary_method.name);
            assert_eq!(entry.is_export, summary_method.is_export);
            assert_eq!(entry.dispatch, summary_method.dispatch);
        }
    }

    #[test]
    fn test_extract_call_summary_empty_module() {
        let item_tree = ItemTree::default();
        let module_bodies = ModuleBodies::new();
        let summary = extract_call_summary(&item_tree, &module_bodies, &[]);

        assert!(summary.methods.is_empty());
        assert!(summary.call_edges.is_empty());
        assert!(summary.notify_regs.is_empty());
        assert!(summary.idle_handler_regs.is_empty());
        assert!(summary.form_entries.is_empty());
    }

    #[test]
    fn test_extract_form_entries() {
        let item_tree = ItemTree::default();
        let module_bodies = ModuleBodies::new();
        let handlers = vec![
            bsl_metadata::FormEventHandler {
                event_type: "OnCreateAtServer".to_string(),
                handler_name: "ПриСозданииНаСервере".to_string(),
            },
            bsl_metadata::FormEventHandler {
                event_type: "OnActivateRow".to_string(),
                handler_name: "СписокПриАктивизацииСтроки".to_string(),
            },
        ];
        let summary = extract_call_summary(&item_tree, &module_bodies, &handlers);

        assert_eq!(summary.form_entries.len(), 2);
        assert_eq!(summary.form_entries[0].event_type, "OnCreateAtServer");
        assert_eq!(summary.form_entries[0].handler_name, Name::new("ПриСозданииНаСервере"));
        assert_eq!(summary.form_entries[1].event_type, "OnActivateRow");
    }

    fn parse_and_extract(code: &str) -> ModuleCallSummary {
        parse_and_extract_with_handlers(code, &[])
    }

    fn parse_and_extract_with_handlers(
        code: &str,
        handlers: &[bsl_metadata::FormEventHandler],
    ) -> ModuleCallSummary {
        let parse = parser::parse(code);
        let item_tree = ItemTree::from_parse(&parse);
        let module_id = crate::ModuleId::new(vfs::FileId(0));
        let module_bodies = ModuleBodies::from_parse(&parse, module_id);
        extract_call_summary(&item_tree, &module_bodies, handlers)
    }

    #[test]
    fn test_handler_to_local_client_to_server_chain() {
        let code = r#"
&НаКлиенте
Процедура ОбработчикСобытия()
    КлиентскийМетод();
КонецПроцедуры

&НаКлиенте
Процедура КлиентскийМетод()
    СерверныйМетод();
КонецПроцедуры

&НаСервере
Процедура СерверныйМетод()
    // server logic
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.methods.len(), 3);
        assert!(summary.methods[0].dispatch.can_run_on_client);
        assert!(summary.methods[2].dispatch.is_server_only());

        let local_edges: Vec<_> =
            summary.call_edges.iter().filter(|e| e.kind == EdgeKind::DirectLocal).collect();
        assert_eq!(local_edges.len(), 2);

        assert_eq!(local_edges[0].caller, CallerId::Method(0));
        assert!(matches!(&local_edges[0].target, CallTarget::Local { callee_local_id: 1 }));

        assert_eq!(local_edges[1].caller, CallerId::Method(1));
        assert!(matches!(&local_edges[1].target, CallTarget::Local { callee_local_id: 2 }));
    }

    #[test]
    fn test_qualified_call_to_common_module() {
        let code = r#"
Процедура МойМетод()
    ОбщийМодуль.ВнешнийМетод();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        let qual_edges: Vec<_> = summary
            .call_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DirectQualifiedModule)
            .collect();
        assert_eq!(qual_edges.len(), 1);
        assert!(matches!(
            &qual_edges[0].target,
            CallTarget::QualifiedModule { module_name, method_name }
                if module_name.as_str() == "ОбщийМодуль"
                    && method_name.as_str() == "ВнешнийМетод"
        ));
    }

    #[test]
    fn test_this_object_call_normalized_to_direct_local() {
        let code = r#"
Процедура A()
    ЭтотОбъект.B();
КонецПроцедуры

Процедура B()
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        let b_id = summary
            .methods
            .iter()
            .find(|method| method.name.as_str() == "B")
            .expect("B method must be extracted")
            .local_id;
        assert_eq!(summary.call_edges.len(), 1);
        assert_eq!(summary.call_edges[0].kind, EdgeKind::DirectLocal);
        assert_eq!(summary.call_edges[0].target, CallTarget::Local { callee_local_id: b_id });
    }

    #[test]
    fn test_this_object_call_english_normalized() {
        let code = r#"
Procedure A()
    ThisObject.B();
EndProcedure

Procedure B()
EndProcedure
"#;
        let summary = parse_and_extract(code);

        let b_id = summary
            .methods
            .iter()
            .find(|method| method.name.as_str() == "B")
            .expect("B method must be extracted")
            .local_id;
        assert_eq!(summary.call_edges.len(), 1);
        assert_eq!(summary.call_edges[0].kind, EdgeKind::DirectLocal);
        assert_eq!(summary.call_edges[0].target, CallTarget::Local { callee_local_id: b_id });
    }

    #[test]
    fn test_this_object_call_unknown_method_emits_this_object_method_variant() {
        let code = r#"
Процедура A()
    ЭтотОбъект.Unknown();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.call_edges.len(), 1);
        assert_eq!(summary.call_edges[0].kind, EdgeKind::DirectLocal);
        assert_eq!(
            summary.call_edges[0].target,
            CallTarget::ThisObjectMethod { method_name: Name::new("Unknown") }
        );
    }

    #[test]
    fn test_qualified_module_call_unchanged() {
        let code = r#"
Процедура Тест()
    СоседнийМодуль.Метод();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.call_edges.len(), 1);
        assert_eq!(summary.call_edges[0].kind, EdgeKind::DirectQualifiedModule);
        assert!(matches!(
            &summary.call_edges[0].target,
            CallTarget::QualifiedModule { module_name, method_name }
                if module_name.as_str() == "СоседнийМодуль"
                    && method_name.as_str() == "Метод"
        ));
    }

    #[test]
    fn test_no_duplicate_edges_for_single_qualified_call() {
        let code = r#"
Процедура Тест()
    ОбщийМодуль.Метод();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        let qual_edges: Vec<_> = summary
            .call_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DirectQualifiedModule)
            .collect();
        assert_eq!(qual_edges.len(), 1, "Should be exactly 1 edge, no duplicates");
    }

    #[test]
    fn test_qualified_call_to_nonexistent_method_still_produces_edge() {
        let code = r#"
Процедура Тест()
    НесуществующийМодуль.НесуществующийМетод();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        let qual_edges: Vec<_> = summary
            .call_edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DirectQualifiedModule)
            .collect();
        assert_eq!(qual_edges.len(), 1);
        assert!(matches!(
            &qual_edges[0].target,
            CallTarget::QualifiedModule { module_name, method_name }
                if module_name.as_str() == "НесуществующийМодуль"
                    && method_name.as_str() == "НесуществующийМетод"
        ));
    }

    #[test]
    fn test_notify_description_in_module_code() {
        let code = r#"
Перем Оповещение;
Оповещение = Новый ОписаниеОповещения("ОбработатьОповещение", ЭтотОбъект);

Процедура ОбработатьОповещение(Результат, ДопПараметры) Экспорт
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.notify_regs.len(), 1);
        assert_eq!(summary.notify_regs[0].caller, CallerId::ModuleCode);
        assert_eq!(summary.notify_regs[0].callback_name, Name::new("ОбработатьОповещение"));
        assert!(summary.notify_regs[0].target_module.is_none(), "ЭтотОбъект means current module");
    }

    #[test]
    fn test_notify_description_english() {
        let code = r#"
Procedure Test()
    Notification = New NotifyDescription("HandleResult", ThisObject);
EndProcedure

Procedure HandleResult(Result, AdditionalParameters) Export
EndProcedure
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.notify_regs.len(), 1);
        assert_eq!(summary.notify_regs[0].caller, CallerId::Method(0));
        assert_eq!(summary.notify_regs[0].callback_name, Name::new("HandleResult"));
        assert!(summary.notify_regs[0].target_module.is_none());
    }

    #[test]
    fn test_manager_access_three_segment() {
        let code = r#"
Процедура Тест()
    Документы.ПриходнаяНакладная.СоздатьЭлемент();
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        let manager_edges: Vec<_> = summary
            .call_edges
            .iter()
            .filter(|e| matches!(&e.target, CallTarget::ManagerAccess { .. }))
            .collect();
        assert_eq!(manager_edges.len(), 1);
        assert!(matches!(
            &manager_edges[0].target,
            CallTarget::ManagerAccess {
                manager_type: ManagerType::Documents,
                object_name,
                method_name: Some(method),
            } if object_name.as_str() == "ПриходнаяНакладная"
                && method.as_str() == "СоздатьЭлемент"
        ));
    }

    #[test]
    fn test_idle_handler_not_edge() {
        let code = r#"
&НаКлиенте
Процедура ПриОткрытии()
    ПодключитьОбработчикОжидания("Обновить", 5, Истина);
КонецПроцедуры

&НаКлиенте
Процедура Обновить()
    // update
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);

        assert_eq!(summary.idle_handler_regs.len(), 1);
        assert_eq!(summary.idle_handler_regs[0].handler_name, Name::new("Обновить"));
        assert!(summary.idle_handler_regs[0].one_shot);
        assert_eq!(summary.idle_handler_regs[0].caller, CallerId::Method(0));

        let local_edges: Vec<_> =
            summary.call_edges.iter().filter(|e| e.kind == EdgeKind::DirectLocal).collect();
        assert!(local_edges.is_empty(), "Idle handler registration should not produce a call edge");
    }

    #[test]
    fn test_method_dispatch_from_code() {
        let code = r#"
&НаКлиенте
Процедура КлиентМетод()
КонецПроцедуры

&НаСервере
Функция СерверФункция()
    Возврат 1;
КонецФункции

&НаКлиентеНаСервереБезКонтекста
Функция ОбщаяФункция()
    Возврат 2;
КонецФункции

Процедура БезАннотации()
КонецПроцедуры
"#;
        let summary = parse_and_extract(code);
        assert_eq!(summary.methods.len(), 4);

        assert!(summary.methods[0].dispatch.can_run_on_client);
        assert!(!summary.methods[0].dispatch.can_run_on_server);

        assert!(summary.methods[1].dispatch.is_server_only());

        assert!(summary.methods[2].dispatch.can_run_on_client);
        assert!(summary.methods[2].dispatch.can_run_on_server);
        assert!(summary.methods[2].dispatch.no_context);

        assert!(summary.methods[3].dispatch.can_run_on_client);
        assert!(!summary.methods[3].dispatch.can_run_on_server);
    }
}
