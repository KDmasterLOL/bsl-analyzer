//! Locale-aware rendering — `display_name(&TypeKind, &dyn DisplayCtx,
//! &dyn TypeKernelDb) -> String`.
//!
//! The only function that renders types as user-visible strings.
//! `Display` on `TypeKind` (derived) is debug-only.
//!
//! See design v5 §4.5 for the contract. Phase 1 covers all `TypeKind`
//! variants; later phases will refine register-inner / form labels
//! when callers need them.

use std::fmt::Write;

use crate::facet::{
    ArrayFacet, DateFacet, FormBindingFacet, FormBindingTargetFacet, FunctionFacet, MapFacet,
    MdoRefFacet, MetaObjFacet, MetaRefFacet, NumberFacet, PlatformObjectFacet, ProjectionFacet,
    StringFacet, StructureFacet, TableFacet,
};
use crate::intern::TypeKernelDb;
use crate::kind::{MetadataKind, Projection, TypeId, TypeKind};
use bsl_metadata::MdoType;

/// Manager-collection label for an MDO family — `СправочникМенеджер` /
/// `CatalogManager`, falling back to the generic collection label when the
/// MDO has no manager-prefix form. Shared by `ManagerCollection` and
/// `AnyMetadataRef` (Phase 0 aliases the latter to the former's shape).
fn manager_collection_label(mdo: MdoType, locale: Locale) -> &'static str {
    match locale {
        Locale::Ru => mdo.manager_type_prefix_ru().unwrap_or("МенеджерКоллекция"),
        Locale::En => mdo.manager_type_prefix().unwrap_or("ManagerCollection"),
    }
}

/// Locale for user-visible labels. Russian is the BSL native; English
/// is the alias surface (used for hover-in-EN clients, doc-comment
/// authors, …).
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum Locale {
    Ru,
    En,
}

/// Rendering context — locale + display preferences.
pub trait DisplayCtx {
    fn locale(&self) -> Locale;

    /// `true` → hover-style rendering, show precision/length/scale.
    /// `false` → completion-style, show bare type name only.
    fn precision_visible(&self) -> bool;
}

/// Trivial sandbox `DisplayCtx`. Production wires its own backed by
/// user preferences.
pub struct PlainDisplayCtx {
    pub locale: Locale,
    pub precision_visible: bool,
}

impl PlainDisplayCtx {
    pub fn hover_ru() -> Self {
        Self { locale: Locale::Ru, precision_visible: true }
    }

    pub fn hover_en() -> Self {
        Self { locale: Locale::En, precision_visible: true }
    }

    pub fn completion_ru() -> Self {
        Self { locale: Locale::Ru, precision_visible: false }
    }
}

impl DisplayCtx for PlainDisplayCtx {
    fn locale(&self) -> Locale {
        self.locale
    }

    fn precision_visible(&self) -> bool {
        self.precision_visible
    }
}

/// Render a `TypeKind` as a user-visible string.
///
/// `db` is needed to resolve nested `TypeId`s (Union members, Array
/// element types, projection field types, …).
pub fn display_name(kind: &TypeKind, ctx: &dyn DisplayCtx, db: &dyn TypeKernelDb) -> String {
    let mut buf = String::new();
    render(kind, ctx, db, &mut buf);
    buf
}

