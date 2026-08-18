//! Orchestrator for the MCP `symbol_info` tool: interpret a qualified BSL name (or a
//! `path`+`line`+`column` position for locals) against the resident analysis database and
//! assemble a consolidated semantic card — kind, signature, type, doc, and definition.
//!
//! All BSL semantics live here (dotted-name interpretation, resident name resolution, card
//! assembly). The MCP adapter is a thin projection that adds graph-derived usages/candidates,
//! applies the output budget, and serializes. Resolution is resident-led: the card is servable
//! whenever the resident host is `Ready`, independent of the call graph.

pub(crate) mod form;

use bsl_metadata::MdoType;
use hir::{
    compute_execution_context, kernel_type_label, method_return_type, DefDatabase, Definition,
    ExecutionContext, ManagerType, MethodId, ModuleId, Name, Semantics,
};
use ide_db::base_db::{Locale, RootQueryDb, SourceDatabase, SourceRootId};
use ide_db::RootDatabaseImpl;
use line_index::{LineColRange, LineIndex};
use symbol_info::{build_signature, render_declaration, CalleeKind, MethodKind, SymbolSignature};
use syntax::TextRange;
use vfs::FileId;

/// The whole workspace is loaded into a single local source root; the resident host mirrors
/// the graph's `GRAPH_SOURCE_ROOT`, so the module index is keyed here.
const CONFIG_SOURCE_ROOT: SourceRootId = SourceRootId(0);

/// A position in a resident file, used by the local/positional resolution path.
#[derive(Debug, Clone, Copy)]
pub struct SymbolPosition {
    pub file_id: FileId,
    /// 0-based line.
    pub line: u32,
    /// 0-based UTF-16-agnostic column (character offset within the line).
    pub column: u32,
}

/// Which card sections the caller wants. An all-false request means "all sections".
#[derive(Debug, Clone, Copy)]
pub struct SymbolInfoSections {
    pub definition: bool,
    pub type_: bool,
    pub doc: bool,
}

impl SymbolInfoSections {
    /// All sections enabled — the default when the caller passes no `include` filter.
    pub fn all() -> Self {
        Self { definition: true, type_: true, doc: true }
    }
}

/// A request for one symbol's card: either a qualified `symbol` name (primary) or a
/// `position` (fallback, for locals/parameters). Exactly one is expected; `symbol` wins if
/// both are set.
#[derive(Debug, Clone)]
pub struct SymbolInfoRequest {
    pub symbol: Option<String>,
    pub position: Option<SymbolPosition>,
    pub locale: Locale,
    pub sections: SymbolInfoSections,
    /// The root the call graph was built against (the resident's `workspace_root` / the MCP
    /// server's `source_dir`), used to encode a form event-handler's path-fallback
    /// `method/file/<rel>::<name>` graph id byte-identically to the graph builder. This is NOT the
    /// config root (`db.all_config_paths()` — e.g. `<workspace>/src/cf`): the graph strips the
    /// workspace root, so using the config root would mint a mismatched, non-resolving id and drop
    /// form-handler usages. `None` disables the form-handler `graph_id` (usages) only.
    pub workspace_root: Option<std::path::PathBuf>,
}

/// The container a symbol lives in (its owning module or metadata object).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolContainer {
    /// The container kind label, e.g. `ОбщийМодуль`, `Справочник`, `Документ`.
    pub kind: String,
    /// The container's name, e.g. the common-module or object name.
    pub name: String,
    /// Where the container's code runs (server/client), when known.
    pub context: Option<String>,
}

/// One member of an aggregate card (a metadata object's attribute or tabular-section field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolMember {
    pub name: String,
    /// `Реквизит`, `ТабличнаяЧасть`, `Измерение`, `Ресурс`, …
    pub kind: String,
    pub ty: Option<String>,
}

