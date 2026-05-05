//! `Элементы.<имя>` → [`Ty::FormControl`] resolution.
//!
//! Inside a managed-form module (`Forms/<X>/Ext/Form/Module.bsl`),
//! `Элементы` is the platform-typed collection of UI controls
//! (`ВсеЭлементыФормы` / `FormAllItems`). The next field-access step
//! (`Элементы.Переприемка`, `Элементы.Кнопка1`) lands here: we resolve
//! the name against `Form.xml`'s `<ChildItems>` (captured during XML
//! parsing as [`bsl_metadata::FormElement`]) and lower the matching
//! element to a [`Ty::FormControl { kind, binding }`].
//!
//! `kind` comes from the XML tag taxonomy (Phase 2 wired this through
//! `bsl_metadata::FormElement::kind`). `binding` carries the resolved
//! `<DataPath>` provenance for row-aware refinement in Phase 5
//! (`.ВыделенныеСтроки → TypedArray(row)`); a missing or unresolvable
//! data path leaves `binding: None` and the control type still routes
//! method/property dispatch through its per-kind platform table.
//!
//! # Cheap-first lookup
//!
//! [`lookup_form_item_field`] checks the receiver shape **before**
//! asking `db.module_metadata(...)` — a non-`ВсеЭлементыФормы`
//! receiver returns immediately with no Salsa cost. The managed-form
//! gate (`Resolver::resolve_this_form`) is the second gate; both stay
//! shallow so the inference hot path pays nothing on unrelated
//! field-access expressions.
//!
//! # Out of scope
//!
//! - Refined property lookup on `Ty::FormControl{Table, Some(_)}` for
//!   `.ВыделенныеСтроки` / `.ТекущаяСтрока`. Phase 5 layers this on
//!   top via `field_lookup`.
//! - Method dispatch — handled by `method_lookup::platform_type_key`
//!   which already routes `Ty::FormControl` through the per-kind
//!   platform table.

use bsl_metadata::{Form, FormElement};
use hir_def::configs::VisibleConfig;
use hir_def::resolver::Resolver;
use hir_def::ty::{FormDataBinding, FormDataTarget, FormElementKind, MetadataKind, Ty};
use hir_def::Name;

use crate::db::HirDatabase;
use crate::field_enum::{FieldInfo, FieldOrigin};
use crate::field_lookup;
use crate::form_attr::lower_form_attribute_to_ty;

/// Russian platform type name for the form-elements collection.
pub const FORM_ITEMS_TYPE_RU: &str = "ВсеЭлементыФормы";
/// English platform type name for the form-elements collection.
pub const FORM_ITEMS_TYPE_EN: &str = "FormAllItems";

/// `true` if `ty` is the form-elements collection (`Элементы` / `Items`
/// receiver). Mirrors the receiver gate used by [`lookup_form_item_field`]
/// so completion can offer the same suggestion list that the field-access
/// pipeline resolves against — single source of truth, no IDE-side
/// duplicate of the bilingual case-folding rule.
pub fn is_form_items_collection_ty(ty: &Ty) -> bool {
    let Ty::PlatformObject(name) = ty else { return false };
    name.eq_ignore_case(&Name::new(FORM_ITEMS_TYPE_RU))
        || name.eq_ignore_case(&Name::new(FORM_ITEMS_TYPE_EN))
}

