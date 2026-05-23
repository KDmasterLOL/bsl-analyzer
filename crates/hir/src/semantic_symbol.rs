//! Type-aware semantic symbols shared by IDE features.
//!
//! This module is the bridge between syntax positions and semantic identity.
//! IDE crates should ask for a `SemanticSymbol` and project it to their own
//! wire shape instead of duplicating name-resolution and type-dispatch rules.

use crate::{Definition, Name, NameClass, ReferenceScope, Semantics};
use hir_def::{DefDatabase, DefWithBodyId, ExprId, MethodId, ModuleId};
use hir_ty::{db::HirDatabase, ImplicitLocalInfo, Ty};
use syntax::{TextRange, TextSize};
use vfs::FileId;

/// Stable identity for comparing symbol occurrences.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SemanticSymbolKey {
    Definition(Definition),
    BodyLocal { file_id: FileId, owner: DefWithBodyId, name_lower: String },
    ImplicitLocal { file_id: FileId, owner: DefWithBodyId, name_lower: String, declaration: ExprId },
    TypedMember { file_id: FileId, range: TextRange },
}

/// Coarse semantic kind used by IDE projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticSymbolKind {
    Function,
    Method,
    Parameter,
    Variable,
    Property,
    Type,
    Namespace,
    Class,
}

/// A resolved symbol occurrence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticSymbol {
    pub key: SemanticSymbolKey,
    pub name: Name,
    pub kind: SemanticSymbolKind,
    pub definition: Option<Definition>,
    pub declaration: Option<SymbolDeclaration>,
    pub ty: Option<Ty>,
}

impl SemanticSymbol {
    /// Determine where references to this occurrence can live.
    ///
    /// Delegates to [`Definition::reference_scope`] when the symbol carries one,
    /// otherwise projects the `SemanticSymbolKey` shape: body-scoped locals are
    /// `FileLocal`, typed-member occurrences map to `Unknown` (they are field
    /// accesses on a typed receiver, not standalone name bindings).
    pub fn reference_scope(&self, db: &dyn DefDatabase) -> ReferenceScope {
        if let Some(def) = self.definition.as_ref() {
            return def.reference_scope(db);
        }
        match &self.key {
            SemanticSymbolKey::BodyLocal { .. } | SemanticSymbolKey::ImplicitLocal { .. } => {
                ReferenceScope::FileLocal
            }
            SemanticSymbolKey::TypedMember { .. } | SemanticSymbolKey::Definition(_) => {
                ReferenceScope::Unknown
            }
        }
    }
}

/// Source-backed declaration target for a symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDeclaration {
    pub file_id: FileId,
    pub range: TextRange,
    pub name: Name,
    pub kind: SemanticSymbolKind,
}

impl<'db, DB: HirDatabase + base_db::RootQueryDb> Semantics<'db, DB> {
    /// Resolve the symbol at `offset` in `file_id`.
    pub fn symbol_at(&self, file_id: FileId, offset: TextSize) -> Option<SemanticSymbol> {
        let parse = self.db.parse(file_id);
        let root = parse.syntax_node();
        let token = root.token_at_offset(offset).right_biased()?;
        self.symbol_for_token(file_id, &token)
    }

    /// Resolve a syntax token to a type-aware semantic symbol.
    pub fn symbol_for_token(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        match crate::classify_token(token) {
            NameClass::FreeName { token } => self.symbol_for_free_name(file_id, &token),
            NameClass::FieldName { receiver, token, is_call } => {
                self.symbol_for_field_name(file_id, &receiver, &token, is_call)
            }
            NameClass::TypeRef { token } => self.symbol_for_type_ref(file_id, &token),
            NameClass::Literal { .. } | NameClass::Keyword { .. } | NameClass::Other => None,
        }
    }

    fn symbol_for_free_name(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        if let Some(definition) = self.try_resolve_builtin(token.text()) {
            return Some(symbol_from_definition(self.db, definition, None));
        }

        if let Some(symbol) = self.symbol_for_body_local(file_id, token) {
            return Some(symbol);
        }

        let definition = self.resolve_name_to_definition(file_id, token)?;
        Some(symbol_from_definition(self.db, definition, None))
    }