/// The definition site of a symbol whose source is a BSL file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolDefinition {
    /// Workspace path of the defining file, when it maps to a source root.
    pub path: Option<String>,
    /// 1-based line of the declaration.
    pub line: u32,
    /// The declaration line (or the local's line) as a source snippet.
    pub snippet: Option<String>,
    /// The declared name alone, 0-based with UTF-16 columns. Absent where the name has
    /// no range of its own — a local, a module card anchored at the file start.
    pub name_range: Option<LineColRange>,
    /// The whole declaration, same units. Absent for the same reason.
    pub enclosing_range: Option<LineColRange>,
}

/// The two byte ranges a definition may know about, kept as one value so the two
/// `Option<TextRange>` cannot be passed in the wrong order.
#[derive(Debug, Clone, Copy, Default)]
struct DefinitionRanges {
    /// The declared name.
    name: Option<TextRange>,
    /// The whole declaration node.
    enclosing: Option<TextRange>,
}

/// The consolidated semantic card for one symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolInfoCard {
    /// The canonical symbol string this card describes.
    pub symbol: String,
    /// A stable kind label, e.g. `function`, `procedure`, `metadata object`, `attribute`,
    /// `local variable`, `parameter`, `platform function`, `common module`.
    pub kind: &'static str,
    pub container: Option<SymbolContainer>,
    /// A one-line signature (methods) or structure summary (aggregate objects).
    pub signature: Option<String>,
    pub doc: Option<String>,
    /// Declared/inferred return type (methods) or the attribute's type (attribute cards).
    pub return_type: Option<String>,
    pub definition: Option<SymbolDefinition>,
    /// Aggregate members for a whole-object card (empty otherwise).
    pub members: Vec<SymbolMember>,
    /// The durable call-graph id for this symbol, when it is a method that the graph can address.
    /// The adapter uses it to attach the `usages` summary directly (no fuzzy re-resolution) and
    /// echoes it back so an agent can walk the full caller list via `graph`.
    pub graph_id: Option<String>,
    /// Set when the graph knew the symbol but the resident host no longer has it (a stale
    /// candidate). The card is otherwise empty and the caller should treat it as best-effort.
    pub semantics_unavailable: bool,
}

impl SymbolInfoCard {
    fn empty(symbol: String, kind: &'static str) -> Self {
        Self {
            symbol,
            kind,
            container: None,
            signature: None,
            doc: None,
            return_type: None,
            definition: None,
            members: Vec::new(),
            graph_id: None,
            semantics_unavailable: false,
        }
    }
}

/// Resolve a symbol on the resident database and assemble its card. Returns `None` when the
/// resident cannot resolve the request — the adapter then offers graph-derived candidates
/// (imprecise-name path) rather than a hard error.
pub fn symbol_info(db: &RootDatabaseImpl, req: &SymbolInfoRequest) -> Option<SymbolInfoCard> {
    if let Some(symbol) = req.symbol.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return resolve_qualified(db, symbol, req);
    }
    if let Some(pos) = req.position {
        return resolve_position(db, pos, req);
    }
    None
}

// --- qualified-name resolution ----------------------------------------------------------

fn resolve_qualified(
    db: &RootDatabaseImpl,
    symbol: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let segments: Vec<&str> = symbol.split('.').map(str::trim).filter(|s| !s.is_empty()).collect();
    if let Some(card) = form::resolve_form(db, symbol, &segments, req) {
        return Some(card);
    }
    match segments.as_slice() {
        [one] => resolve_single(db, symbol, one, req),
        [a, b] => resolve_pair(db, symbol, a, b, req),
        [a, b, c] => resolve_triple(db, symbol, a, b, c, req),
        _ => None,
    }
}

