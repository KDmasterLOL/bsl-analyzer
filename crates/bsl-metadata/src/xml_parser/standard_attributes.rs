//! Standard attributes handling for different MDO types.
//!
//! This module is a thin adapter: it reads the authoritative attribute spec
//! from `bsl_platform::standard_mdo_attributes` and instantiates concrete
//! `Attribute` / `RegisterAttribute` values by evaluating the presence
//! conditions against the MDO's configuration properties.

use bsl_platform::{
    standard_attributes_for, AttrValueKind, MdoTemplateKind, ObjectView, PresenceCondition,
    StandardAttrSpec, StandardKind,
};

use crate::metadata_object::{Attribute, AttributeType, MdoType};
use crate::register::RegisterAttribute;

use super::helpers::{
    child_bool, child_text, child_u32, create_register_standard_attribute, find_child,
};

// ============================================================================
// MdoProperties - extracted properties from MDO XML
// ============================================================================

/// Extracted properties from MDO XML, used for standard attribute generation
pub(crate) struct MdoProperties {
    pub name: String,
    pub code_length: Option<u32>,
    pub number_length: Option<u32>,
    pub description_length: Option<u32>,
    pub hierarchical: bool,
    pub owners: Vec<String>,
    pub periodicity: Option<String>,
    pub check_unique: bool,
    pub code_series: Option<String>,
}

impl MdoProperties {
    /// Extract properties from a `<Properties>` roxmltree node
    pub(crate) fn from_node(props_node: roxmltree::Node<'_, '_>) -> Self {
        let name = child_text(props_node, "Name").unwrap_or("").to_string();
        let code_length = child_u32(props_node, "CodeLength");
        let number_length = child_u32(props_node, "NumberLength");
        let description_length = child_u32(props_node, "DescriptionLength");
        let hierarchical = child_bool(props_node, "Hierarchical");
        let check_unique = child_bool(props_node, "CheckUnique");
        let periodicity =
            child_text(props_node, "InformationRegisterPeriodicity").map(|s| s.to_string());
        let code_series = child_text(props_node, "CodeSeries").map(|s| s.to_string());

        // Collect <Owners><Item>text</Item></Owners>
        let owners = find_child(props_node, "Owners")
            .map(|owners_node| {
                owners_node
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "Item")
                    .filter_map(|n| n.text())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        MdoProperties {
            name,
            code_length,
            number_length,
            description_length,
            hierarchical,
            owners,
            periodicity,
            check_unique,
            code_series,
        }
    }
}

// ============================================================================
// Generic adapter
// ============================================================================

/// Returns `true` if the presence condition is satisfied by `properties`.
fn condition_satisfied(cond: PresenceCondition, p: &MdoProperties) -> bool {
    match cond {
        PresenceCondition::Always => true,
        PresenceCondition::HasCode => p.code_length.is_some_and(|l| l > 0),
        PresenceCondition::HasDescription => p.description_length.is_some_and(|l| l > 0),
        PresenceCondition::HasNumber => p.number_length.is_some_and(|l| l > 0),
        PresenceCondition::Hierarchical => p.hierarchical,
        PresenceCondition::HasOwners => !p.owners.is_empty(),
        PresenceCondition::IsPeriodic => {
            p.periodicity.as_deref().unwrap_or("Nonperiodical") != "Nonperiodical"
        }
    }
}

/// Build an `AttributeType` from a spec entry and the MDO properties.
fn build_attr_type(spec: &StandardAttrSpec, p: &MdoProperties, mdo_type: MdoType) -> AttributeType {
    match spec.value {
        AttrValueKind::Boolean => AttributeType::Boolean,
        AttrValueKind::DateTime => AttributeType::DateTime,
        AttrValueKind::StringCodeOrDescription => {
            let length = match spec.kind {
                StandardKind::Code => p.code_length,
                StandardKind::Description => p.description_length,
                _ => None,
            };
            AttributeType::String { length }
        }
        AttrValueKind::StringNumber => AttributeType::String { length: p.number_length },
        AttrValueKind::StringUnbounded => AttributeType::String { length: None },
        AttrValueKind::NumberLineNumber => AttributeType::Number { precision: 10, scale: 0 },
        AttrValueKind::SelfRef => AttributeType::Ref { mdo_type, name: p.name.clone() },
        AttrValueKind::OwnerRef => owner_attr_type(&p.owners),
        AttrValueKind::AnyDocumentRef => {
            AttributeType::AnyObjectRef { mdo_type: MdoType::Document }
        }
        AttrValueKind::Unknown => AttributeType::Unknown,
    }
}

/// Build an `Attribute` from a spec entry.
fn build_attribute(spec: &StandardAttrSpec, p: &MdoProperties, mdo_type: MdoType) -> Attribute {
    Attribute {
        name: spec.kind.russian_name().to_string(),
        name_en: Some(spec.kind.english_name().to_string()),
        attr_type: build_attr_type(spec, p, mdo_type),
    }
}

/// Populate `attributes` from a platform spec, filtering by presence conditions.
fn add_standard_attributes_from_spec(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
    template: MdoTemplateKind,
    view: ObjectView,
) {
    for spec in standard_attributes_for(template, view) {
        if !condition_satisfied(spec.condition, properties) {
            continue;
        }
        attributes.push(build_attribute(spec, properties, mdo_type));
    }
}

// ============================================================================
// Register standard attributes (Vec<RegisterAttribute>)
// ============================================================================

/// Add common standard attributes for all register types (Active, LineNumber, Recorder)
pub(crate) fn add_register_common_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    mdo_type: MdoType,
) {
    use crate::metadata_object::StandardAttributeKind;
    // Active - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::Active,
        mdo_type,
        object_name,
    ));
    // LineNumber - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::LineNumber,
        mdo_type,
        object_name,
    ));
    // Recorder - always present
    attributes.push(create_register_standard_attribute(
        &StandardAttributeKind::Recorder,
        mdo_type,
        object_name,
    ));
}