    fn symbol_for_field_name(
        &self,
        file_id: FileId,
        receiver: &syntax::SyntaxNode,
        token: &syntax::SyntaxToken,
        is_call: bool,
    ) -> Option<SemanticSymbol> {
        let method = || {
            self.resolve_method_call_to_definition(file_id, token)
                .map(|definition| symbol_from_definition(self.db, definition, None))
        };
        let property = || self.symbol_for_typed_property(file_id, receiver, token);

        if is_call {
            method().or_else(property).or_else(|| {
                self.resolve_name_to_definition(file_id, token)
                    .map(|definition| symbol_from_definition(self.db, definition, None))
            })
        } else {
            property().or_else(method).or_else(|| {
                self.resolve_name_to_definition(file_id, token)
                    .map(|definition| symbol_from_definition(self.db, definition, None))
            })
        }
    }

    fn symbol_for_typed_property(
        &self,
        file_id: FileId,
        receiver: &syntax::SyntaxNode,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        let receiver_ty = self.type_of_expr(file_id, receiver);
        if receiver_ty.is_unknown() {
            return None;
        }

        let configs = self.db.configurations(file_id);
        let name = Name::new(token.text());
        let receiver_id = hir_ty::ty_bridge::ty_to_typeid(self.db, &receiver_ty);
        let field = hir_ty::lookup_field(self.db, &configs, receiver_id, &name)?;
        Some(SemanticSymbol {
            key: SemanticSymbolKey::TypedMember { file_id, range: token.text_range() },
            name: field.name,
            kind: SemanticSymbolKind::Property,
            definition: None,
            declaration: None,
            ty: Some(field.ty),
        })
    }

    fn symbol_for_type_ref(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        let definition = self.resolve_name_to_definition(file_id, token);
        definition.map(|definition| symbol_from_definition(self.db, definition, None))
    }

    fn symbol_for_body_local(
        &self,
        file_id: FileId,
        token: &syntax::SyntaxToken,
    ) -> Option<SemanticSymbol> {
        let (owner, body, source_map) = self.body_for_range(file_id, token.text_range())?;
        let name = Name::new(token.text());
        let name_lower = name.as_str().to_lowercase();

        if let Some(binding_id) = body
            .bindings_iter()
            .find(|(_, binding)| binding.name.eq_ignore_case(&name))
            .map(|(id, _)| id)
        {
            let binding = body.binding(binding_id);
            let range = source_map.binding_range(binding_id)?;
            let is_param = body.params().any(|param_id| param_id == binding_id);
            let kind =
                if is_param { SemanticSymbolKind::Parameter } else { SemanticSymbolKind::Variable };
            return Some(SemanticSymbol {
                key: SemanticSymbolKey::BodyLocal { file_id, owner, name_lower },
                name: binding.name.clone(),
                kind,
                definition: None,
                declaration: Some(SymbolDeclaration {
                    file_id,
                    range,
                    name: binding.name.clone(),
                    kind,
                }),
                ty: self.type_of_binding_at(file_id, range),
            });
        }

        let occurrence_expr = source_map.expr_at_range(token.text_range())?;
        // Phase O.17: route through the per-owner Salsa cell — owner
        // is already in hand from `body_for_range`, so we hit exactly
        // one query (the matching method or module-code) instead of
        // the file-wide `infer_query` aggregate.
        let routed = crate::infer_owner(self.db, file_id, owner);
        let implicit = routed.implicit_locals().get(&name_lower)?;
        // Phase 3 §4.D: accessor bridges stored `TypeId` → owned `Ty`.
        let occurrence_ty = routed.type_of_expr(self.db, occurrence_expr).unwrap_or(Ty::Unknown);
        let (declaration, range, ty) = select_implicit_local_declaration(
            &source_map,
            implicit,
            token.text_range(),
            occurrence_ty,
        )?;
        Some(SemanticSymbol {
            key: SemanticSymbolKey::ImplicitLocal { file_id, owner, name_lower, declaration },
            name: implicit.name.clone(),
            kind: SemanticSymbolKind::Variable,
            definition: None,
            declaration: Some(SymbolDeclaration {
                file_id,
                range,
                name: implicit.name.clone(),
                kind: SemanticSymbolKind::Variable,
            }),
            ty: Some(ty),
        })
    }