/// `<Name>`: a common module (whole-module card) or a platform global function.
fn resolve_single(
    db: &RootDatabaseImpl,
    symbol: &str,
    name: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let module_index = db.module_index(CONFIG_SOURCE_ROOT);
    if let Some(file_id) = module_index.resolve_common_module(&Name::new(name)) {
        let display =
            module_index.canonical_common_module_name(&Name::new(name)).unwrap_or(name).to_string();
        let mut card = SymbolInfoCard::empty(symbol.to_string(), "common module");
        card.container = Some(SymbolContainer {
            kind: "ОбщийМодуль".to_string(),
            name: display,
            context: module_context(db, ModuleId::new(file_id)),
        });
        // A module card is anchored at the file start: the module has no declaration
        // node of its own, so there is no range to publish.
        card.definition = def_from_file_line(
            db,
            file_id,
            syntax::TextSize::from(0u32),
            DefinitionRanges::default(),
            req.sections.definition,
        );
        return Some(card);
    }

    // A bare platform global function (`СтрНайти`, `ТекущаяДата`, …).
    let callee = CalleeKind::GlobalFunction { name: name.into() };
    let sigs = build_signature(db, FileId(0), &callee)?;
    let sig = sigs.first()?;
    Some(card_from_platform_sig(symbol, sig, req))
}

/// `<A>.<B>`: a common-module method, a whole metadata object, or a platform type method.
fn resolve_pair(
    db: &RootDatabaseImpl,
    symbol: &str,
    a: &str,
    b: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let module_index = db.module_index(CONFIG_SOURCE_ROOT);

    // Common module method.
    if let Some(module_file) = module_index.resolve_common_module(&Name::new(a)) {
        let display =
            module_index.canonical_common_module_name(&Name::new(a)).unwrap_or(a).to_string();
        let callee =
            CalleeKind::CommonModuleMethod { module: Name::new(&display), method: Name::new(b) };
        if let Some(sigs) = build_signature(db, module_file, &callee) {
            if let Some(sig) = sigs.first() {
                let container = SymbolContainer {
                    kind: "ОбщийМодуль".to_string(),
                    name: display,
                    context: module_context(db, ModuleId::new(module_file)),
                };
                return Some(card_from_method_sig(db, symbol, sig, Some(container), req));
            }
        }
    }

    // Whole metadata object (`Справочник.Товары`).
    if let Some(mdo_type) = parse_mdo_type(a) {
        if let Some(card) = object_card(db, symbol, mdo_type, b, req) {
            return Some(card);
        }
    }

    // A platform type method (`Массив.Добавить`) — brief card; full reference lives in
    // `syntax_help`.
    let callee = CalleeKind::PlatformMethod { type_name: a.into(), method_name: b.into() };
    if let Some(sigs) = build_signature(db, FileId(0), &callee) {
        if let Some(sig) = sigs.first() {
            return Some(card_from_platform_sig(symbol, sig, req));
        }
    }

    None
}

/// `<MdoType>.<Object>.<Member>`: a metadata attribute, or an object/manager module method.
fn resolve_triple(
    db: &RootDatabaseImpl,
    symbol: &str,
    a: &str,
    b: &str,
    c: &str,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let mdo_type = parse_mdo_type(a)?;

    // Attribute / tabular field on the metadata object (type + ownership).
    if let Some(card) = attribute_card(db, symbol, mdo_type, b, c) {
        return Some(card);
    }

    let module_index = db.module_index(CONFIG_SOURCE_ROOT);
    let object = Name::new(b);
    let method = Name::new(c);

    // Object / record-set module method → routed through LocalMethod on the def module.
    for module_file in [
        module_index.resolve_object_module(mdo_type, &object),
        module_index.resolve_record_set_module(mdo_type, &object),
    ]
    .into_iter()
    .flatten()
    {
        let module_id = ModuleId::new(module_file);
        let callee = CalleeKind::LocalMethod { module_id, method: method.clone() };
        if let Some(sigs) = build_signature(db, module_file, &callee) {
            if let Some(sig) = sigs.first() {
                if !sig.is_export {
                    return None;
                }
                let container = SymbolContainer {
                    kind: mdo_type.russian_name().to_string(),
                    name: b.to_string(),
                    context: None,
                };
                return Some(card_from_method_sig(db, symbol, sig, Some(container), req));
            }
        }
    }

    // Manager module method (`Справочники.Товары.СоздатьЭлемент` style overrides).
    let callee = CalleeKind::ManagerModuleMethod {
        mdo_type,
        object: object.clone(),
        method: method.clone(),
    };
    if let Some(module_file) =
        module_index.resolve_manager(ManagerType::from_mdo_type(mdo_type)?, &object)
    {
        if let Some(sigs) = build_signature(db, module_file, &callee) {
            if let Some(sig) = sigs.first() {
                let container = SymbolContainer {
                    kind: format!("Менеджер{}", mdo_type.russian_name()),
                    name: b.to_string(),
                    context: None,
                };
                return Some(card_from_method_sig(db, symbol, sig, Some(container), req));
            }
        }
    }

    None
}