/// Add Period attribute for registers (conditional based on periodicity)
pub(crate) fn add_register_period_attr(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    mdo_type: MdoType,
    periodicity: Option<&str>,
) {
    use crate::metadata_object::StandardAttributeKind;
    let should_add_period = match mdo_type {
        // InformationRegister: add Period only if not Nonperiodical
        MdoType::InformationRegister => periodicity.unwrap_or("Nonperiodical") != "Nonperiodical",
        // AccumulationRegister: Period is ALWAYS present
        MdoType::AccumulationRegister => true,
        // AccountingRegister and CalculationRegister: standard
        // attributes (including AcctReg's `Период`) come from the
        // platform composite-prefix `*Record.<Имя>` for `*Record`
        // receivers. Pushing them here would land them in
        // `register.attributes()`, which `enumerate_register_fields`
        // iterates BEFORE the platform-properties branch and so
        // shadows the richer platform metadata (notably
        // `is_readonly: true` on `НомерСтроки` per HBK). Until the
        // pre-existing InfoReg/AccumReg shadowing is reworked
        // wholesale, intentionally do nothing here — `*Record`
        // receivers still see the four common standards through the
        // platform branch. CalcReg has no plain `Период` at all
        // (`ПериодРегистрации` / `ПериодДействия*` / `БазовыйПериод*`
        // are distinct), so this also avoids fabricating a phantom
        // field on that flavour.
        MdoType::AccountingRegister | MdoType::CalculationRegister => false,
        // Other register types: no Period handling yet
        _ => false,
    };

    if should_add_period {
        attributes.push(create_register_standard_attribute(
            &StandardAttributeKind::Period,
            mdo_type,
            object_name,
        ));
    }
}

/// Add standard attributes for InformationRegister
pub(crate) fn add_information_register_standard_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
    periodicity: Option<&str>,
) {
    add_register_common_attrs(attributes, object_name, MdoType::InformationRegister);
    add_register_period_attr(attributes, object_name, MdoType::InformationRegister, periodicity);
}

