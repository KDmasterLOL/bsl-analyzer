//! Resolve a qualified BSL name to the declarations it names — all of them.
//!
//! The multiplicity is in the return type on purpose. A name spelled the same
//! way in two roots (a configuration and an extension adopting it) has two
//! declarations, and an implementation that quietly picked one would answer
//! for a symbol the caller never asked about. The path-derived
//! [`hir::ModuleIndex`] cannot serve here for exactly that reason: it is
//! first-wins and visibility-blind by construction.
//!
//! Matching goes through [`ModuleKey`], derived from the module's path, not
//! through `ModuleMembers::module_name`: that field is `ObjectModule` for the
//! object module of every metadata object, so a three-segment name could not be
//! told apart by it.

use hir::{
    module_key_for_path, parse_form_module_path, DefDatabase, FormKey, ModuleId, ModuleKey, Name,
};
use ide_db::base_db::{Locale, SourceDatabase, SourceRootId, BSL_SOURCE_ROOT};
use ide_db::RootDatabaseImpl;
use stdx::case::CaseExt;
use syntax::TextRange;
use vfs::FileId;

use crate::name_lookup::NameCategory;
use crate::symbol_info::form::{is_common_form_keyword, is_form_marker};
use crate::symbol_info::{symbol_info, SymbolInfoRequest, SymbolInfoSections};

/// The source root holding `.bsl` — extensions are registered into it alongside
/// the base configuration, so one root covers every module.
const ROOT: SourceRootId = BSL_SOURCE_ROOT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Method,
    Variable,
}

/// One declaration site of a name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Declaration {
    pub file_id: FileId,
    /// The declared name alone.
    pub name_range: TextRange,
    /// The whole declaration.
    pub enclosing_range: Option<TextRange>,
    pub kind: DeclarationKind,
}

/// Every declaration a qualified name of one to three segments resolves to.
///
/// Empty means the name did not resolve as a declaration — either it is not
/// qualified at all (the name dictionary answers those), or it names something
/// that has no declaration to walk references from
/// (see [`classify_unreferenceable`]).
pub fn resolve_declarations(db: &RootDatabaseImpl, name: &str) -> Vec<Declaration> {
    let segments: Vec<&str> = name.split('.').map(str::trim).filter(|s| !s.is_empty()).collect();
    if let Some(form) = FormMember::parse(&segments) {
        // `symbol_info` reads a form member as an attribute, then as an item,
        // and only then as a handler. A form carrying both an attribute `Список`
        // and a procedure `Список()` would otherwise be an attribute to
        // the card and a method to the reference walk — the same divergence the
        // triple below refuses, on the route the triple does not cover.
        if matches!(classify_unreferenceable(db, name), Some(UnsupportedCategory::Form)) {
            return Vec::new();
        }
        return form.resolve(db);
    }
    let (owner, member) = match segments.as_slice() {
        [module, member] => (Owner::Common { name: module.fold_lower() }, *member),
        [mdo, object, member] => {
            let Some(mdo_type) = parse_mdo_type(mdo) else { return Vec::new() };
            // `symbol_info` reads a triple as an attribute BEFORE a module
            // method, and the two surfaces promise that one string means one
            // thing. A catalog with both an attribute `Код` and an exported
            // `Процедура Код()` would otherwise be an attribute to the card and
            // a method to the reference walk — the same name answering about
            // two different entities.
            if matches!(
                classify_unreferenceable(db, name),
                Some(UnsupportedCategory::MetadataMember)
            ) {
                return Vec::new();
            }
            (Owner::Object { mdo_type, name: object.fold_lower() }, *member)
        }
        _ => return Vec::new(),
    };

    let members = db.module_members(ROOT);
    let source_root = db.source_root_input(ROOT).root(db);
    let file_set = source_root.file_set();
    let member_name = Name::new(member);

    let mut out = Vec::new();
    for module in members.modules.values() {
        let Some(path) = file_set.path_for_file(&module.file_id) else { continue };
        let Some(key) = module_key_for_path(&path.as_path().to_string_lossy()) else { continue };
        if !owner.matches(&key) {
            continue;
        }

        for method in &module.methods {
            if method.name.eq_ignore_case(&member_name) {
                out.push(Declaration {
                    file_id: module.file_id,
                    name_range: method.name_range,
                    enclosing_range: Some(method.source_range),
                    kind: DeclarationKind::Method,
                });
            }
        }
        for variable in &module.variables {
            if variable.name.eq_ignore_case(&member_name) {
                out.push(Declaration {
                    file_id: module.file_id,
                    name_range: variable.name_range,
                    enclosing_range: Some(variable.source_range),
                    kind: DeclarationKind::Variable,
                });
            }
        }
    }

    out.sort_by_key(|decl| (decl.file_id.0, decl.name_range.start()));
    out
}