/// Resolve `Элементы.<field>` against the form's XML element table.
///
/// Returns `Some(FieldInfo)` only when **all** are true:
/// 1. `base_ty` is `Ty::PlatformObject("ВсеЭлементыФормы" | "FormAllItems")`
///    (case-insensitive, bilingual);
/// 2. the resolver's enclosing module is a managed form
///    ([`Resolver::resolve_this_form`] gate — strict, ordinary forms and
///    forms without a loaded `Form.xml` payload return `false`);
/// 3. the form metadata declares an element with this name
///    (case-insensitive — BSL identifiers are case-insensitive).
///
/// Otherwise returns `None` so the caller can fall through to
/// `lookup_field` (which may still find a platform property on the
/// `ВсеЭлементыФормы` type, e.g. `.Количество()` if such ever lands in
/// platform data).
///
/// `is_readonly` is set to `true`: `Элементы.X` returns the control
/// reference itself; the **slot** in the `Элементы` collection is not
/// assignable (BSL has no syntax to overwrite a control reference at
/// that name).
pub(crate) fn lookup_form_item_field(
    db: &dyn HirDatabase,
    resolver: &Resolver,
    base_ty: &Ty,
    field: &Name,
) -> Option<FieldInfo> {
    let is_form_items_receiver = match base_ty {
        Ty::PlatformObject(name) => {
            // BSL identifiers are case-insensitive AND the platform names
            // are Cyrillic — ASCII case folding (`eq_ignore_ascii_case`)
            // does NOT cover Cyrillic, so a mixed-case spelling like
            // `вСеЭлементыФормы` would silently miss. Use `Name::eq_ignore_case`
            // which lowercases both sides via the same Unicode-aware
            // path the rest of the resolver uses (mirrors `scope.rs` /
            // `narrow.rs`).
            name.eq_ignore_case(&Name::new(FORM_ITEMS_TYPE_RU))
                || name.eq_ignore_case(&Name::new(FORM_ITEMS_TYPE_EN))
        }
        _ => false,
    };
    if !is_form_items_receiver {
        return None;
    }
    if !resolver.resolve_this_form(db) {
        return None;
    }
    let module_id = resolver.module_id()?;
    let metadata = db.module_metadata(module_id);
    let form = metadata.form.as_ref()?;
    let element = form.find_element(field.as_str())?;
    let configs = db.configurations(module_id.file_id);
    let ty = lower_form_element(form, element, &configs);
    Some(FieldInfo {
        name: Name::new(&element.name),
        name_en: None,
        ty,
        is_readonly: true,
        origin: FieldOrigin::PlatformProperty,
    })
}

/// Lower a single [`FormElement`] to a [`Ty::FormControl`].
///
/// Pulls `kind` from the XML-tag taxonomy and resolves the optional
/// `<DataPath>` binding via [`resolve_data_path`]. A `~`-prefixed path
/// (the platform's marker for a deleted form attribute) and any path
/// whose first segment does not match a form attribute both collapse
/// to `binding: None` — the wider `Ty::FormControl` is still useful
/// (method dispatch and the kind-specific property table both work).
///
/// Pure on `(form, element, configs)` — split out from
/// [`lookup_form_item_field`] so the lowering rules can be unit-tested
/// without spinning up a Salsa database. Mirrors the
/// [`lower_form_attribute_to_ty`] / [`crate::form_attr::resolve_form_attribute`]
/// pair.
pub(crate) fn lower_form_element(
    form: &Form,
    element: &FormElement,
    configs: &[VisibleConfig],
) -> Ty {
    let binding = element
        .data_path
        .as_deref()
        .filter(|dp| !dp.starts_with('~'))
        .and_then(|dp| resolve_data_path(dp, form, configs));
    Ty::FormControl { kind: element.kind, binding }
}

/// Build the row Ty for a tabular-section binding — the same shape
/// `field_enum::enumerate_fields` produces for tabular-section
/// iteration. `MetadataKind::TabularSectionRow { parent: mdo }` carries
/// the column schema; the qualified name `"Owner.Section"` lets the
/// enumerator find the section inside the right MDO.
fn row_ty_of_tabular_section_target(target: &FormDataTarget) -> Option<Ty> {
    match target {
        FormDataTarget::TabularSection { mdo_type, owner, section } => Some(Ty::MetadataRef {
            kind: MetadataKind::TabularSectionRow { parent: *mdo_type },
            name: Name::new(&format!("{}.{}", owner.as_str(), section.as_str())),
        }),
        FormDataTarget::Attribute { .. } => None,
    }
}