// --- metadata object / attribute cards --------------------------------------------------

fn object_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    mdo_type: MdoType,
    name: &str,
    _req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    // Registers live in a separate substrate (`Register`, not `MetadataObject`) with
    // dimensions/resources/attributes rather than plain attributes + tabular sections.
    if mdo_type.is_register() {
        return register_card(db, symbol, mdo_type, name);
    }
    let obj = db.resolve_metadata_object_across_roots(mdo_type, name)?;
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "metadata object");
    card.container = Some(SymbolContainer {
        kind: mdo_type.russian_name().to_string(),
        name: obj.name.clone(),
        context: None,
    });
    let mut members = Vec::new();
    for attr in &obj.attributes {
        members.push(SymbolMember {
            name: attr.name.clone(),
            kind: "Реквизит".to_string(),
            ty: Some(attr.attr_type.to_string()),
        });
    }
    for ts in &obj.tabular_sections {
        members.push(SymbolMember {
            name: ts.name().to_string(),
            kind: "ТабличнаяЧасть".to_string(),
            ty: None,
        });
    }
    card.signature = Some(format!(
        "{}.{} — {} реквизит(ов), {} табличн. част(и)",
        mdo_type.russian_name(),
        obj.name,
        obj.attributes.len(),
        obj.tabular_sections.len()
    ));
    card.members = members;
    Some(card)
}

/// A whole-register card: dimensions, resources, and attributes as members.
fn register_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    mdo_type: MdoType,
    name: &str,
) -> Option<SymbolInfoCard> {
    let reg = db.resolve_register_across_roots(mdo_type, name)?;
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "metadata object");
    card.container = Some(SymbolContainer {
        kind: mdo_type.russian_name().to_string(),
        name: reg.name().to_string(),
        context: None,
    });
    let mut members = Vec::new();
    for d in reg.dimensions() {
        members.push(SymbolMember {
            name: d.name().to_string(),
            kind: "Измерение".to_string(),
            ty: register_member_type(d.attr_type(), d.type_str()),
        });
    }
    for r in reg.resources() {
        members.push(SymbolMember {
            name: r.name().to_string(),
            kind: "Ресурс".to_string(),
            ty: register_member_type(r.attr_type(), r.type_str()),
        });
    }
    for a in reg.attributes() {
        members.push(SymbolMember {
            name: a.name().to_string(),
            kind: "Реквизит".to_string(),
            ty: register_member_type(a.attr_type(), a.type_str()),
        });
    }
    card.signature = Some(format!(
        "{}.{} — {} измерен., {} ресурс., {} реквизит.",
        mdo_type.russian_name(),
        reg.name(),
        reg.dimensions().len(),
        reg.resources().len(),
        reg.attributes().len()
    ));
    card.members = members;
    Some(card)
}