fn render(kind: &TypeKind, ctx: &dyn DisplayCtx, db: &dyn TypeKernelDb, buf: &mut String) {
    match kind {
        TypeKind::Unknown => buf.push_str(match ctx.locale() {
            Locale::Ru => "Неизвестно",
            Locale::En => "Unknown",
        }),
        TypeKind::Never => buf.push_str(match ctx.locale() {
            Locale::Ru => "Никогда",
            Locale::En => "Never",
        }),
        TypeKind::Any => buf.push_str(match ctx.locale() {
            Locale::Ru => "Произвольный",
            Locale::En => "Any",
        }),
        TypeKind::Boolean => buf.push_str(match ctx.locale() {
            Locale::Ru => "Булево",
            Locale::En => "Boolean",
        }),
        TypeKind::Null => buf.push_str("NULL"),
        TypeKind::Undefined => buf.push_str(match ctx.locale() {
            Locale::Ru => "Неопределено",
            Locale::En => "Undefined",
        }),
        TypeKind::Uuid => buf.push_str(match ctx.locale() {
            Locale::Ru => "УникальныйИдентификатор",
            Locale::En => "UUID",
        }),
        TypeKind::Number(facet) => render_number(facet, ctx, buf),
        TypeKind::String(facet) => render_string(facet, ctx, buf),
        TypeKind::Date(facet) => render_date(facet, ctx, buf),
        TypeKind::Array(facet) => render_array(facet, ctx, db, buf),
        TypeKind::Map(facet) => render_map(facet, ctx, db, buf),
        TypeKind::Structure(facet) => render_structure(facet, ctx, buf),
        TypeKind::ValueList(elem) => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "СписокЗначений",
                Locale::En => "ValueList",
            });
            if let Some(id) = elem {
                buf.push_str(match ctx.locale() {
                    Locale::Ru => " из ",
                    Locale::En => " of ",
                });
                render(db.lookup_type(*id), ctx, db, buf);
            }
        }
        TypeKind::ValueTable(facet) => render_table(facet, ctx, db, /* row */ false, buf),
        TypeKind::ValueTableRow(facet) => render_table(facet, ctx, db, /* row */ true, buf),
        TypeKind::ValueStorage => buf.push_str(match ctx.locale() {
            Locale::Ru => "ХранилищеЗначения",
            Locale::En => "ValueStorage",
        }),
        TypeKind::TypeDescriptor => buf.push_str(match ctx.locale() {
            Locale::Ru => "Тип",
            Locale::En => "Type",
        }),
        TypeKind::PlatformObject(PlatformObjectFacet { name }) => buf.push_str(name),
        TypeKind::MetadataRef(facet) => render_meta_ref(facet, ctx, buf),
        TypeKind::MetadataObject(facet) => render_meta_obj(facet, ctx, buf),
        TypeKind::AnyMetadataRef { mdo_type } => {
            // A flavour-scoped any-ref reads like the bare reference kind
            // (`СправочникСсылка`), NOT the manager collection
            // (`Справочники`) — it denotes "some reference of this
            // flavour", a value, not the manager. Fall back to the
            // collection label only for flavours with no `*Ref` kind.
            match MetadataKind::ref_kind_for(*mdo_type) {
                Some(kind) => buf.push_str(kind.display_label(ctx.locale())),
                None => buf.push_str(manager_collection_label(*mdo_type, ctx.locale())),
            }
        }
        TypeKind::AnyRef => buf.push_str(match ctx.locale() {
            Locale::Ru => "ЛюбаяСсылка",
            Locale::En => "AnyRef",
        }),
        TypeKind::ManagerCollection(mdo_type) => {
            buf.push_str(manager_collection_label(*mdo_type, ctx.locale()));
        }
        TypeKind::ObjectManager(facet) => {
            let kind_label = match ctx.locale() {
                Locale::Ru => facet.mdo.russian_name(),
                Locale::En => facet.mdo.english_name(),
            };
            write!(buf, "{}.{}", kind_label, facet.name).unwrap();
        }
        // Parent-qualified so the owning MDO disambiguates same-named
        // sections (`Catalog X.Товары` vs `Document X.Товары`).
        TypeKind::TabularSection { parent, name } => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "ТабличнаяЧасть<",
                Locale::En => "TabularSection<",
            });
            write!(buf, "{}.{}", parent.name, name).unwrap();
            buf.push('>');
        }
        TypeKind::TabularSectionRow { parent, name } => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "СтрокаТабличнойЧасти<",
                Locale::En => "TabularSectionRow<",
            });
            write!(buf, "{}.{}", parent.name, name).unwrap();
            buf.push('>');
        }
        TypeKind::RegisterDimension { parent, name } => {
            let label = match ctx.locale() {
                Locale::Ru => "Измерение",
                Locale::En => "Dimension",
            };
            write!(buf, "{}<{}.{}>", label, parent.name, name).unwrap();
        }
        TypeKind::RegisterResource { parent, name } => {
            let label = match ctx.locale() {
                Locale::Ru => "Ресурс",
                Locale::En => "Resource",
            };
            write!(buf, "{}<{}.{}>", label, parent.name, name).unwrap();
        }
        TypeKind::RegisterAttribute { parent, name } => {
            let label = match ctx.locale() {
                Locale::Ru => "Реквизит",
                Locale::En => "Attribute",
            };
            write!(buf, "{}<{}.{}>", label, parent.name, name).unwrap();
        }
        TypeKind::RegisterFilter { .. } => buf.push_str(match ctx.locale() {
            Locale::Ru => "Отбор",
            Locale::En => "Filter",
        }),
        TypeKind::Attribute { parent, name } => {
            write!(buf, "{}.{}", parent.name, name).unwrap();
        }
        TypeKind::FormData { kind, underlying } => {
            // Concrete platform wrapper (`ДанныеФормыСтруктура` /
            // `…Коллекция` / `…СтруктураСКоллекцией`) — locale-independent
            // platform key, mirroring `Ty::display`'s wrapper.
            buf.push_str(kind.platform_type_name());
            if let Some(owner) = underlying {
                buf.push(':');
                render_mdo_ref(owner, ctx, buf);
            }
        }
        TypeKind::FormControl { kind, binding } => {
            // Per-kind platform wrapper name (`ПолеФормы`, `ТаблицаФормы`,
            // …) — locale-independent platform key. `Other` has no wrapper;
            // fall back to the generic localized label.
            buf.push_str(kind.base_platform_type_name().unwrap_or(match ctx.locale() {
                Locale::Ru => "ЭлементФормы",
                Locale::En => "FormControl",
            }));
            if let Some(binding) = binding {
                render_form_binding(binding, ctx, db, buf);
            }
        }
        TypeKind::ThisObject { owner, .. } => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "ЭтотОбъект",
                Locale::En => "ThisObject",
            });
            buf.push(':');
            render_mdo_ref(owner, ctx, buf);
        }
        TypeKind::ThisManager { owner, .. } => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "ЭтотМенеджер",
                Locale::En => "ThisManager",
            });
            buf.push(':');
            render_mdo_ref(owner, ctx, buf);
        }
        TypeKind::Union(members) => render_union(members.as_ref(), ctx, db, buf),
        TypeKind::Function(facet) => render_function(facet, ctx, db, buf),
        TypeKind::QueryResult(facet) => render_query_result(facet, ctx, db, buf),
        TypeKind::QueryResultSelection(facet) => {
            buf.push_str(match ctx.locale() {
                Locale::Ru => "ВыборкаИзРезультатаЗапроса",
                Locale::En => "QueryResultSelection",
            });
            render_projection_suffix(&facet.projection, ctx, db, buf);
        }
        TypeKind::QueryBatchResult { .. } => buf.push_str(match ctx.locale() {
            Locale::Ru => "ПакетРезультатовЗапроса",
            Locale::En => "QueryBatchResult",
        }),
        TypeKind::Query { .. } => buf.push_str(match ctx.locale() {
            Locale::Ru => "Запрос",
            Locale::En => "Query",
        }),
    }
}