/// Refined property lookup on `Ty::FormControl{Table, Some(b)}` for
/// the row-aware properties — `.ВыделенныеСтроки` / `.ТекущаяСтрока` /
/// `.ТекущиеДанные` and their English aliases.
///
/// Returns `Some(FieldInfo)` only when **all** are true:
/// - receiver is `Ty::FormControl{kind: Table, binding: Some(b)}`,
/// - `b.target` is `TabularSection{mdo_type, owner, section}` (Phase 5
///   row refinement is scoped to MDO tabular sections — see Phase 4
///   docs on the `<Columns>` follow-up),
/// - `field` matches one of the refined property names
///   (case-insensitive, bilingual via `Name::eq_ignore_case`).
///
/// Otherwise returns `None` so the caller falls through to
/// [`crate::platform_property_lookup::lookup_platform_property`], which
/// resolves un-refined `ТаблицаФормы` properties (`.Видимость`,
/// `.Заголовок`, `.УсловноеОформление`, …) through the platform table
/// indirected by [`Ty::platform_type_name`].
///
/// `is_readonly` matches `platform_data.json` for these three
/// properties — the slot itself is read-only.
pub(crate) fn refine_form_control_property(receiver_ty: &Ty, field: &Name) -> Option<FieldInfo> {
    let Ty::FormControl { kind: FormElementKind::Table, binding: Some(binding) } = receiver_ty
    else {
        return None;
    };
    let row = row_ty_of_tabular_section_target(binding.target())?;

    // Bilingual canonical names — recreated as `Name` so
    // `eq_ignore_case` runs the same Unicode-aware fold the rest of
    // the resolver uses.
    let selected_rows_ru = Name::new("ВыделенныеСтроки");
    let selected_rows_en = Name::new("SelectedRows");
    let current_row_ru = Name::new("ТекущаяСтрока");
    let current_row_en = Name::new("CurrentRow");
    let current_data_ru = Name::new("ТекущиеДанные");
    let current_data_en = Name::new("CurrentData");

    // Per-property `is_readonly` mirrors `bsl-platform/data/platform_data.json`:
    // - `ВыделенныеСтроки` / `SelectedRows` → readonly (the slot itself
    //   is platform-managed; user can mutate the array contents but
    //   not reassign the slot).
    // - `ТекущиеДанные` / `CurrentData` → readonly.
    // - `ТекущаяСтрока` / `CurrentRow` → **writable** (assigning a row
    //   activates it; matches `is_readonly: false` in platform_data).
    let (canonical_ru, canonical_en, ty, is_readonly) =
        if field.eq_ignore_case(&selected_rows_ru) || field.eq_ignore_case(&selected_rows_en) {
            // `.ВыделенныеСтроки` — refined from platform's bare `Массив`
            // to `TypedArray(row)` so iteration / indexing yields the
            // section row Ty rather than `Произвольный → Unknown`.
            (selected_rows_ru, selected_rows_en, Ty::TypedArray(Box::new(row)), true)
        } else if field.eq_ignore_case(&current_row_ru) || field.eq_ignore_case(&current_row_en) {
            (current_row_ru, current_row_en, row, false)
        } else if field.eq_ignore_case(&current_data_ru) || field.eq_ignore_case(&current_data_en) {
            (current_data_ru, current_data_en, row, true)
        } else {
            return None;
        };

    Some(FieldInfo {
        name: canonical_ru,
        name_en: Some(canonical_en),
        ty,
        is_readonly,
        origin: FieldOrigin::PlatformProperty,
    })
}