/// A single register member (dimension/resource/attribute): its type + owning register.
fn register_member_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    mdo_type: MdoType,
    object: &str,
    member: &str,
) -> Option<SymbolInfoCard> {
    let reg = db.resolve_register_across_roots(mdo_type, object)?;
    let found = reg
        .dimensions()
        .iter()
        .find(|d| name_eq(d.name(), member))
        .map(|d| ("Измерение", register_member_type(d.attr_type(), d.type_str())))
        .or_else(|| {
            reg.resources()
                .iter()
                .find(|r| name_eq(r.name(), member))
                .map(|r| ("Ресурс", register_member_type(r.attr_type(), r.type_str())))
        })
        .or_else(|| {
            reg.attributes()
                .iter()
                .find(|a| name_eq(a.name(), member))
                .map(|a| ("Реквизит", register_member_type(a.attr_type(), a.type_str())))
        })?;
    let mut card = SymbolInfoCard::empty(symbol.to_string(), "attribute");
    card.return_type = found.1;
    card.container = Some(SymbolContainer {
        kind: format!("{} ({})", mdo_type.russian_name(), found.0),
        name: reg.name().to_string(),
        context: None,
    });
    Some(card)
}

/// The displayable type of a register member: the structured `ОписаниеТипов` when parsed,
/// otherwise the raw type string, dropping the empty case.
fn register_member_type(
    attr_type: Option<&bsl_metadata::AttributeType>,
    type_str: &str,
) -> Option<String> {
    if let Some(t) = attr_type {
        return Some(t.to_string());
    }
    (!type_str.is_empty()).then(|| type_str.to_string())
}

fn attribute_card(
    db: &RootDatabaseImpl,
    symbol: &str,
    mdo_type: MdoType,
    object: &str,
    member: &str,
) -> Option<SymbolInfoCard> {
    if mdo_type.is_register() {
        return register_member_card(db, symbol, mdo_type, object, member);
    }
    let obj = db.resolve_metadata_object_across_roots(mdo_type, object)?;

    if let Some(attr) = obj.attributes.iter().find(|a| name_eq(&a.name, member)) {
        let mut card = SymbolInfoCard::empty(symbol.to_string(), "attribute");
        card.return_type = Some(attr.attr_type.to_string());
        card.container = Some(SymbolContainer {
            kind: mdo_type.russian_name().to_string(),
            name: obj.name.clone(),
            context: None,
        });
        return Some(card);
    }

    // A tabular-section name, or a `TabularSection.Field` under this object.
    for ts in &obj.tabular_sections {
        if name_eq(ts.name(), member) {
            let mut card = SymbolInfoCard::empty(symbol.to_string(), "tabular section");
            card.container = Some(SymbolContainer {
                kind: mdo_type.russian_name().to_string(),
                name: obj.name.clone(),
                context: None,
            });
            card.members = ts
                .attributes()
                .iter()
                .map(|a| SymbolMember {
                    name: a.name().to_string(),
                    kind: "Реквизит".to_string(),
                    ty: Some(a.attr_type().to_string()),
                })
                .collect();
            return Some(card);
        }
    }

    None
}

// --- positional (local / parameter) resolution ------------------------------------------

fn resolve_position(
    db: &RootDatabaseImpl,
    pos: SymbolPosition,
    req: &SymbolInfoRequest,
) -> Option<SymbolInfoCard> {
    let offset = crate::name_lookup::offset_for_line_col(db, pos.file_id, pos.line, pos.column)?;
    let parse = db.parse(pos.file_id);
    let root = parse.syntax_node();
    let token = root.token_at_offset(offset).right_biased()?;

    let sema = Semantics::new(db);
    let def = sema.resolve_name_to_definition(pos.file_id, &token)?;

    let name = def.name(db).map(|n| n.as_str().to_string()).unwrap_or_else(|| token.text().into());
    let kind = kind_for_definition(&def);
    let mut card = SymbolInfoCard::empty(name.clone(), kind);
    card.symbol = name;
    card.signature = Some(def.label(db));

    if req.sections.doc {
        card.doc = def.docs(db).and_then(|d| d.purpose.clone());
    }

    if req.sections.type_ {
        if let Definition::Method(method_id) = &def {
            card.return_type = method_return_type_label(db, *method_id, req.locale);
        }
    }

    if req.sections.definition {
        if let (Some(module), Some(range)) = (def.module(db), def.source_range(db)) {
            card.definition = def_from_file_line(
                db,
                module.file_id,
                range.start(),
                DefinitionRanges { name: def.name_range(db), enclosing: Some(range) },
                true,
            );
        } else {
            // Locals and parameters have no method-level source range, but their POSITION is
            // known — it is how the caller reached them. Publishing a location without ranges
            // here would mean "somewhere in this file" while the legacy `line` names the line.
            card.definition = def_from_file_line(
                db,
                pos.file_id,
                offset,
                DefinitionRanges { name: Some(token.text_range()), enclosing: None },
                true,
            );
        }
    }

    Some(card)
}

