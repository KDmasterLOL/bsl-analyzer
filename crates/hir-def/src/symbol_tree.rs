use crate::docs::{compute_variable_docs_with_node, MethodDocs, VariableDocs};
use crate::item_tree::{Annotation, ItemTree, ModItem, Param};
use crate::ty::doc_types::{parse_method_doc_types, MethodTypeHints};
use crate::type_ref::TypeRef;
use crate::{MethodId, ModuleId, Name, VariableId};
use la_arena::{Arena, Idx};
use rustc_hash::FxHashMap;
use smol_str::SmolStr;
use std::sync::Arc;
use stdx::case::CaseExt;
use syntax::{Parse, SyntaxKind, SyntaxNode};
use text_size::TextRange;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolTree {
    methods: Arena<MethodSymbol>,

    variables: Arena<VariableSymbol>,

    methods_by_name: FxHashMap<SmolStr, Vec<Idx<MethodSymbol>>>,

    variables_by_name: FxHashMap<SmolStr, Vec<Idx<VariableSymbol>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSymbol {
    pub id: MethodId,

    pub name: Name,

    pub is_function: bool,

    pub is_export: bool,

    pub params: Vec<ParamSymbol>,

    pub annotations: Vec<Annotation>,

    pub source_range: TextRange,

    pub docs: Option<Arc<MethodDocs>>,

    pub return_type_ref: Option<TypeRef>,
}

impl MethodSymbol {
    pub fn syntax_node(&self, parse: &Parse<SyntaxNode>) -> Option<SyntaxNode> {
        let expected_kind =
            if self.is_function { SyntaxKind::FUNCTION_DEF } else { SyntaxKind::PROCEDURE_DEF };
        let target = self.source_range;
        let root = parse.syntax_node();
        if !root.text_range().contains_range(target) {
            return None;
        }
        // Descend by range containment instead of scanning every descendant: the
        // covering element of the method's full range is the method node itself,
        // possibly below same-range wrapper nodes (climbed back off here).
        let mut node = match root.covering_element(target) {
            syntax::NodeOrToken::Node(n) => n,
            syntax::NodeOrToken::Token(t) => t.parent()?,
        };
        let mut found = None;
        loop {
            if node.text_range() == target && node.kind() == expected_kind {
                found = Some(node.clone());
            }
            match node.parent() {
                Some(parent) if parent.text_range() == target => node = parent,
                _ => break,
            }
        }
        found
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariableSymbol {
    pub id: VariableId,

    pub name: Name,

    pub is_export: bool,

    pub annotations: Vec<Annotation>,

    pub source_range: TextRange,

    pub name_range: TextRange,

    pub docs: Option<Arc<VariableDocs>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSymbol {
    pub name: Name,

    pub is_val: bool,

    pub has_default: bool,

    pub type_ref: Option<TypeRef>,
}

impl SymbolTree {
    pub fn from_item_tree(
        item_tree: &ItemTree,
        module_id: ModuleId,
        parse: &syntax::Parse<syntax::SyntaxNode>,
        source_text: &str,
    ) -> Self {
        let mut builder = SymbolTreeBuilder::new(module_id, parse, source_text, item_tree);

        for (idx, item) in item_tree.top_level_items().iter().enumerate() {
            let local_id = idx as u32;

            match item {
                ModItem::Procedure(proc_idx) => {
                    let proc = item_tree.procedure(*proc_idx);
                    builder.add_procedure(local_id, proc);
                }
                ModItem::Function(func_idx) => {
                    let func = item_tree.function(*func_idx);
                    builder.add_function(local_id, func);
                }
                ModItem::Variable(var_idx) => {
                    let var = item_tree.variable(*var_idx);
                    builder.add_variable(local_id, var);
                }
            }
        }

        builder.build()
    }

    #[cfg(test)]
    pub fn from_item_tree_no_docs(item_tree: &ItemTree, module_id: ModuleId) -> Self {
        let mut methods = Arena::new();
        let mut variables = Arena::new();
        let mut methods_by_name: FxHashMap<SmolStr, Vec<Idx<MethodSymbol>>> = FxHashMap::default();
        let mut variables_by_name: FxHashMap<SmolStr, Vec<Idx<VariableSymbol>>> =
            FxHashMap::default();

        for (idx, item) in item_tree.top_level_items().iter().enumerate() {
            let local_id = idx as u32;

            match item {
                ModItem::Procedure(proc_idx) => {
                    let proc = item_tree.procedure(*proc_idx);
                    let method_id = MethodId { module: module_id, local_id };
                    let symbol = MethodSymbol {
                        id: method_id,
                        name: proc.name.clone(),
                        is_function: false,
                        is_export: proc.is_export,
                        params: proc.params.iter().map(ParamSymbol::from).collect(),
                        annotations: proc.annotations.to_vec(),
                        source_range: proc.source_range,
                        docs: None,
                        return_type_ref: None,
                    };
                    let key: SmolStr = symbol.name.as_str().fold_lower().into();
                    let idx = methods.alloc(symbol);
                    methods_by_name.entry(key).or_default().push(idx);
                }
                ModItem::Function(func_idx) => {
                    let func = item_tree.function(*func_idx);
                    let method_id = MethodId { module: module_id, local_id };
                    let symbol = MethodSymbol {
                        id: method_id,
                        name: func.name.clone(),
                        is_function: true,
                        is_export: func.is_export,
                        params: func.params.iter().map(ParamSymbol::from).collect(),
                        annotations: func.annotations.to_vec(),
                        source_range: func.source_range,
                        docs: None,
                        return_type_ref: None,
                    };
                    let key: SmolStr = symbol.name.as_str().fold_lower().into();
                    let idx = methods.alloc(symbol);
                    methods_by_name.entry(key).or_default().push(idx);
                }
                ModItem::Variable(var_idx) => {
                    let var = item_tree.variable(*var_idx);
                    let key: SmolStr = var.name.as_str().fold_lower().into();
                    let variable_id = VariableId { module: module_id, local_id };
                    let symbol = VariableSymbol {
                        id: variable_id,
                        name: var.name.clone(),
                        is_export: var.is_export,
                        annotations: var.annotations.to_vec(),
                        source_range: var.source_range,
                        name_range: var.name_range,
                        docs: None,
                    };
                    let idx = variables.alloc(symbol);
                    let entry = variables_by_name.entry(key).or_default();
                    if entry.is_empty() {
                        entry.push(idx);
                    }
                }
            }
        }

        SymbolTree { methods, variables, methods_by_name, variables_by_name }
    }

    pub fn find_method(&self, name: &Name) -> Option<&MethodSymbol> {
        let key = name.as_str().fold_lower();
        let indices = self.methods_by_name.get(key.as_str())?;
        indices.first().map(|&idx| &self.methods[idx])
    }

    pub fn find_methods(&self, name: &Name) -> Vec<&MethodSymbol> {
        let key = name.as_str().fold_lower();
        self.methods_by_name
            .get(key.as_str())
            .map(|indices| indices.iter().map(|&idx| &self.methods[idx]).collect())
            .unwrap_or_default()
    }

    pub fn find_variable(&self, name: &Name) -> Option<&VariableSymbol> {
        let key = name.as_str().fold_lower();
        let indices = self.variables_by_name.get(key.as_str())?;
        indices.first().map(|&idx| &self.variables[idx])
    }

    pub fn methods(&self) -> impl Iterator<Item = &MethodSymbol> {
        self.methods.iter().map(|(_, m)| m)
    }

    pub fn exported_methods(&self) -> impl Iterator<Item = &MethodSymbol> {
        self.methods().filter(|m| m.is_export)
    }

    pub fn variables(&self) -> impl Iterator<Item = &VariableSymbol> {
        self.variables.iter().map(|(_, v)| v)
    }

    pub fn exported_variables(&self) -> impl Iterator<Item = &VariableSymbol> {
        self.variables().filter(|v| v.is_export)
    }

    pub fn find_method_by_id(&self, method_id: MethodId) -> Option<&MethodSymbol> {
        self.methods().find(|m| m.id == method_id)
    }

    pub fn find_variable_by_id(&self, variable_id: VariableId) -> Option<&VariableSymbol> {
        self.variables().find(|v| v.id == variable_id)
    }
}

struct SymbolTreeBuilder<'a> {
    module_id: ModuleId,
    parse: &'a syntax::Parse<syntax::SyntaxNode>,
    source_text: &'a str,
    item_tree: &'a ItemTree,
    methods: Arena<MethodSymbol>,
    variables: Arena<VariableSymbol>,
    methods_by_name: FxHashMap<SmolStr, Vec<Idx<MethodSymbol>>>,
    variables_by_name: FxHashMap<SmolStr, Vec<Idx<VariableSymbol>>>,
    var_def_nodes: FxHashMap<TextRange, syntax::SyntaxNode>,
}

impl<'a> SymbolTreeBuilder<'a> {
    fn new(
        module_id: ModuleId,
        parse: &'a syntax::Parse<syntax::SyntaxNode>,
        source_text: &'a str,
        item_tree: &'a ItemTree,
    ) -> Self {
        let var_def_nodes: FxHashMap<TextRange, syntax::SyntaxNode> = parse
            .syntax_node()
            .descendants()
            .filter(|n| n.kind() == syntax::SyntaxKind::VAR_DEF)
            .map(|n| (n.text_range(), n))
            .collect();

        Self {
            module_id,
            parse,
            source_text,
            item_tree,
            methods: Arena::new(),
            variables: Arena::new(),
            methods_by_name: FxHashMap::default(),
            variables_by_name: FxHashMap::default(),
            var_def_nodes,
        }
    }

    fn add_procedure(&mut self, local_id: u32, proc: &crate::item_tree::Procedure) {
        let method_id = MethodId { module: self.module_id, local_id };

        let docs = self.parse_method_docs(method_id);
        let hints = docs.as_deref().and_then(|d| parse_method_doc_types(&d.raw));

        let symbol = MethodSymbol {
            id: method_id,
            name: proc.name.clone(),
            is_function: false,
            is_export: proc.is_export,
            params: Self::params_with_hints(&proc.params, hints.as_ref()),
            annotations: proc.annotations.to_vec(),
            source_range: proc.source_range,
            return_type_ref: hints.as_ref().map(|h| h.ret.clone()),
            docs,
        };

        self.add_method_symbol(symbol);
    }

    fn add_function(&mut self, local_id: u32, func: &crate::item_tree::Function) {
        let method_id = MethodId { module: self.module_id, local_id };

        let docs = self.parse_method_docs(method_id);
        let hints = docs.as_deref().and_then(|d| parse_method_doc_types(&d.raw));

        let symbol = MethodSymbol {
            id: method_id,
            name: func.name.clone(),
            is_function: true,
            is_export: func.is_export,
            params: Self::params_with_hints(&func.params, hints.as_ref()),
            annotations: func.annotations.to_vec(),
            source_range: func.source_range,
            return_type_ref: hints.as_ref().map(|h| h.ret.clone()),
            docs,
        };

        self.add_method_symbol(symbol);
    }

    fn params_with_hints(params: &[Param], hints: Option<&MethodTypeHints>) -> Vec<ParamSymbol> {
        params
            .iter()
            .map(|p| {
                let mut sym = ParamSymbol::from(p);
                if let Some(hints) = hints {
                    sym.type_ref = hints
                        .params
                        .iter()
                        .find(|(n, _)| p.name.eq_ignore_case(n))
                        .map(|(_, t)| t.clone());
                }
                sym
            })
            .collect()
    }

    fn parse_method_docs(&self, method_id: MethodId) -> Option<Arc<MethodDocs>> {
        crate::docs::compute_method_docs(self.parse, self.item_tree, method_id, self.source_text)
    }

    fn add_method_symbol(&mut self, symbol: MethodSymbol) {
        let key: SmolStr = symbol.name.as_str().fold_lower().into();
        let idx = self.methods.alloc(symbol);

        self.methods_by_name.entry(key).or_default().push(idx);
    }

    fn add_variable(&mut self, local_id: u32, var: &crate::item_tree::Variable) {
        let key: SmolStr = var.name.as_str().fold_lower().into();
        let variable_id = VariableId { module: self.module_id, local_id };

        let var_node = self.var_def_nodes.get(&var.source_range);
        debug_assert!(
            var_node.is_some(),
            "VAR_DEF for variable {:?} not found in parse tree (range = {:?})",
            var.name,
            var.source_range,
        );
        let docs = match var_node {
            Some(node) => compute_variable_docs_with_node(node, var, self.source_text),
            None => crate::docs::compute_variable_docs(
                self.parse,
                self.item_tree,
                variable_id,
                self.source_text,
            ),
        };

        let symbol = VariableSymbol {
            id: variable_id,
            name: var.name.clone(),
            is_export: var.is_export,
            annotations: var.annotations.to_vec(),
            source_range: var.source_range,
            name_range: var.name_range,
            docs,
        };

        let idx = self.variables.alloc(symbol);
        let entry = self.variables_by_name.entry(key).or_default();
        if entry.is_empty() {
            entry.push(idx);
        }
    }

    fn build(self) -> SymbolTree {
        SymbolTree {
            methods: self.methods,
            variables: self.variables,
            methods_by_name: self.methods_by_name,
            variables_by_name: self.variables_by_name,
        }
    }
}

impl From<&Param> for ParamSymbol {
    fn from(param: &Param) -> Self {
        ParamSymbol {
            name: param.name.clone(),
            is_val: param.is_val,
            has_default: param.has_default,
            type_ref: None,
        }
    }
}

/// Rough live bytes of a [`SymbolTree`] for Salsa's `memory_usage` introspection:
/// the method/variable arenas with each symbol's name, param-symbol vec and
/// annotation vec, plus the two name-index maps and their `SmolStr` keys.
/// `docs` (`Option<Arc<…Docs>>`) counts only as the inline `Arc` pointer already
/// in the arena element — the pointee is shared, so its heap is not re-attributed
/// here. Nested `TypeRef` payloads inside params are likewise left at their
/// inline size; both keep the estimate simple at the cost of a mild undercount.
pub(crate) fn symbol_tree_heap(v: &Arc<SymbolTree>) -> usize {
    use crate::heap_estimate::{map_table_bytes, name_bytes, smol_str_bytes, vec_bytes};

    let tree = &**v;
    let mut bytes = std::mem::size_of::<SymbolTree>();

    bytes += vec_bytes::<MethodSymbol>(tree.methods.len());
    for method in tree.methods.values() {
        bytes += name_bytes(&method.name);
        bytes += vec_bytes::<ParamSymbol>(method.params.len());
        for param in &method.params {
            bytes += name_bytes(&param.name);
        }
        bytes += vec_bytes::<Annotation>(method.annotations.len());
    }

    bytes += vec_bytes::<VariableSymbol>(tree.variables.len());
    for variable in tree.variables.values() {
        bytes += name_bytes(&variable.name);
        bytes += vec_bytes::<Annotation>(variable.annotations.len());
    }

    bytes += map_table_bytes::<SmolStr, Vec<Idx<MethodSymbol>>>(tree.methods_by_name.len());
    for (key, idxs) in &tree.methods_by_name {
        bytes += smol_str_bytes(key.len()) + vec_bytes::<Idx<MethodSymbol>>(idxs.len());
    }

    bytes += map_table_bytes::<SmolStr, Vec<Idx<VariableSymbol>>>(tree.variables_by_name.len());
    for (key, idxs) in &tree.variables_by_name {
        bytes += smol_str_bytes(key.len()) + vec_bytes::<Idx<VariableSymbol>>(idxs.len());
    }

    bytes
}

// Condensed per-module symbol list (no green-tree pin): on the cross-module call
// resolution path. High cap keeps it across chunk-boundary LRU trims so a later
// chunk resolving a call into this module doesn't re-derive it (re-parse + lower).
#[salsa::tracked(lru = 2048, heap_size = crate::symbol_tree::symbol_tree_heap)]
pub fn symbol_tree_query<'db>(
    db: &'db dyn crate::DefDatabase,
    file_id_input: base_db::FileIdInput<'db>,
) -> std::sync::Arc<SymbolTree> {
    let _span = tracing::info_span!("symbol_tree", ?file_id_input).entered();
    let file_id = file_id_input.file_id(db);
    let item_tree = db.item_tree(file_id);
    let parse = db.parse_ref(file_id);
    let source_text = db.file_text(file_id);
    let module_id = crate::ModuleId::new(file_id);
    std::sync::Arc::new(SymbolTree::from_item_tree(&item_tree, module_id, parse, &source_text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item_tree::{Function, Procedure, Variable};
    use crate::ModuleId;
    use text_size::TextSize;
    use vfs::FileId;

    fn make_text_range(start: u32, end: u32) -> TextRange {
        TextRange::new(TextSize::from(start), TextSize::from(end))
    }

    #[test]
    fn test_symbol_tree_basic() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Первая"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let func_idx = item_tree.functions.alloc(Function {
            name: Name::new("Вторая"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(20, 30),
            name_range: make_text_range(20, 30),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Function(func_idx));

        let var_idx = item_tree.variables.alloc(Variable {
            name: Name::new("МодульнаяПеременная"),
            is_export: true,
            annotations: Box::new([]),
            source_range: make_text_range(40, 50),
            name_range: make_text_range(40, 50),
        });
        item_tree.top_level.push(ModItem::Variable(var_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert_eq!(symbol_tree.methods().count(), 2);

        let first = symbol_tree.find_method(&Name::new("Первая")).unwrap();
        assert_eq!(first.name.as_str(), "Первая");
        assert!(!first.is_function);
        assert!(!first.is_export);

        let second = symbol_tree.find_method(&Name::new("Вторая")).unwrap();
        assert_eq!(second.name.as_str(), "Вторая");
        assert!(second.is_function);
        assert!(second.is_export);

        assert_eq!(symbol_tree.variables().count(), 1);

        let var = symbol_tree.find_variable(&Name::new("МодульнаяПеременная")).unwrap();
        assert_eq!(var.name.as_str(), "МодульнаяПеременная");
        assert!(var.is_export);
    }

    #[test]
    fn test_case_insensitive_lookup() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("МояПроцедура"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert!(symbol_tree.find_method(&Name::new("МояПроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("мояпроцедура")).is_some());
        assert!(symbol_tree.find_method(&Name::new("МОЯПРОЦЕДУРА")).is_some());
        assert!(symbol_tree.find_method(&Name::new("МоЯпРоЦеДуРа")).is_some());

        let m1 = symbol_tree.find_method(&Name::new("МояПроцедура")).unwrap();
        let m2 = symbol_tree.find_method(&Name::new("мояпроцедура")).unwrap();
        assert_eq!(m1.id, m2.id);
    }

    #[test]
    fn test_exported_methods_filter() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc1_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Приватная"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc1_idx));

        let proc2_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Публичная"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(20, 30),
            name_range: make_text_range(20, 30),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc2_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert_eq!(symbol_tree.methods().count(), 2);

        let exported: Vec<_> = symbol_tree.exported_methods().collect();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name.as_str(), "Публичная");
    }

    #[test]
    fn test_method_with_parameters() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("СПараметрами"),
            is_export: false,
            params: Box::new([
                Param {
                    name: Name::new("Параметр1"),
                    is_val: true,
                    has_default: false,
                    default_value: None,
                    name_range: make_text_range(0, 10),
                },
                Param {
                    name: Name::new("Параметр2"),
                    is_val: false,
                    has_default: true,
                    default_value: None,
                    name_range: make_text_range(12, 22),
                },
            ]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: Some(make_text_range(0, 10)),
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        let method = symbol_tree.find_method(&Name::new("СПараметрами")).unwrap();
        assert_eq!(method.params.len(), 2);
        assert_eq!(method.params[0].name.as_str(), "Параметр1");
        assert!(method.params[0].is_val);
        assert!(!method.params[0].has_default);
        assert_eq!(method.params[1].name.as_str(), "Параметр2");
        assert!(!method.params[1].is_val);
        assert!(method.params[1].has_default);
    }

    #[test]
    fn test_method_with_annotations() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("НаКлиенте"),
            is_export: true,
            params: Box::new([]),
            annotations: Box::new([Annotation {
                kind: crate::item_tree::AnnotationKind::AtClient,
                range: make_text_range(0, 10),
            }]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        let method = symbol_tree.find_method(&Name::new("НаКлиенте")).unwrap();
        assert_eq!(method.annotations.len(), 1);
        assert_eq!(method.annotations[0].kind, crate::item_tree::AnnotationKind::AtClient);
    }

    #[test]
    fn test_not_found() {
        let item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert!(symbol_tree.find_method(&Name::new("НесуществующаяПроцедура")).is_none());
        assert!(symbol_tree.find_variable(&Name::new("НесуществующаяПеременная")).is_none());
    }

    #[test]
    fn test_empty_symbol_tree() {
        let item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert_eq!(symbol_tree.methods().count(), 0);
        assert_eq!(symbol_tree.variables().count(), 0);
        assert_eq!(symbol_tree.exported_methods().count(), 0);
        assert_eq!(symbol_tree.exported_variables().count(), 0);
    }

    #[test]
    fn test_variable_case_insensitive() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let var_idx = item_tree.variables.alloc(Variable {
            name: Name::new("МояПеременная"),
            is_export: false,
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Variable(var_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert!(symbol_tree.find_variable(&Name::new("МояПеременная")).is_some());
        assert!(symbol_tree.find_variable(&Name::new("мояпеременная")).is_some());
        assert!(symbol_tree.find_variable(&Name::new("МОЯПЕРЕМЕННАЯ")).is_some());
    }

    #[test]
    fn test_exported_variables_filter() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let var1_idx = item_tree.variables.alloc(Variable {
            name: Name::new("Приватная"),
            is_export: false,
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
        });
        item_tree.top_level.push(ModItem::Variable(var1_idx));

        let var2_idx = item_tree.variables.alloc(Variable {
            name: Name::new("Публичная"),
            is_export: true,
            annotations: Box::new([]),
            source_range: make_text_range(20, 30),
            name_range: make_text_range(20, 30),
        });
        item_tree.top_level.push(ModItem::Variable(var2_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        assert_eq!(symbol_tree.variables().count(), 2);

        let exported: Vec<_> = symbol_tree.exported_variables().collect();
        assert_eq!(exported.len(), 1);
        assert_eq!(exported[0].name.as_str(), "Публичная");
    }

    #[test]
    fn test_find_methods_multiple() {
        let mut item_tree = ItemTree::default();
        let file_id = FileId(0);
        let module_id = ModuleId::new(file_id);

        let proc_idx = item_tree.procedures.alloc(Procedure {
            name: Name::new("Метод"),
            is_export: false,
            params: Box::new([]),
            annotations: Box::new([]),
            source_range: make_text_range(0, 10),
            name_range: make_text_range(0, 10),
            param_list_range: None,
            sig_end: make_text_range(0, 10).end(),
        });
        item_tree.top_level.push(ModItem::Procedure(proc_idx));

        let symbol_tree = SymbolTree::from_item_tree_no_docs(&item_tree, module_id);

        let methods = symbol_tree.find_methods(&Name::new("Метод"));
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].name.as_str(), "Метод");

        let not_found = symbol_tree.find_methods(&Name::new("НеСуществует"));
        assert_eq!(not_found.len(), 0);
    }
}
