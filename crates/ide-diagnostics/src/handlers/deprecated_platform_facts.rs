use bsl_platform::deprecation::{
    self, CompatibilityBucket, DeprecationEntry, DisplayKind, ElementKind, LifecycleGroup, Lookup,
};
use hir::DeprecatedKind8312;
use stdx::case::CaseExt;

pub(crate) fn global_function_fact(
    name: &str,
    group: LifecycleGroup,
) -> Option<&'static DeprecationEntry> {
    let entry = deprecation::registry().lookup(Lookup::global_method(name))?;
    if entry.element_kind == ElementKind::GlobalMethod
        && entry.display == DisplayKind::Function
        && entry.group == group
    {
        Some(entry)
    } else {
        None
    }
}

pub(crate) fn managed_form_type_fact(name: &str) -> Option<&'static DeprecationEntry> {
    let entry = deprecation::registry().lookup(Lookup::type_(name))?;
    if entry.element_kind == ElementKind::Type
        && entry.display == DisplayKind::Type
        && entry.group == LifecycleGroup::ManagedForm
        && entry.compatibility == CompatibilityBucket::CompatibilityMode8_3_14
    {
        Some(entry)
    } else {
        None
    }
}

pub(crate) fn deprecated_method_fact(name: &str) -> Option<&'static DeprecationEntry> {
    let entry = deprecation::registry().lookup(Lookup::global_method(name))?;
    if entry.element_kind != ElementKind::GlobalMethod || entry.display != DisplayKind::Method {
        return None;
    }

    match entry.compatibility {
        CompatibilityBucket::CompatibilityMode8_3_10
        | CompatibilityBucket::CompatibilityMode8_3_17 => Some(entry),
        CompatibilityBucket::Any
        | CompatibilityBucket::CompatibilityMode8_3_6
        | CompatibilityBucket::CompatibilityMode8_3_12
        | CompatibilityBucket::CompatibilityMode8_3_14 => None,
    }
}

pub(crate) fn canonical_name_for(
    entry: &DeprecationEntry,
    input_name: &str,
) -> Option<&'static str> {
    let lower = input_name.fold_lower();
    if lower == entry.ru.fold_lower() {
        return Some(entry.ru);
    }
    if !entry.en.is_empty() && lower == entry.en.fold_lower() {
        return Some(entry.en);
    }
    None
}

pub(crate) fn replacement_for_name(
    entry: &DeprecationEntry,
    input_name: &str,
) -> Option<&'static str> {
    let replacement = entry.replacement?;
    let lower = input_name.fold_lower();
    if lower == entry.ru.fold_lower() {
        return Some(replacement.ru);
    }
    if !entry.en.is_empty() && lower == entry.en.fold_lower() {
        return Some(replacement.en);
    }
    None
}

pub(crate) fn is_russian_alias(entry: &DeprecationEntry, input_name: &str) -> bool {
    input_name.fold_lower() == entry.ru.fold_lower()
}

pub(crate) fn deprecated_8312_replacement(name: &str, kind: DeprecatedKind8312) -> &'static str {
    let lower = name.fold_lower();
    deprecation::registry()
        .entries()
        .iter()
        .find(|entry| is_8312_kind(entry, kind) && matches_entry_name(entry, &lower))
        .and_then(|entry| replacement_for_name(entry, name))
        .unwrap_or("")
}

fn is_8312_kind(entry: &DeprecationEntry, kind: DeprecatedKind8312) -> bool {
    if entry.compatibility != CompatibilityBucket::CompatibilityMode8_3_12 {
        return false;
    }

    match kind {
        DeprecatedKind8312::Attribute => {
            entry.element_kind == ElementKind::Attribute && entry.display == DisplayKind::Attribute
        }
        DeprecatedKind8312::Method => {
            entry.element_kind == ElementKind::Method && entry.display == DisplayKind::Method
        }
        DeprecatedKind8312::GlobalMethod => {
            entry.element_kind == ElementKind::GlobalMethod
                && entry.display == DisplayKind::GlobalMethod
        }
        DeprecatedKind8312::EnumName => {
            entry.element_kind == ElementKind::EnumName && entry.display == DisplayKind::EnumName
        }
        DeprecatedKind8312::EnumValue => {
            entry.element_kind == ElementKind::EnumValue && entry.display == DisplayKind::EnumValue
        }
    }
}

fn matches_entry_name(entry: &DeprecationEntry, lower_name: &str) -> bool {
    lower_name == entry.ru.fold_lower()
        || (!entry.en.is_empty() && lower_name == entry.en.fold_lower())
}