fn kind_for_definition(def: &Definition) -> &'static str {
    match def {
        Definition::Method(_) => "method",
        Definition::Variable(_) => "module variable",
        Definition::Parameter { .. } => "parameter",
        Definition::Local { .. } => "local variable",
        Definition::BuiltinFunction(_) | Definition::BuiltinMethodHandle { .. } => {
            "platform function"
        }
        Definition::Module(_) => "module",
        Definition::MdoObject { .. } | Definition::MdoCollectionType(_) => "metadata object",
        Definition::MdoManagerModule { .. } => "manager module",
        Definition::VirtualTableField { .. } => "field",
        Definition::Unresolved => "unresolved",
    }
}

// --- shared card assembly ---------------------------------------------------------------

fn card_from_method_sig(
    db: &RootDatabaseImpl,
    symbol: &str,
    sig: &SymbolSignature,
    container: Option<SymbolContainer>,
    req: &SymbolInfoRequest,
) -> SymbolInfoCard {
    let kind = match sig.kind {
        MethodKind::Function => "function",
        MethodKind::Procedure => "procedure",
    };
    let mut card = SymbolInfoCard::empty(symbol.to_string(), kind);
    card.container = container;
    card.signature = Some(signature_string(sig));
    card.graph_id = method_graph_id(db, sig);

    if req.sections.doc {
        card.doc = sig.purpose.clone().or_else(|| sig.description.clone());
    }
    if req.sections.type_ {
        card.return_type = return_type_label(db, sig, req.locale);
    }
    if req.sections.definition {
        if let Some(method_id) = sig.method_id {
            card.definition = def_for_method(db, method_id);
        }
    }
    card
}

/// The durable call-graph id for a resolved user method, encoded with the SAME path-derived
/// grammar the graph builder uses (`method/<scope>/<name>`), so the adapter can read its
/// usages by id instead of round-tripping the qualified name through the graph's fuzzy
/// resolver (which does not match a dotted `Module.Method` against a `method/.../...` id).
/// `None` when the method has no addressable module (platform members, file-layout modules).
fn method_graph_id(db: &RootDatabaseImpl, sig: &SymbolSignature) -> Option<String> {
    let method_id = sig.method_id?;
    let path = file_path(db, method_id.module.file_id)?;
    crate::graph::method_id_for_path(&path, &sig.name_russian)
}

fn card_from_platform_sig(
    symbol: &str,
    sig: &SymbolSignature,
    req: &SymbolInfoRequest,
) -> SymbolInfoCard {
    let kind = match sig.kind {
        MethodKind::Function => "platform function",
        MethodKind::Procedure => "platform procedure",
    };
    let mut card = SymbolInfoCard::empty(symbol.to_string(), kind);
    card.signature = Some(signature_string(sig));
    if req.sections.doc {
        // Point at `syntax_help` for the full reference rather than duplicating it here.
        card.doc = sig
            .purpose
            .clone()
            .or_else(|| sig.description.clone())
            .map(|d| format!("{d}\n\nПолная справка → syntax_help"))
            .or_else(|| Some("Полная справка → syntax_help".to_string()));
    }
    if req.sections.type_ && !sig.returns.is_empty() {
        card.return_type = Some(join_type_refs(&sig.returns));
    }
    card
}

fn signature_string(sig: &SymbolSignature) -> String {
    // The real BSL declaration (`Функция Имя(П) Экспорт`) — the return type is surfaced in the
    // dedicated `return_type` field, not appended here as the signature-help view would.
    render_declaration(sig)
}