fn render_number(facet: &NumberFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Число",
        Locale::En => "Number",
    });
    if ctx.precision_visible() {
        match (facet.precision, facet.scale) {
            (Some(p), Some(s)) => {
                write!(buf, "({}, {})", p, s).unwrap();
            }
            (Some(p), None) => {
                write!(buf, "({})", p).unwrap();
            }
            _ => {}
        }
    }
}

fn render_string(facet: &StringFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Строка",
        Locale::En => "String",
    });
    if ctx.precision_visible() {
        if let Some(len) = facet.length {
            write!(buf, "({})", len).unwrap();
        }
    }
}

fn render_date(facet: &DateFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    use crate::facet::DateComponent;
    buf.push_str(match (ctx.locale(), facet.component) {
        (Locale::Ru, DateComponent::Date) => "Дата",
        (Locale::Ru, DateComponent::Time) => "Время",
        (Locale::Ru, DateComponent::DateTime) => "ДатаВремя",
        (Locale::En, DateComponent::Date) => "Date",
        (Locale::En, DateComponent::Time) => "Time",
        (Locale::En, DateComponent::DateTime) => "DateTime",
    });
}

fn render_array(facet: &ArrayFacet, ctx: &dyn DisplayCtx, db: &dyn TypeKernelDb, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Массив",
        Locale::En => "Array",
    });
    if let Some(elem) = facet.element {
        buf.push_str(match ctx.locale() {
            Locale::Ru => " из ",
            Locale::En => " of ",
        });
        render(db.lookup_type(elem), ctx, db, buf);
    }
}