/// Walk `<DataPath>` segment-by-segment to recover the binding's
/// provenance: the chain itself ([`FormDataBinding::path`]) and the
/// resolved target type at the chain's tail
/// ([`FormDataTarget::TabularSection`] / [`FormDataTarget::Attribute`]).
///
/// Resolution flow:
/// 1. Split `data_path` on `.`. The chain is **always** at least one
///    segment ([`FormDataBinding::new`] enforces this on the way out).
/// 2. The first segment must match a form attribute by name
///    (`Form::find_attribute`, case-insensitive). Forms typically
///    contain `Объект` (the main attribute) plus user-declared
///    attributes; both are eligible — the first segment is **not**
///    restricted to the main attribute.
/// 3. Subsequent segments traverse `field_lookup::lookup_field` from
///    the previous segment's resolved Ty. This reuses the same
///    machinery that powers `Объект.Дата` resolution inside the form
///    module — no second resolution pass.
/// 4. Decide the target shape from the tail Ty: a tabular-section
///    `Ty::MetadataRef` carries `(parent: MdoType, name: "Owner.Section")`,
///    which we split into structured `(mdo_type, owner, section)`;
///    everything else collapses to a scalar `Attribute { ty }`.
///
/// Returns `None` for unresolvable paths (unknown first segment,
/// mid-path lookup miss, empty path) so the caller surfaces
/// `binding: None` rather than a half-baked binding.
fn resolve_data_path(
    data_path: &str,
    form: &Form,
    configs: &[VisibleConfig],
) -> Option<FormDataBinding> {
    let segments: Vec<Name> =
        data_path.split('.').filter(|s| !s.is_empty()).map(Name::new).collect();
    let (head, rest) = segments.split_first()?;

    let attr = form.find_attribute(head.as_str())?;
    let mut current_ty = lower_form_attribute_to_ty(attr, configs);

    for seg in rest {
        let info = field_lookup::lookup_field(configs, &current_ty, seg)?;
        current_ty = info.ty;
    }

    let target = match &current_ty {
        // Tabular-section reference: enumerator stores the qualified
        // name as `"Owner.Section"`. Split it back into structured
        // form so Phase 5 doesn't have to re-parse.
        //
        // Scope note: `<Columns>`-backed form attributes (e.g. an
        // attribute typed as `v8:ValueTable` with a `<Columns>`
        // schema) lower to `Ty::FormData{Collection, None}` rather
        // than `Ty::MetadataRef{TabularSection,_}`. Those land in the
        // `other` arm below as `Attribute{FormData(Collection)}` and
        // are NOT promoted to `TabularSection` — Phase 5 row-aware
        // refinement only covers MDO tabular sections. If
        // `<Columns>`-based row schemas need refining later, this is
        // where the dedicated `FormDataTarget::Columns(...)` variant
        // would land.
        Ty::MetadataRef { kind: MetadataKind::TabularSection { parent }, name } => {
            let raw = name.as_str();
            let (owner, section) = raw.rsplit_once('.')?;
            FormDataTarget::TabularSection {
                mdo_type: *parent,
                owner: Name::new(owner),
                section: Name::new(section),
            }
        }
        // Path resolved but the tail is not a tabular-section ref.
        // Surface the resolved Ty as the bound type rather than
        // dropping the provenance entirely — hover still shows the
        // path (debugging aid), and refined lookup gracefully
        // degrades to the kind-specific platform table.
        other => FormDataTarget::Attribute { ty: Box::new(other.clone()) },
    };

    FormDataBinding::new(segments.into_boxed_slice(), target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bsl_metadata::tabular_section::{TabularSection, TabularSectionAttribute};
    use bsl_metadata::{
        AttributeType, Configuration, Form, FormAttribute, FormElement, FormElementKind, FormType,
        MdoType, MetadataObject,
    };
    use hir_def::ty::FormDataKind;
    use std::sync::Arc;
    use uuid::Uuid;

    fn empty_form(name: &str) -> Form {
        Form::new(name.to_string(), FormType::Managed, Uuid::nil())
    }

    fn wrap_config(config: Configuration) -> Vec<VisibleConfig> {
        vec![VisibleConfig { name: None, configuration: Arc::new(config) }]
    }

    fn document_with_section(
        doc_name: &str,
        attrs: Vec<(&str, AttributeType)>,
        section_name: &str,
        section_attrs: Vec<(&str, AttributeType)>,
    ) -> MetadataObject {
        let mut doc = MetadataObject::new(MdoType::Document, doc_name);
        for (name, ty) in attrs {
            doc.add_attribute(bsl_metadata::Attribute {
                name: name.to_string(),
                name_en: None,
                attr_type: ty,
            });
        }
        let mut ts = TabularSection::new(Uuid::new_v4(), section_name);
        let cols: Vec<TabularSectionAttribute> = section_attrs
            .into_iter()
            .map(|(n, t)| TabularSectionAttribute::new(Uuid::new_v4(), n.to_string(), t))
            .collect();
        ts.set_attributes(cols);
        doc.add_tabular_section(ts);
        doc
    }

    #[test]
    fn lower_form_element_button_with_no_data_path_has_no_binding() {
        let form = empty_form("Ф");
        let element = FormElement::with_kind("Кнопка1", 1, None, FormElementKind::Button, None);
        let ty = lower_form_element(&form, &element, &[]);
        match ty {
            Ty::FormControl { kind, binding } => {
                assert_eq!(kind, FormElementKind::Button);
                assert!(binding.is_none(), "no DataPath ⇒ binding=None");
            }
            other => panic!("expected FormControl, got {other:?}"),
        }
    }

    #[test]
    fn lower_form_element_with_wrong_data_path_has_no_binding() {
        // `~`-prefix marks a deleted form attribute (the platform's own
        // convention). The control surface is still useful, but the
        // binding cannot be resolved.
        let form = empty_form("Ф");
        let element = FormElement::with_kind(
            "СломаннаяТаблица",
            1,
            Some("~Объект.Удалена".to_string()),
            FormElementKind::Table,
            None,
        );
        let ty = lower_form_element(&form, &element, &[]);
        match ty {
            Ty::FormControl { kind: FormElementKind::Table, binding } => {
                assert!(binding.is_none(), "~-prefixed DataPath ⇒ binding=None");
            }
            other => panic!("expected FormControl{{Table,None}}, got {other:?}"),
        }
    }

    #[test]
    fn lower_form_element_unknown_first_segment_yields_no_binding() {
        // DataPath references an attribute the form does not declare —
        // collapses to None rather than producing a half-baked binding.
        let form = empty_form("Ф");
        let element = FormElement::with_kind(
            "ЗабытоеПоле",
            1,
            Some("ОтсутствующийРеквизит.X".to_string()),
            FormElementKind::Field,
            None,
        );
        let ty = lower_form_element(&form, &element, &[]);
        match ty {
            Ty::FormControl { binding, .. } => assert!(binding.is_none()),
            other => panic!("expected FormControl, got {other:?}"),
        }
    }

    #[test]
    fn lower_form_element_with_scalar_attribute_yields_attribute_target() {
        // DataPath `Замечание` resolves to a string-typed scalar form
        // attribute. Phase 4 must record `Attribute{ ty: String }`
        // (no MetadataRef shape involved).
        let mut form = empty_form("Ф");
        form.attributes
            .push(FormAttribute::new("Замечание", AttributeType::String { length: Some(100) }));
        let element = FormElement::with_kind(
            "ПолеЗамечание",
            1,
            Some("Замечание".to_string()),
            FormElementKind::Field,
            None,
        );
        let ty = lower_form_element(&form, &element, &[]);
        match ty {
            Ty::FormControl { kind: FormElementKind::Field, binding: Some(b) } => {
                assert_eq!(b.path().len(), 1);
                assert_eq!(b.path()[0].as_str(), "Замечание");
                match b.target() {
                    FormDataTarget::Attribute { ty } => assert_eq!(**ty, Ty::String),
                    other => panic!("expected Attribute{{String}}, got {other:?}"),
                }
            }
            other => panic!("expected FormControl{{Field,Some(Attribute)}}, got {other:?}"),
        }
    }

    #[test]
    fn lower_form_element_with_tabular_section_path_yields_tabular_section_target() {
        // The headline scenario for Phase 4: `Объект.Переприемка` on a
        // managed-form module whose main attribute is a Document with
        // a tabular section. The binding records (Document, ПКО,
        // Переприемка) so Phase 5 can refine `.ВыделенныеСтроки` to
        // `TypedArray(row)` without re-walking the path.
        let mut form = empty_form("ФормаПКО");
        form.attributes.push(FormAttribute {
            name: "Объект".to_string(),
            attr_type: AttributeType::Ref {
                mdo_type: MdoType::Document, name: "ПКО".to_string()
            },
            is_main: true,
            columns: vec![],
        });

        let mut config = Configuration::new("Test");
        config.add_metadata_object(document_with_section(
            "ПКО",
            vec![],
            "Переприемка",
            vec![("ШтрихКод", AttributeType::String { length: Some(13) })],
        ));
        let configs = wrap_config(config);

        let element = FormElement::with_kind(
            "Переприемка",
            255,
            Some("Объект.Переприемка".to_string()),
            FormElementKind::Table,
            None,
        );
        let ty = lower_form_element(&form, &element, &configs);
        match ty {
            Ty::FormControl { kind: FormElementKind::Table, binding: Some(b) } => {
                assert_eq!(b.path().len(), 2);
                assert_eq!(b.path()[0].as_str(), "Объект");
                assert_eq!(b.path()[1].as_str(), "Переприемка");
                match b.target() {
                    FormDataTarget::TabularSection { mdo_type, owner, section } => {
                        assert_eq!(*mdo_type, MdoType::Document);
                        assert_eq!(owner.as_str(), "ПКО");
                        assert_eq!(section.as_str(), "Переприемка");
                    }
                    other => panic!("expected TabularSection target, got {other:?}"),
                }
            }
            other => panic!("expected FormControl{{Table,Some(TabularSection)}}, got {other:?}"),
        }
    }

    #[test]
    fn lower_form_element_carries_kind_for_other_buckets() {
        // A handful of non-Field/Table kinds — pin the kind passes
        // through the lowering so future refactors don't accidentally
        // drop the taxonomy.
        let form = empty_form("Ф");
        for k in [
            FormElementKind::Group,
            FormElementKind::UsualGroup,
            FormElementKind::Pages,
            FormElementKind::Page,
            FormElementKind::CommandBar,
            FormElementKind::ButtonGroup,
            FormElementKind::Decoration,
            FormElementKind::Addition,
            FormElementKind::Other,
        ] {
            let element = FormElement::with_kind("X", 1, None, k, None);
            match lower_form_element(&form, &element, &[]) {
                Ty::FormControl { kind, binding: None } => assert_eq!(kind, k),
                other => panic!("expected FormControl{{kind={k:?},None}}, got {other:?}"),
            }
        }
    }

    /// Regression for the Codex Phase 4 review finding: receiver
    /// comparison must fold Cyrillic case, not just ASCII. Pin the
    /// behaviour by walking through `Name::eq_ignore_case` directly —
    /// `lookup_form_item_field` is still gated on a Salsa db so we
    /// can't drive it end-to-end in a pure unit test, but the receiver
    /// comparator is the same `Name` API.
    #[test]
    fn cyrillic_case_insensitive_receiver_match() {
        let canonical = Name::new(FORM_ITEMS_TYPE_RU);
        let mixed = Name::new("вСеЭлементыФормы");
        let lower = Name::new("всеэлементыформы");
        let upper = Name::new("ВСЕЭЛЕМЕНТЫФОРМЫ");
        assert!(mixed.eq_ignore_case(&canonical));
        assert!(lower.eq_ignore_case(&canonical));
        assert!(upper.eq_ignore_case(&canonical));
        // Sanity: ASCII-only `eq_ignore_ascii_case` would NOT have
        // recognised these — keeps the regression honest.
        assert!(!"вСеЭлементыФормы".eq_ignore_ascii_case(FORM_ITEMS_TYPE_RU));

        // English alias also folds.
        let canonical_en = Name::new(FORM_ITEMS_TYPE_EN);
        let mixed_en = Name::new("formALLitems");
        assert!(mixed_en.eq_ignore_case(&canonical_en));
    }

    // ---- Phase 5: refine_form_control_property ----

    fn binding_to(mdo: MdoType, owner: &str, section: &str) -> FormDataBinding {
        FormDataBinding::new(
            Box::new([Name::new(owner), Name::new(section)]),
            FormDataTarget::TabularSection {
                mdo_type: mdo,
                owner: Name::new(owner),
                section: Name::new(section),
            },
        )
        .expect("non-empty path")
    }

    #[test]
    fn refine_selected_rows_returns_typed_array_of_row() {
        // Headline Phase 5 case: refined `.ВыделенныеСтроки` overrides
        // the platform's bare `Массив` with `TypedArray(row_ty)` so
        // iteration / `.Количество()` / indexing all carry the row Ty.
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        let info = refine_form_control_property(&receiver, &Name::new("ВыделенныеСтроки"))
            .expect("refined property");
        match info.ty {
            Ty::TypedArray(elem) => match *elem {
                Ty::MetadataRef {
                    kind: MetadataKind::TabularSectionRow { parent: MdoType::Document },
                    name,
                } => assert_eq!(name.as_str(), "ПКО.Переприемка"),
                other => panic!("expected row Ty inside TypedArray, got {other:?}"),
            },
            other => panic!("expected TypedArray(row), got {other:?}"),
        }
        assert!(info.is_readonly);
        assert_eq!(info.name.as_str(), "ВыделенныеСтроки");
        assert_eq!(info.name_en.as_ref().unwrap().as_str(), "SelectedRows");
    }

    #[test]
    fn refine_current_row_returns_row_ty() {
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Catalog, "Номенклатура", "ЕдиницыИзмерения")),
        };
        let info = refine_form_control_property(&receiver, &Name::new("ТекущаяСтрока"))
            .expect("refined ТекущаяСтрока");
        match info.ty {
            Ty::MetadataRef {
                kind: MetadataKind::TabularSectionRow { parent: MdoType::Catalog },
                name,
            } => assert_eq!(name.as_str(), "Номенклатура.ЕдиницыИзмерения"),
            other => panic!("expected row Ty, got {other:?}"),
        }
        assert_eq!(info.name_en.as_ref().unwrap().as_str(), "CurrentRow");
    }

    #[test]
    fn refine_current_data_returns_row_ty() {
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        let info = refine_form_control_property(&receiver, &Name::new("ТекущиеДанные"))
            .expect("refined ТекущиеДанные");
        assert!(matches!(
            info.ty,
            Ty::MetadataRef { kind: MetadataKind::TabularSectionRow { .. }, .. }
        ));
        assert_eq!(info.name_en.as_ref().unwrap().as_str(), "CurrentData");
    }

    #[test]
    fn refine_recognises_english_aliases() {
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        for english in ["SelectedRows", "CurrentRow", "CurrentData"] {
            assert!(
                refine_form_control_property(&receiver, &Name::new(english)).is_some(),
                "{english} must resolve via English alias"
            );
        }
    }

    #[test]
    fn refine_is_case_insensitive_cyrillic() {
        // Regression: the field name comparator must fold Cyrillic
        // case (mirrors the Phase 4 receiver fix).
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        for spelling in ["ВЫДЕЛЕННЫЕСТРОКИ", "выделенныестроки", "вЫдЕлЕнНыЕсТрОкИ"]
        {
            assert!(
                refine_form_control_property(&receiver, &Name::new(spelling)).is_some(),
                "spelling {spelling:?} must resolve"
            );
        }
    }

    #[test]
    fn refine_returns_none_for_non_refined_field() {
        // `.Видимость` / `.Заголовок` are NOT refined — they fall
        // through to the platform-property adapter (handled by
        // `field_lookup::lookup_field`'s catch-all). Refinement must
        // not steal them.
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        assert!(refine_form_control_property(&receiver, &Name::new("Видимость")).is_none());
        assert!(refine_form_control_property(&receiver, &Name::new("Заголовок")).is_none());
        assert!(refine_form_control_property(&receiver, &Name::new("ШтрихКод")).is_none());
    }

    #[test]
    fn refine_returns_none_when_kind_is_not_table() {
        // `.ВыделенныеСтроки` only makes sense on a Table control —
        // never on Field / Button / Group. Refinement must not fire.
        for kind in [
            FormElementKind::Field,
            FormElementKind::Button,
            FormElementKind::Group,
            FormElementKind::UsualGroup,
            FormElementKind::Pages,
            FormElementKind::Page,
            FormElementKind::CommandBar,
            FormElementKind::ButtonGroup,
            FormElementKind::Decoration,
            FormElementKind::Addition,
            FormElementKind::Other,
        ] {
            let receiver = Ty::FormControl {
                kind,
                binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
            };
            assert!(
                refine_form_control_property(&receiver, &Name::new("ВыделенныеСтроки")).is_none(),
                "kind {kind:?} must not refine .ВыделенныеСтроки"
            );
        }
    }

    #[test]
    fn refine_returns_none_when_binding_absent() {
        // No DataPath ⇒ no row schema ⇒ refinement degrades to platform
        // fallback (which keeps the bare `Массив`).
        let receiver = Ty::FormControl { kind: FormElementKind::Table, binding: None };
        assert!(refine_form_control_property(&receiver, &Name::new("ВыделенныеСтроки")).is_none());
    }

    /// Regression for the Codex Phase 5 review finding: `is_readonly`
    /// must be per-property, mirroring `platform_data.json`. Earlier
    /// version applied a blanket `true` and would have flagged
    /// legitimate `Элементы.Таблица.ТекущаяСтрока = НайденнаяСтрока`
    /// as a read-only-property assignment. Pin the platform-aligned
    /// shape: SelectedRows and CurrentData stay read-only, CurrentRow
    /// is writable.
    #[test]
    fn refine_is_readonly_matches_platform_per_property() {
        let receiver = Ty::FormControl {
            kind: FormElementKind::Table,
            binding: Some(binding_to(MdoType::Document, "ПКО", "Переприемка")),
        };
        let selected =
            refine_form_control_property(&receiver, &Name::new("ВыделенныеСтроки")).unwrap();
        assert!(selected.is_readonly, "ВыделенныеСтроки is platform-readonly");
        let selected_en =
            refine_form_control_property(&receiver, &Name::new("SelectedRows")).unwrap();
        assert!(selected_en.is_readonly);

        let current_data =
            refine_form_control_property(&receiver, &Name::new("ТекущиеДанные")).unwrap();
        assert!(current_data.is_readonly, "ТекущиеДанные is platform-readonly");
        let current_data_en =
            refine_form_control_property(&receiver, &Name::new("CurrentData")).unwrap();
        assert!(current_data_en.is_readonly);

        let current_row =
            refine_form_control_property(&receiver, &Name::new("ТекущаяСтрока")).unwrap();
        assert!(!current_row.is_readonly, "ТекущаяСтрока is writable per platform_data");
        let current_row_en =
            refine_form_control_property(&receiver, &Name::new("CurrentRow")).unwrap();
        assert!(!current_row_en.is_readonly);
    }

    #[test]
    fn refine_returns_none_when_target_is_attribute() {
        // `<Columns>`-backed form attributes lower to
        // `Attribute{FormData(Collection)}` (see Phase 4 scope note).
        // Until a dedicated `FormDataTarget::Columns` variant lands,
        // refinement must NOT fire on Attribute targets — the row Ty
        // would be wrong.
        let attr_binding = FormDataBinding::new(
            Box::new([Name::new("ТабличнаяЧасть")]),
            FormDataTarget::Attribute { ty: Box::new(Ty::ValueTable) },
        )
        .unwrap();
        let receiver =
            Ty::FormControl { kind: FormElementKind::Table, binding: Some(attr_binding) };
        assert!(refine_form_control_property(&receiver, &Name::new("ВыделенныеСтроки")).is_none());
    }

    #[test]
    fn lower_form_element_main_attribute_object_path_segment_resolves() {
        // Cross-check the bridge into `lower_form_attribute_to_ty`:
        // `Объект` typed as `cfg:DocumentObject.ПКО` lowers to
        // `Ty::FormData{Structure, Some((Document, "ПКО"))}`. Phase 4
        // needs that to project correctly when DataPath continues past
        // the main attribute. We exercise the simplest case here —
        // bare main attribute, no further segments — to pin
        // `target = Attribute{Ty::FormData{...}}` so Phase 5 has a
        // stable shape to refine on.
        let mut form = empty_form("Ф");
        form.attributes.push(FormAttribute {
            name: "Объект".to_string(),
            attr_type: AttributeType::Ref {
                mdo_type: MdoType::Document, name: "ПКО".to_string()
            },
            is_main: true,
            columns: vec![],
        });
        let element = FormElement::with_kind(
            "ПолеОбъекта",
            1,
            Some("Объект".to_string()),
            FormElementKind::Field,
            None,
        );
        let ty = lower_form_element(&form, &element, &[]);
        match ty {
            Ty::FormControl { kind: FormElementKind::Field, binding: Some(b) } => {
                assert_eq!(b.path().len(), 1);
                match b.target() {
                    FormDataTarget::Attribute { ty: inner } => match inner.as_ref() {
                        Ty::FormData {
                            kind: FormDataKind::Structure,
                            underlying: Some((mdo, name)),
                        } => {
                            assert_eq!(*mdo, MdoType::Document);
                            assert_eq!(name.as_str(), "ПКО");
                        }
                        other => panic!("expected FormData{{Structure}}, got {other:?}"),
                    },
                    other => panic!("expected Attribute target, got {other:?}"),
                }
            }
            other => panic!("expected FormControl{{Field,Some(Attribute)}}, got {other:?}"),
        }
    }
}