fn return_type_label(
    db: &RootDatabaseImpl,
    sig: &SymbolSignature,
    locale: Locale,
) -> Option<String> {
    if !sig.returns.is_empty() {
        return Some(join_type_refs(&sig.returns));
    }
    let method_id = sig.method_id?;
    method_return_type_label(db, method_id, locale)
}

fn method_return_type_label(
    db: &RootDatabaseImpl,
    method_id: MethodId,
    locale: Locale,
) -> Option<String> {
    let ty = method_return_type(db, method_id);
    type_label(db, ty, locale)
}

fn type_label(db: &RootDatabaseImpl, ty: hir::TypeId, locale: Locale) -> Option<String> {
    let label = kernel_type_label(db, ty, locale, false);
    (!label.is_empty()).then_some(label)
}

fn join_type_refs(returns: &[symbol_info::TypeRef]) -> String {
    returns.iter().map(|t| t.russian.as_str()).collect::<Vec<_>>().join(" | ")
}

// --- helpers ----------------------------------------------------------------------------

/// Interpret a metadata-object-type keyword, accepting both singular (`Справочник`) and
/// plural (`Справочники`) Russian/English forms without regex.
fn parse_mdo_type(s: &str) -> Option<MdoType> {
    s.parse::<MdoType>().ok().or_else(|| MdoType::from_plural(s))
}

/// Unicode-aware case-insensitive name match (BSL identifiers are case-insensitive and
/// bilingual identifiers include Cyrillic, which ASCII folding would miss).
fn name_eq(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn module_context(db: &RootDatabaseImpl, module_id: ModuleId) -> Option<String> {
    let meta = db.module_metadata(module_id);
    let ctx = meta
        .execution_context
        .or_else(|| meta.common_module.as_ref().map(|cm| compute_execution_context(cm)))?;
    Some(execution_context_label(ctx).to_string())
}

fn execution_context_label(ctx: ExecutionContext) -> &'static str {
    match ctx {
        ExecutionContext::Server => "Сервер",
        ExecutionContext::ServerCall => "Сервер (вызов сервера)",
        ExecutionContext::Client => "Клиент",
        ExecutionContext::ClientServer => "Клиент и сервер",
        ExecutionContext::ExternalConnection => "Внешнее соединение",
        ExecutionContext::Unknown => "Не определён",
    }
}

fn def_for_method(db: &RootDatabaseImpl, method_id: MethodId) -> Option<SymbolDefinition> {
    let def = Definition::Method(method_id);
    let enclosing = def.source_range(db)?;
    def_from_file_line(
        db,
        method_id.module.file_id,
        enclosing.start(),
        DefinitionRanges { name: def.name_range(db), enclosing: Some(enclosing) },
        true,
    )
}

fn def_from_file_line(
    db: &RootDatabaseImpl,
    file_id: FileId,
    offset: syntax::TextSize,
    ranges: DefinitionRanges,
    include: bool,
) -> Option<SymbolDefinition> {
    if !include {
        return None;
    }
    let text = db.file_text(file_id);
    let line_index = LineIndex::new(&text);
    let line_col = line_index.line_col(offset);
    let snippet = line_text(&text, line_col.line).map(str::to_string);
    let to_line_col = |range: Option<TextRange>| {
        range.and_then(|range| line_index.utf16_line_col_range(&text, range))
    };
    Some(SymbolDefinition {
        path: file_path(db, file_id),
        line: line_col.line + 1,
        snippet,
        name_range: to_line_col(ranges.name),
        enclosing_range: to_line_col(ranges.enclosing),
    })
}

fn line_text(text: &str, line: u32) -> Option<&str> {
    text.lines().nth(line as usize).map(str::trim_end)
}

fn file_path(db: &RootDatabaseImpl, file_id: FileId) -> Option<String> {
    let source_root = db.source_root_input(CONFIG_SOURCE_ROOT).root(db);
    let vfs_path = source_root.file_set().path_for_file(&file_id)?;
    Some(vfs_path.as_path().to_str()?.replace('\\', "/"))
}