fn render_map(facet: &MapFacet, ctx: &dyn DisplayCtx, db: &dyn TypeKernelDb, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Соответствие",
        Locale::En => "Map",
    });
    if facet.key.is_some() || facet.value.is_some() {
        buf.push('<');
        render_optional(facet.key, ctx, db, buf);
        buf.push_str(", ");
        render_optional(facet.value, ctx, db, buf);
        buf.push('>');
    }
}

fn render_structure(facet: &StructureFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Структура",
        Locale::En => "Structure",
    });
    if let Some(keys) = &facet.keys {
        buf.push('(');
        for (i, k) in keys.iter().enumerate() {
            if i > 0 {
                buf.push_str(", ");
            }
            buf.push_str(k);
        }
        buf.push(')');
    }
}

fn render_table(
    facet: &TableFacet,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    is_row: bool,
    buf: &mut String,
) {
    let base = match (ctx.locale(), is_row) {
        (Locale::Ru, false) => "ТаблицаЗначений",
        (Locale::Ru, true) => "СтрокаТаблицыЗначений",
        (Locale::En, false) => "ValueTable",
        (Locale::En, true) => "ValueTableRow",
    };
    buf.push_str(base);
    render_projection_suffix(&facet.projection, ctx, db, buf);
}

fn render_projection_suffix(
    projection: &Option<std::sync::Arc<Projection>>,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    let Some(proj) = projection else { return };
    if !ctx.precision_visible() {
        return;
    }
    buf.push_str(" { ");
    for (i, field) in proj.fields.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push_str(&field.name);
        buf.push_str(": ");
        // Phase 3 §4.G.5d: prefer the SDBL display shadow when captured —
        // it carries precision/scale/length (`Число(15, 2)`, `Строка(50)`)
        // that the interned `field.ty` drops. Falls back to kernel rendering
        // of `field.ty` when no shadow is present (`raw_sdbl_types` is `None`,
        // or indices don't line up).
        match proj.raw_sdbl_types.as_deref().and_then(|shadows| shadows.get(i)) {
            Some(shadow) => buf.push_str(&shadow.display),
            None => render(db.lookup_type(field.ty), ctx, db, buf),
        }
    }
    buf.push_str(" }");
}

fn render_meta_ref(facet: &MetaRefFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    // Phase 3 §4.G.5d: `MetadataKind::display_label` is the exhaustive
    // bilingual label source — no `Debug` leak for kinds (TabularSection,
    // register refs, …) the old curated match didn't pin.
    buf.push_str(facet.kind.display_label(ctx.locale()));
    buf.push('.');
    buf.push_str(&facet.name);
}

fn render_meta_obj(facet: &MetaObjFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    // Phase 3 §4.G.5d: exhaustive bilingual label via `display_label`
    // (no `Debug` leak for unlisted object kinds).
    buf.push_str(facet.kind.display_label(ctx.locale()));
    buf.push('.');
    buf.push_str(&facet.name);
}

fn render_mdo_ref(facet: &MdoRefFacet, ctx: &dyn DisplayCtx, buf: &mut String) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => facet.mdo_type.russian_name(),
        Locale::En => facet.mdo_type.english_name(),
    });
    buf.push('.');
    buf.push_str(&facet.name);
}

fn render_form_binding(
    binding: &FormBindingFacet,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    buf.push(':');
    if !binding.path.is_empty() {
        for (i, segment) in binding.path.iter().enumerate() {
            if i > 0 {
                buf.push('.');
            }
            buf.push_str(segment);
        }
        buf.push_str(" -> ");
    }
    render_form_binding_target(&binding.target, ctx, db, buf);
}

fn render_form_binding_target(
    target: &FormBindingTargetFacet,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    match target {
        FormBindingTargetFacet::TabularSection { mdo_ref, section } => {
            render_mdo_ref(mdo_ref, ctx, buf);
            buf.push('.');
            buf.push_str(section);
        }
        FormBindingTargetFacet::Attribute { ty } => render(db.lookup_type(*ty), ctx, db, buf),
    }
}

fn render_union(members: &[TypeId], ctx: &dyn DisplayCtx, db: &dyn TypeKernelDb, buf: &mut String) {
    for (i, &m) in members.iter().enumerate() {
        if i > 0 {
            buf.push_str(" | ");
        }
        render(db.lookup_type(m), ctx, db, buf);
    }
}