/// Add standard attributes for AccumulationRegister
pub(crate) fn add_accumulation_register_standard_attrs(
    attributes: &mut Vec<RegisterAttribute>,
    object_name: &str,
) {
    add_register_common_attrs(attributes, object_name, MdoType::AccumulationRegister);
    // AccumulationRegister always has Period
    add_register_period_attr(attributes, object_name, MdoType::AccumulationRegister, None);
}

// AccountingRegister / CalculationRegister: deliberately no
// configuration-side standard-attribute connectors. Their `*Record`
// composite-prefix in HBK already declares the four common standards
// (`Активность`/`НомерСтроки`/`Период`/`Регистратор` for AcctReg;
// `Активность`/`НомерСтроки`/`Регистратор` for CalcReg — CalcReg has
// no plain `Период`), with richer attributes (`is_readonly`,
// documentation) than the symbolic `RegisterAttribute` shape we'd
// build here. Surfacing them through `register.attributes()` would
// cause `enumerate_register_fields` to iterate them BEFORE the
// platform-properties branch and shadow the platform-side info
// (notably flipping `НомерСтроки` from readonly to writable). The
// pre-existing InfoReg/AccumReg connectors do the same — addressing
// that shadowing wholesale is its own PR.

// ============================================================================
// Catalog/Document standard attributes (Vec<Attribute>) — wrapper API
// ============================================================================

/// Add standard attributes for Catalog objects
pub(crate) fn add_catalog_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::Catalog,
        ObjectView::Object,
    );
}

/// Add standard attributes for Document objects
pub(crate) fn add_document_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::Document,
        ObjectView::Object,
    );
}

/// Add standard attributes for BusinessProcess objects
pub(crate) fn add_business_process_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::BusinessProcess,
        ObjectView::Object,
    );
}

/// Add standard attributes for Task objects
pub(crate) fn add_task_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::Task,
        ObjectView::Object,
    );
}

/// Add standard attributes for ExchangePlan objects
pub(crate) fn add_exchange_plan_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::ExchangePlan,
        ObjectView::Object,
    );
}

/// Add standard attributes for ChartOfCharacteristicTypes objects
pub(crate) fn add_chart_of_characteristic_types_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::ChartOfCharacteristicTypes,
        ObjectView::Object,
    );
}

/// Add standard attributes for ChartOfAccounts objects
pub(crate) fn add_chart_of_accounts_standard_attributes(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::ChartOfAccounts,
        ObjectView::Object,
    );
}

/// Add standard attributes for InformationRegister (Attribute variant)
pub(crate) fn add_information_register_standard_attributes_as_attrs(
    attributes: &mut Vec<Attribute>,
    properties: &MdoProperties,
    mdo_type: MdoType,
) {
    add_standard_attributes_from_spec(
        attributes,
        properties,
        mdo_type,
        MdoTemplateKind::InformationRegister,
        ObjectView::Object,
    );
}

// ============================================================================
// Owner type helper
// ============================================================================

/// Parse owner types from owner string values
fn owner_attr_type(owners: &[String]) -> AttributeType {
    let types = parse_owner_types(owners);
    match types.len() {
        0 => AttributeType::Unknown,
        1 => types.into_iter().next().unwrap(),
        _ => AttributeType::Composite { types },
    }
}

fn parse_owner_types(items: &[String]) -> Vec<AttributeType> {
    items
        .iter()
        .filter_map(|item| {
            // Parse format: "Catalog.Контрагенты" or "ChartOfCharacteristicTypes.Свойства"
            let parts: Vec<&str> = item.split('.').collect();
            if parts.len() != 2 {
                tracing::warn!(value = %item, "Invalid owner format, expected Type.Name");
                return None;
            }

            let type_str = parts[0];
            let name = parts[1].to_string();

            match type_str.parse::<MdoType>() {
                Ok(mdo_type) => Some(AttributeType::Ref { mdo_type, name }),
                Err(e) => {
                    tracing::warn!(type_str = %type_str, error = %e, "Unknown MDO type in owner");
                    None
                }
            }
        })
        .collect()
}