/// What a name turned out to be, when it turned out to be something no
/// reference walk enumerates.
///
/// ONE closed vocabulary for both routes to that answer. The two sources speak
/// different languages — `symbol_info` publishes prose kinds (`common module`),
/// the name dictionary publishes codes (`common_module`) — and letting either
/// leak through would make the published `category` depend on which stage
/// rejected the anchor rather than on what the entity is. The wire codes are the
/// dictionary's, so a consumer that already matches on [`NameCategory`] matches
/// on these too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedCategory {
    CommonModule,
    MetadataObject,
    /// An attribute, a tabular section, a field.
    MetadataMember,
    /// A form, or one of its attributes or items.
    Form,
    /// A platform type, method, property or global function.
    PlatformMember,
    /// The symbol was resolved, but nothing knows where its references could
    /// live — `ReferenceScope::Unknown`, or a declaration whose symbol the
    /// semantics could not derive. Not a kind of entity: a kind of ignorance,
    /// and it is named apart for exactly that reason.
    UnknownScope,
}

impl UnsupportedCategory {
    /// The whole vocabulary, for every place that PUBLISHES the list.
    pub const ALL: &'static [Self] = &[
        Self::CommonModule,
        Self::MetadataObject,
        Self::MetadataMember,
        Self::Form,
        Self::PlatformMember,
        Self::UnknownScope,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::CommonModule => "common_module",
            Self::MetadataObject => "metadata_object",
            Self::MetadataMember => "metadata_member",
            Self::Form => "form",
            Self::PlatformMember => "platform_member",
            Self::UnknownScope => "unknown_scope",
        }
    }

    /// From the name dictionary's category, for a candidate that matched exactly
    /// but carries no place to anchor on.
    ///
    /// `None` for a method or a module variable: those ARE referenceable, and
    /// calling one unsupported would deny a walk that exists. `None` too for an
    /// object or manager module as a whole: no spelling this surface accepts names
    /// one — a position inside it answers `unknown_scope` — so a category for it
    /// would be a published code nothing can produce.
    pub fn from_name_category(category: NameCategory) -> Option<Self> {
        Some(match category {
            NameCategory::CommonModule => Self::CommonModule,
            NameCategory::MetadataObject => Self::MetadataObject,
            NameCategory::MetadataMember => Self::MetadataMember,
            NameCategory::Form => Self::Form,
            NameCategory::PlatformMember => Self::PlatformMember,
            NameCategory::Module | NameCategory::ModuleMethod | NameCategory::ModuleVariable => {
                return None
            }
        })
    }
}

/// The name resolves, but to something no reference walk can enumerate — a
/// metadata object or one of its members, a platform member, a module as a
/// whole. The category is taken from what `symbol_info` makes of the same
/// string, so the two surfaces cannot drift apart on it.
///
/// `None` covers both "does not resolve at all" and "resolves to a method",
/// which are the caller's other two routes: the name dictionary, and a
/// declaration this module was expected to find.
pub fn classify_unreferenceable(db: &RootDatabaseImpl, name: &str) -> Option<UnsupportedCategory> {
    let req = SymbolInfoRequest {
        symbol: Some(name.to_string()),
        position: None,
        locale: Locale::Ru,
        // A category is all that is wanted; the card's sections are the
        // expensive part and none of them is read.
        sections: SymbolInfoSections { definition: false, type_: false, doc: false },
        workspace_root: None,
    };
    let card = symbol_info(db, &req)?;
    Some(match card.kind {
        "common module" => UnsupportedCategory::CommonModule,
        "metadata object" => UnsupportedCategory::MetadataObject,
        "attribute" | "tabular section" | "field" => UnsupportedCategory::MetadataMember,
        "platform function" | "platform procedure" => UnsupportedCategory::PlatformMember,
        "form" | "form attribute" | "form item" => UnsupportedCategory::Form,
        // A method is referenceable — saying otherwise here would hide a hole in
        // `resolve_declarations` behind an outcome that claims the symbol has no
        // references to walk.
        _ => return None,
    })
}