fn render_function(
    facet: &FunctionFacet,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "Функция(",
        Locale::En => "Function(",
    });
    for (i, p) in facet.params.iter().enumerate() {
        if i > 0 {
            buf.push_str(", ");
        }
        buf.push_str(&p.name);
        buf.push_str(": ");
        render(db.lookup_type(p.ty), ctx, db, buf);
    }
    buf.push_str(") -> ");
    render(db.lookup_type(facet.returns), ctx, db, buf);
}

fn render_query_result(
    facet: &ProjectionFacet,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    buf.push_str(match ctx.locale() {
        Locale::Ru => "РезультатЗапроса",
        Locale::En => "QueryResult",
    });
    render_projection_suffix(&facet.projection, ctx, db, buf);
}

fn render_optional(
    id: Option<TypeId>,
    ctx: &dyn DisplayCtx,
    db: &dyn TypeKernelDb,
    buf: &mut String,
) {
    match id {
        Some(id) => render(db.lookup_type(id), ctx, db, buf),
        None => buf.push('?'),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bsl_metadata::MdoType;
    use expect_test::expect;

    use super::*;
    use crate::builders::Builders;
    use crate::facet::{
        DateComponent, FormBindingFacet, FormBindingTargetFacet, FormDataFacet, FormElementFacet,
        MdoRefFacet,
    };
    use crate::kind::{ConfigId, MetadataKind, ProjectionFieldSource, ProjectionOrigin};
    use crate::testing::{InMemoryDb, RootConfigCtx};

    fn ru() -> PlainDisplayCtx {
        PlainDisplayCtx::hover_ru()
    }

    fn en() -> PlainDisplayCtx {
        PlainDisplayCtx::hover_en()
    }

    fn show(db: &InMemoryDb, id: TypeId, ctx: &dyn DisplayCtx) -> String {
        display_name(db.lookup_type(id), ctx, db)
    }

    #[test]
    fn primitives_ru_and_en() {
        let db = InMemoryDb::new();
        expect!["Булево"].assert_eq(&show(&db, db.boolean(), &ru()));
        expect!["Boolean"].assert_eq(&show(&db, db.boolean(), &en()));
        expect!["Неизвестно"].assert_eq(&show(&db, db.unknown(), &ru()));
        expect!["Unknown"].assert_eq(&show(&db, db.unknown(), &en()));
        expect!["Произвольный"].assert_eq(&show(&db, db.any(), &ru()));
        expect!["Any"].assert_eq(&show(&db, db.any(), &en()));
        expect!["NULL"].assert_eq(&show(&db, db.null(), &ru()));
        expect!["Неопределено"].assert_eq(&show(&db, db.undefined(), &ru()));
    }

    #[test]
    fn number_with_precision_hover_vs_completion() {
        let db = InMemoryDb::new();
        let id = db.number(Some(15), Some(2));
        expect!["Число(15, 2)"].assert_eq(&show(&db, id, &PlainDisplayCtx::hover_ru()));
        expect!["Число"].assert_eq(&show(&db, id, &PlainDisplayCtx::completion_ru()));
        expect!["Number(15, 2)"].assert_eq(&show(&db, id, &PlainDisplayCtx::hover_en()));
        // Precision-only.
        let p = db.number(Some(10), None);
        expect!["Число(10)"].assert_eq(&show(&db, p, &PlainDisplayCtx::hover_ru()));
    }

    #[test]
    fn string_length_hover_only() {
        let db = InMemoryDb::new();
        let id = db.string(Some(50), false);
        expect!["Строка(50)"].assert_eq(&show(&db, id, &PlainDisplayCtx::hover_ru()));
        expect!["String(50)"].assert_eq(&show(&db, id, &PlainDisplayCtx::hover_en()));
        expect!["Строка"].assert_eq(&show(&db, id, &PlainDisplayCtx::completion_ru()));
    }

    #[test]
    fn date_components() {
        let db = InMemoryDb::new();
        expect!["Дата"].assert_eq(&show(&db, db.date(DateComponent::Date), &ru()));
        expect!["Время"].assert_eq(&show(&db, db.date(DateComponent::Time), &ru()));
        expect!["ДатаВремя"].assert_eq(&show(&db, db.date(DateComponent::DateTime), &ru()));
        expect!["DateTime"].assert_eq(&show(&db, db.date(DateComponent::DateTime), &en()));
    }

    #[test]
    fn metadata_ref_bilingual() {
        let db = InMemoryDb::new();
        let cfg = RootConfigCtx;
        let cat = db.metadata_ref(MetadataKind::CatalogRef, "Номенклатура".to_string(), &cfg);
        expect!["СправочникСсылка.Номенклатура"].assert_eq(&show(&db, cat, &ru()));
        expect!["CatalogRef.Номенклатура"].assert_eq(&show(&db, cat, &en()));
    }

    #[test]
    fn metadata_ref_tabular_section_uses_label_not_debug() {
        // §4.G.5d regression guard: kinds the old curated match didn't pin
        // (TabularSection, register refs, …) must render through
        // `MetadataKind::display_label`, NOT the Rust `Debug` shape. A
        // non-generic name (`Товары`) ensures we're checking the kind label,
        // not a fixture name that coincidentally contains the expected word.
        let db = InMemoryDb::new();
        let cfg = RootConfigCtx;
        let ts = db.metadata_ref(
            MetadataKind::TabularSection { parent: MdoType::Catalog },
            "Номенклатура.Товары".to_string(),
            &cfg,
        );
        expect!["ТабличнаяЧасть.Номенклатура.Товары"].assert_eq(&show(&db, ts, &ru()));
        expect!["TabularSection.Номенклатура.Товары"].assert_eq(&show(&db, ts, &en()));
    }

    #[test]
    fn any_ref_renders_localized_label() {
        // `AnyRef` is the `ЛюбаяСсылка` supertype — rendered with the bare
        // localized word, not the manager-collection label used by the
        // flavoured `AnyMetadataRef`.
        let db = InMemoryDb::new();
        expect!["ЛюбаяСсылка"].assert_eq(&show(&db, db.any_ref(), &ru()));
        expect!["AnyRef"].assert_eq(&show(&db, db.any_ref(), &en()));
    }

    #[test]
    fn any_metadata_ref_renders_ref_kind_label() {
        // A flavoured any-ref reads like the bare reference kind
        // (`СправочникСсылка`), NOT the manager collection (`Справочники`)
        // — it is a reference value, not the manager.
        let db = InMemoryDb::new();
        let any_catalog = db.any_metadata_ref(MdoType::Catalog);
        expect!["СправочникСсылка"].assert_eq(&show(&db, any_catalog, &ru()));
        expect!["CatalogRef"].assert_eq(&show(&db, any_catalog, &en()));
    }

    #[test]
    fn tabular_section_label_is_parent_qualified() {
        // The owning MDO is part of the label so `Catalog "X".Товары` and
        // `Document "X".Товары` don't collide.
        use crate::kind::MetadataKind;
        let db = InMemoryDb::new();
        let cfg = RootConfigCtx;
        let parent = db.meta_ref_facet(MetadataKind::CatalogRef, "Номенклатура".to_string(), &cfg);
        let ts = db.tabular_section(parent.clone(), "Товары".to_string());
        let row = db.tabular_section_row(parent, "Товары".to_string());
        expect!["ТабличнаяЧасть<Номенклатура.Товары>"].assert_eq(&show(&db, ts, &ru()));
        expect!["TabularSection<Номенклатура.Товары>"].assert_eq(&show(&db, ts, &en()));
        expect!["СтрокаТабличнойЧасти<Номенклатура.Товары>"].assert_eq(&show(&db, row, &ru()));
    }

    #[test]
    fn array_of_element() {
        let db = InMemoryDb::new();
        let n = db.number(None, None);
        let arr = db.array(Some(n));
        expect!["Массив из Число"].assert_eq(&show(&db, arr, &ru()));
        expect!["Array of Number"].assert_eq(&show(&db, arr, &en()));

        // Bare array — no element clause.
        let bare = db.array(None);
        expect!["Массив"].assert_eq(&show(&db, bare, &ru()));
    }

    #[test]
    fn register_inner_variants_use_ctx_locale() {
        // Labels honour the context locale and are parent-qualified
        // (`<Регистр>.<Имя>`) so the owning register is visible.
        use crate::kind::MetadataKind;
        let db = InMemoryDb::new();
        let cfg = RootConfigCtx;
        let parent =
            db.meta_ref_facet(MetadataKind::InformationRegisterRecordSet, "Цены".to_string(), &cfg);
        let dim = db.register_dimension(parent.clone(), "Период".to_string());
        let res = db.register_resource(parent.clone(), "Сумма".to_string());
        let att = db.register_attribute(parent.clone(), "Комментарий".to_string());
        let filt = db.register_filter(parent);

        expect!["Измерение<Цены.Период>"].assert_eq(&show(&db, dim, &ru()));
        expect!["Dimension<Цены.Период>"].assert_eq(&show(&db, dim, &en()));
        expect!["Ресурс<Цены.Сумма>"].assert_eq(&show(&db, res, &ru()));
        expect!["Resource<Цены.Сумма>"].assert_eq(&show(&db, res, &en()));
        expect!["Реквизит<Цены.Комментарий>"].assert_eq(&show(&db, att, &ru()));
        expect!["Attribute<Цены.Комментарий>"].assert_eq(&show(&db, att, &en()));
        expect!["Отбор"].assert_eq(&show(&db, filt, &ru()));
        expect!["Filter"].assert_eq(&show(&db, filt, &en()));
    }

    #[test]
    fn union_pipe_separated_deterministic() {
        let db = InMemoryDb::new();
        let n = db.number(None, None);
        let s = db.string(None, false);
        let u = db.union(vec![n, s]);
        // Member order is canonicalised by sort; either rendering is
        // valid, but it must be deterministic across re-interns. We
        // assert determinism (re-intern same id, same string), not the
        // specific order.
        let rendered = show(&db, u, &ru());
        let u2 = db.union(vec![s, n]);
        assert_eq!(show(&db, u2, &ru()), rendered);
        // Sanity: rendering matches one of the two valid orderings.
        assert!(rendered == "Число | Строка" || rendered == "Строка | Число", "got {:?}", rendered);
    }

    #[test]
    fn query_result_with_projection() {
        let db = InMemoryDb::new();
        let n = db.number(Some(15), Some(2));
        let s = db.string(None, false);
        let proj = db.projection_from_fields(
            vec![("Цена".to_string(), n), ("Наименование".to_string(), s)],
            ProjectionFieldSource::Column,
            ProjectionOrigin::SdblQuery,
        );
        let qr = db.query_result(Some(proj), crate::facet::ProjectionSource::Sdbl);

        // Hover (with precision_visible) shows the projection columns.
        expect!["РезультатЗапроса { Цена: Число(15, 2), Наименование: Строка }"].assert_eq(&show(
            &db,
            qr,
            &ru(),
        ));

        // Completion mode hides projection.
        expect!["РезультатЗапроса"].assert_eq(&show(&db, qr, &PlainDisplayCtx::completion_ru()));
    }

    #[test]
    fn projection_prefers_sdbl_display_shadow() {
        // §4.G.5d: when `raw_sdbl_types` is captured, the hover suffix must
        // use the pre-rendered SDBL label (precision/scale/length) rather
        // than the interned `field.ty` (which drops them). Here `field.ty`
        // is a bare `Число` (no precision) but the shadow says `Число(15, 2)`.
        use crate::facet::SdblTypeShadowFacet;
        use crate::kind::{Projection, ProjectionField};

        let db = InMemoryDb::new();
        let bare_number = db.number(None, None);
        let fields: Arc<[ProjectionField]> = Arc::from([ProjectionField::new(
            "Цена".to_string(),
            bare_number,
            ProjectionFieldSource::Column,
        )]);
        let shadows: Arc<[SdblTypeShadowFacet]> =
            Arc::from([SdblTypeShadowFacet::new("Число(15, 2)".to_string())]);
        let proj = Arc::new(Projection::new(fields, ProjectionOrigin::SdblQuery, Some(shadows)));
        let qr = db.query_result(Some(proj), crate::facet::ProjectionSource::Sdbl);

        // Shadow wins over the bare interned `field.ty` (`Число`).
        expect!["РезультатЗапроса { Цена: Число(15, 2) }"].assert_eq(&show(&db, qr, &ru()));
    }

    #[test]
    fn function_renders_params_and_return() {
        use crate::facet::{ArgArity, FunctionFacet, FunctionOrigin, ParamPassing, ParamSpec};

        let db = InMemoryDb::new();
        let n = db.number(None, None);
        let s = db.string(None, false);

        let facet = FunctionFacet {
            params: Arc::from([
                ParamSpec {
                    name: "Цена".to_string(),
                    ty: n,
                    passing: ParamPassing::ByRef,
                    variadic: false,
                },
                ParamSpec {
                    name: "Имя".to_string(),
                    ty: s,
                    passing: ParamPassing::ByVal,
                    variadic: false,
                },
            ]),
            defaults: Arc::from([None, None]),
            min_args: 2,
            max_args: ArgArity::Fixed(2),
            returns: n,
            origin: FunctionOrigin::UserDefined,
        };
        let id = db.function(facet);
        expect!["Функция(Цена: Число, Имя: Строка) -> Число"].assert_eq(&show(&db, id, &ru()));
    }

    #[test]
    fn form_variants_render_bilingually_with_payloads() {
        let db = InMemoryDb::new();
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let form_data =
            db.mk_form_data(FormDataFacet::StructureWithCollection, Some(owner.clone()));
        expect!["ДанныеФормыСтруктураСКоллекцией:Справочник.Контрагенты"].assert_eq(&show(
            &db,
            form_data,
            &ru(),
        ));
        expect!["ДанныеФормыСтруктураСКоллекцией:Catalog.Контрагенты"].assert_eq(&show(
            &db,
            form_data,
            &en(),
        ));

        // The concrete wrapper distinguishes the three form-data shapes.
        let structure = db.mk_form_data(FormDataFacet::Structure, Some(owner.clone()));
        expect!["ДанныеФормыСтруктура:Справочник.Контрагенты"].assert_eq(&show(
            &db,
            structure,
            &ru(),
        ));
        let collection = db.mk_form_data(FormDataFacet::Collection, None);
        expect!["ДанныеФормыКоллекция"].assert_eq(&show(&db, collection, &ru()));

        let binding = FormBindingFacet {
            path: Arc::from(["Объект".to_string(), "Наименование".to_string()]),
            target: FormBindingTargetFacet::Attribute { ty: db.string(Some(50), false) },
        };
        let control = db.mk_form_control(FormElementFacet::Field, Some(binding));
        expect!["ПолеФормы:Объект.Наименование -> Строка(50)"].assert_eq(&show(
            &db,
            control,
            &ru(),
        ));
        expect!["ПолеФормы:Объект.Наименование -> String(50)"].assert_eq(&show(
            &db,
            control,
            &en(),
        ));
    }

    #[test]
    fn this_variants_render_owner_bilingually() {
        let db = InMemoryDb::new();
        let owner = MdoRefFacet {
            mdo_type: MdoType::Document, name: "ЗаказКлиента".to_string()
        };
        let object = db.mk_this_object(ConfigId::Root, owner.clone());
        let manager = db.mk_this_manager(ConfigId::Root, owner);

        expect!["ЭтотОбъект:Документ.ЗаказКлиента"].assert_eq(&show(&db, object, &ru()));
        expect!["ThisObject:Document.ЗаказКлиента"].assert_eq(&show(&db, object, &en()));
        expect!["ЭтотМенеджер:Документ.ЗаказКлиента"].assert_eq(&show(&db, manager, &ru()));
        expect!["ThisManager:Document.ЗаказКлиента"].assert_eq(&show(&db, manager, &en()));
    }

    #[test]
    fn form_control_tabular_section_binding_renders_target() {
        let db = InMemoryDb::new();
        let owner =
            MdoRefFacet { mdo_type: MdoType::Catalog, name: "Контрагенты".to_string() };
        let binding = FormBindingFacet {
            path: Arc::from(["Объект".to_string(), "Товары".to_string()]),
            target: FormBindingTargetFacet::TabularSection {
                mdo_ref: owner,
                section: "Товары".to_string(),
            },
        };
        let control = db.mk_form_control(FormElementFacet::Table, Some(binding));

        expect!["ТаблицаФормы:Объект.Товары -> Справочник.Контрагенты.Товары"].assert_eq(&show(
            &db,
            control,
            &ru(),
        ));
        expect!["ТаблицаФормы:Объект.Товары -> Catalog.Контрагенты.Товары"].assert_eq(&show(
            &db,
            control,
            &en(),
        ));
    }
}