    fn body_for_range(
        &self,
        file_id: FileId,
        range: TextRange,
    ) -> Option<(DefWithBodyId, hir_def::Body, hir_def::BodySourceMap)> {
        let module_bodies = self.db.module_bodies(ModuleId::new(file_id));

        if let Some(result) = module_bodies.module_code_result() {
            if result.source_map.expr_at_range(range).is_some()
                || result.source_map.binding_at_range(range).is_some()
            {
                return Some((
                    DefWithBodyId::ModuleCode,
                    result.body.clone(),
                    result.source_map.clone(),
                ));
            }
        }

        for (local_id, body, source_map) in module_bodies.method_bodies() {
            if source_map.expr_at_range(range).is_some()
                || source_map.binding_at_range(range).is_some()
            {
                return Some((DefWithBodyId::Method(local_id), body.clone(), source_map.clone()));
            }
        }

        None
    }
}

fn select_implicit_local_declaration(
    source_map: &hir_def::BodySourceMap,
    implicit: &ImplicitLocalInfo,
    occurrence_range: TextRange,
    occurrence_ty: Ty,
) -> Option<(ExprId, TextRange, Ty)> {
    if !occurrence_ty.is_unknown() {
        let typed_preceding = implicit
            .assignments
            .iter()
            .filter_map(|assignment| {
                if assignment.ty != occurrence_ty {
                    return None;
                }
                let range = source_map.expr_range(assignment.target)?;
                (range.start() <= occurrence_range.start()).then_some((assignment, range))
            })
            .next_back();

        if let Some((assignment, range)) = typed_preceding {
            return Some((assignment.target, range, assignment.ty.clone()));
        }
    }

    let preceding = implicit
        .assignments
        .iter()
        .filter_map(|assignment| {
            let range = source_map.expr_range(assignment.target)?;
            (range.start() <= occurrence_range.start()).then_some((assignment, range))
        })
        .next_back();

    if let Some((assignment, range)) = preceding {
        return Some((assignment.target, range, assignment.ty.clone()));
    }

    let range = source_map.expr_range(implicit.first_assignment)?;
    Some((implicit.first_assignment, range, implicit.ty.clone()))
}

fn symbol_from_definition(
    db: &dyn hir_def::DefDatabase,
    definition: Definition,
    ty: Option<Ty>,
) -> SemanticSymbol {
    let name = definition.name(db).unwrap_or_else(Name::missing);
    let kind = kind_for_definition(&definition);
    let declaration = declaration_for_definition(db, &definition, kind);
    SemanticSymbol {
        key: SemanticSymbolKey::Definition(definition.clone()),
        name,
        kind,
        definition: Some(definition),
        declaration,
        ty,
    }
}

fn kind_for_definition(definition: &Definition) -> SemanticSymbolKind {
    match definition {
        Definition::Method(_) | Definition::BuiltinFunction(_) => SemanticSymbolKind::Function,
        Definition::BuiltinMethodHandle { .. } => SemanticSymbolKind::Method,
        Definition::Variable(_) | Definition::Local { .. } => SemanticSymbolKind::Variable,
        Definition::Parameter { .. } => SemanticSymbolKind::Parameter,
        Definition::VirtualTableField { .. } => SemanticSymbolKind::Property,
        Definition::MdoCollectionType(_) => SemanticSymbolKind::Class,
        Definition::MdoObject { .. } => SemanticSymbolKind::Type,
        Definition::MdoManagerModule { .. } | Definition::Module(_) => {
            SemanticSymbolKind::Namespace
        }
        Definition::Unresolved => SemanticSymbolKind::Variable,
    }
}

fn declaration_for_definition(
    db: &dyn hir_def::DefDatabase,
    definition: &Definition,
    kind: SemanticSymbolKind,
) -> Option<SymbolDeclaration> {
    match definition {
        Definition::Method(method_id) => declaration_for_method(db, *method_id, kind),
        Definition::Variable(var_id) => Some(SymbolDeclaration {
            file_id: var_id.module.file_id,
            range: definition.source_range(db)?,
            name: definition.name(db)?,
            kind,
        }),
        _ => None,
    }
}

fn declaration_for_method(
    db: &dyn hir_def::DefDatabase,
    method_id: MethodId,
    kind: SemanticSymbolKind,
) -> Option<SymbolDeclaration> {
    Some(SymbolDeclaration {
        file_id: method_id.module.file_id,
        range: definition_source_range(db, method_id)?,
        name: Definition::Method(method_id).name(db)?,
        kind,
    })
}

fn definition_source_range(
    db: &dyn hir_def::DefDatabase,
    method_id: MethodId,
) -> Option<TextRange> {
    Definition::Method(method_id).source_range(db)
}