/// A method of a form module, addressed the way `symbol_info` addresses it:
/// `ОбщаяФорма.<Форма>.<Метод>` or `<Тип>.<Объект>.Форма.<Форма>.<Метод>`.
///
/// A form module is not in the path-derived module table — [`module_key_for_path`]
/// answers `None` for it — and its handlers are not exported, so the two sources
/// stage 1 otherwise uses know nothing about them. Without this the surface
/// answered `not_found` for a name `symbol_info` resolves into a card, which is
/// exactly the "found is not missing" promise this tool exists to keep.
struct FormMember<'a> {
    owner: Option<(bsl_metadata::MdoType, &'a str)>,
    form: &'a str,
    member: &'a str,
}

impl<'a> FormMember<'a> {
    fn parse(segments: &[&'a str]) -> Option<Self> {
        match segments {
            [common, form, member] if is_common_form_keyword(common) => {
                Some(Self { owner: None, form, member })
            }
            [mdo, object, marker, form, member] if is_form_marker(marker) => {
                Some(Self { owner: Some((parse_mdo_type(mdo)?, object)), form, member })
            }
            _ => None,
        }
    }

    /// Every form module the name addresses, walked over paths rather than
    /// through `ModuleIndex::resolve_form_module`: that table is keyed by
    /// (owner, form name) and first-wins, so a form a configuration extension
    /// adopts would answer with the base file alone and a rename taken from
    /// that answer would silently miss the second copy — the very thing the
    /// multiplicity of this return type exists to prevent.
    fn resolve(&self, db: &RootDatabaseImpl) -> Vec<Declaration> {
        let source_root = db.source_root_input(ROOT).root(db);
        let file_set = source_root.file_set();
        let member = Name::new(self.member);

        let mut out = Vec::new();
        for file_id in file_set.iter() {
            let Some(path) = file_set.path_for_file(&file_id) else { continue };
            let Some(key) = parse_form_module_path(&path.as_path().to_string_lossy()) else {
                continue;
            };
            if !self.matches(&key) {
                continue;
            }

            let tree = db.symbol_tree(ModuleId::new(file_id));
            for method in tree.methods().filter(|method| method.name.eq_ignore_case(&member)) {
                out.push(Declaration {
                    file_id,
                    name_range: method.name_range,
                    enclosing_range: Some(method.source_range),
                    kind: DeclarationKind::Method,
                });
            }
        }

        out.sort_by_key(|declaration| (declaration.file_id.0, declaration.name_range.start()));
        out
    }

    fn matches(&self, key: &FormKey) -> bool {
        let owner_matches = match (self.owner, &key.owner) {
            (None, None) => true,
            (Some((mdo_type, object)), Some((key_type, key_name))) => {
                mdo_type == *key_type && object.fold_lower() == key_name.fold_lower()
            }
            _ => false,
        };
        owner_matches && self.form.fold_lower() == key.form_name.fold_lower()
    }
}

enum Owner {
    Common { name: String },
    Object { mdo_type: bsl_metadata::MdoType, name: String },
}

impl Owner {
    /// Object, manager and record-set modules share the `Тип.Объект.Метод`
    /// spelling — the same one `symbol_info` accepts and the name dictionary
    /// prints — so all three match one owner.
    fn matches(&self, key: &ModuleKey) -> bool {
        match (self, key) {
            (Owner::Common { name }, ModuleKey::Common { name: key_name }) => {
                *name == key_name.fold_lower()
            }
            (
                Owner::Object { mdo_type, name },
                ModuleKey::Manager { mdo_type: key_type, name: key_name }
                | ModuleKey::Object { mdo_type: key_type, name: key_name }
                | ModuleKey::RecordSet { mdo_type: key_type, name: key_name },
            ) => mdo_type == key_type && *name == key_name.fold_lower(),
            _ => false,
        }
    }
}

fn parse_mdo_type(s: &str) -> Option<bsl_metadata::MdoType> {
    s.parse::<bsl_metadata::MdoType>().ok().or_else(|| bsl_metadata::MdoType::from_plural(s))
}
