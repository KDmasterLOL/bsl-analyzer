use std::sync::Arc;
use stdx::case::CaseExt;

use bsl_metadata::QueryMetadataResolver;
use rustc_hash::FxHashMap;

use crate::hir::{FieldDef, Name, TableRef};
use crate::types::SdblType;

/// Where a [`Scope`] gets its metadata answers.
///
/// `Config` owns an `Arc<Configuration>` so a scope built for completion can be
/// returned by value (no borrow escapes). `Resolver` borrows a db-backed
/// per-MDO resolver used transiently during query lowering — that scope is never
/// returned, so the borrow stays local.
#[derive(Debug)]
enum MetaSource<'a> {
    Config(Arc<bsl_metadata::Configuration>),
    Resolver(&'a dyn QueryMetadataResolver),
}

impl MetaSource<'_> {
    fn resolver(&self) -> &dyn QueryMetadataResolver {
        match self {
            MetaSource::Config(config) => &**config,
            MetaSource::Resolver(resolver) => *resolver,
        }
    }
}

#[derive(Debug)]
pub struct Scope<'a> {
    frames: Vec<ScopeFrame>,

    metadata: Option<MetaSource<'a>>,
}

impl Default for Scope<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
struct ScopeFrame {
    tables: FxHashMap<String, TableRef>,

    temp_tables: FxHashMap<String, TempTableDef>,
}

#[derive(Debug, Clone)]
pub struct TempTableDef {
    pub name: String,

    pub fields: Vec<FieldDef>,
}

impl<'a> Scope<'a> {
    pub fn new() -> Self {
        Self { frames: vec![ScopeFrame::default()], metadata: None }
    }

    pub fn new_with_metadata(metadata: Option<Arc<bsl_metadata::Configuration>>) -> Self {
        Self { frames: vec![ScopeFrame::default()], metadata: metadata.map(MetaSource::Config) }
    }

    pub fn new_with_resolver(resolver: &'a dyn QueryMetadataResolver) -> Self {
        Self { frames: vec![ScopeFrame::default()], metadata: Some(MetaSource::Resolver(resolver)) }
    }

    pub fn push_frame(&mut self) {
        self.frames.push(ScopeFrame::default());
    }

    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    pub fn add_table(&mut self, table: TableRef) {
        if let Some(frame) = self.frames.last_mut() {
            let key = table.effective_name().fold_lower();
            frame.tables.insert(key, table);
        }
    }

    pub fn add_temp_table(&mut self, name: String, fields: Vec<FieldDef>) {
        if let Some(frame) = self.frames.last_mut() {
            let key = name.fold_lower();
            tracing::debug!(name = %name, fields = fields.len(), "Adding temporary table to scope");
            frame.temp_tables.insert(key, TempTableDef { name, fields });
        }
    }

    pub fn remove_temp_table(&mut self, name: &str) {
        let name_lower = name.fold_lower();
        for frame in self.frames.iter_mut().rev() {
            if frame.temp_tables.remove(&name_lower).is_some() {
                tracing::debug!(name = %name, "Removed temporary table from scope");
                return;
            }
        }
    }

    pub fn find_temp_table(&self, name: &str) -> Option<&TempTableDef> {
        let name_lower = name.fold_lower();
        for frame in self.frames.iter().rev() {
            if let Some(temp_table) = frame.temp_tables.get(&name_lower) {
                tracing::debug!(name = %name, "Found temporary table in scope");
                return Some(temp_table);
            }
        }
        None
    }

    pub fn find_table(&self, name: &str) -> Option<&TableRef> {
        let name_lower = name.fold_lower();
        for frame in self.frames.iter().rev() {
            if let Some(table) = frame.tables.get(&name_lower) {
                return Some(table);
            }
        }
        None
    }

    pub fn current_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.frames.last().into_iter().flat_map(|f| f.tables.values())
    }

    pub fn all_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.frames.iter().flat_map(|f| f.tables.values())
    }

    pub fn resolve_column_type(&self, table_alias: Option<&str>, column_name: &str) -> SdblType {
        if let Some(alias) = table_alias {
            if let Some(table) = self.find_table(alias) {
                return self.find_column_type_in_table(table, column_name);
            }
            return SdblType::Unknown;
        }

        let mut found_type: Option<SdblType> = None;

        for table in self.all_tables() {
            let ty = self.find_column_type_in_table(table, column_name);
            if !ty.is_unknown_or_error() {
                if found_type.is_some() {
                    return SdblType::Error;
                }
                found_type = Some(ty);
            }
        }

        found_type.unwrap_or(SdblType::Unknown)
    }

    fn find_column_type_in_table(&self, table: &TableRef, column_name: &str) -> SdblType {
        if let Some(ref resolved) = table.metadata {
            if let Some(field) = resolved.find_field(column_name) {
                return field.ty.clone();
            }
        }
        SdblType::Unknown
    }

    pub fn find_tables_with_column(&self, column_name: &str) -> Vec<String> {
        let mut result = Vec::new();

        for table in self.all_tables() {
            if let Some(ref resolved) = table.metadata {
                if resolved
                    .fields()
                    .iter()
                    .any(|f| stdx::case::eq_ignore_case(&f.name, column_name))
                {
                    result.push(table.effective_name().to_string());
                }
            }
        }

        result
    }

    pub fn find_field_def(
        &self,
        table_alias: Option<&str>,
        column_name: &str,
    ) -> Option<&FieldDef> {
        if let Some(alias) = table_alias {
            if let Some(table) = self.find_table(alias) {
                if let Some(ref resolved) = table.metadata {
                    return resolved.find_field(column_name);
                }
            }
            return None;
        }

        for table in self.all_tables() {
            if let Some(ref resolved) = table.metadata {
                if let Some(field) = resolved.find_field(column_name) {
                    return Some(field);
                }
            }
        }

        None
    }

    pub fn is_table_alias(&self, name: &str) -> bool {
        self.find_table(name).is_some()
    }

    pub fn column_completions(&self, table_alias: Option<&str>) -> Vec<ColumnCompletion> {
        let mut result = Vec::new();

        let tables: Vec<&TableRef> = if let Some(alias) = table_alias {
            self.find_table(alias).into_iter().collect()
        } else {
            self.all_tables().collect()
        };

        for table in tables {
            let table_name = &table.full_name;

            tracing::info!(
                full_name = %table_name,
                alias = ?table.alias,
                has_metadata = table.metadata.is_some(),
                fields_count = table.metadata.as_ref().map(|m| m.fields().len()).unwrap_or(0),
                "column_completions: Processing table"
            );

            if let Some(ref resolved) = table.metadata {
                tracing::info!(
                    full_name = %table_name,
                    total_fields = resolved.fields().len(),
                    field_names = ?resolved.fields().iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                    "column_completions: Fields in metadata"
                );

                for field in resolved.fields() {
                    result.push(ColumnCompletion {
                        column_name: Name::from(field.name.as_str()),
                        table_name: Name::from(table_name.as_str()),
                        ty: field.ty.clone(),
                        is_standard: field.is_standard,
                    });
                }
            } else {
                tracing::debug!(
                    full_name = %table_name,
                    "column_completions: Table has no resolved fields (may be from extension)"
                );
            }
        }

        result
    }

    pub fn resolve_nested_field_type(&self, table_alias: &str, field_chain: &[String]) -> SdblType {
        const MAX_DEPTH: usize = 10;

        if field_chain.len() > MAX_DEPTH {
            tracing::warn!(
                depth = field_chain.len(),
                "exceeded max nesting depth, possible circular reference"
            );
            return SdblType::Error;
        }

        let Some(table) = self.find_table(table_alias) else {
            tracing::warn!(alias = %table_alias, "table not found in scope");
            return SdblType::Unknown;
        };

        tracing::info!(alias = %table_alias, table_name = %table.full_name, "found table");

        let Some(metadata) = &table.metadata else {
            tracing::warn!(alias = %table_alias, "table has no metadata");
            return SdblType::Unknown;
        };

        let mut current_fields = metadata.fields().to_vec();
        let mut current_type = SdblType::Unknown;

        tracing::info!(
            alias = %table_alias,
            initial_fields_count = current_fields.len(),
            "starting field resolution"
        );

        for (i, field_name) in field_chain.iter().enumerate() {
            tracing::info!(
                step = i + 1,
                field = %field_name,
                available_fields = current_fields.len(),
                "resolving nested field"
            );

            let Some(field) = current_fields.iter().find(|f| f.matches_name(field_name)) else {
                tracing::warn!(
                    field = %field_name,
                    available_fields = ?current_fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
                    "field not found"
                );
                return SdblType::Unknown;
            };

            tracing::info!(
                field = %field_name,
                field_type = ?field.ty,
                "found field"
            );

            current_type = field.ty.clone();

            match &current_type {
                SdblType::Ref(mdo_ref) => {
                    tracing::info!(
                        mdo_ref = ?mdo_ref,
                        "following reference"
                    );
                    match self.resolve_ref_fields(mdo_ref) {
                        Some(fields) => {
                            tracing::info!(
                                fields_count = fields.len(),
                                "resolved reference fields"
                            );
                            current_fields = fields;
                        }
                        None => {
                            tracing::warn!(
                                mdo_ref = ?mdo_ref,
                                has_metadata = self.metadata.is_some(),
                                "failed to resolve reference fields"
                            );
                            return SdblType::Unknown;
                        }
                    }
                }
                SdblType::Composite { types } => match self.resolve_composite_fields(types) {
                    Some(fields) => current_fields = fields,
                    None => {
                        tracing::debug!("failed to resolve composite fields");
                        return SdblType::Unknown;
                    }
                },
                SdblType::DefinedType { name, underlying_type } => {
                    match self.resolve_defined_type_fields(name, underlying_type) {
                        Some(fields) => current_fields = fields,
                        None => {
                            tracing::debug!(
                                defined_type = %name,
                                "failed to resolve DefinedType fields"
                            );
                            return SdblType::Unknown;
                        }
                    }
                }
                SdblType::TabularSectionRef { parent_mdo_type, parent_mdo_name, ts_name } => {
                    tracing::info!(
                        parent = %parent_mdo_name,
                        ts_name = %ts_name,
                        "following tabular section reference"
                    );
                    match self.resolve_tabular_section_fields(
                        *parent_mdo_type,
                        parent_mdo_name,
                        ts_name,
                    ) {
                        Some(fields) => {
                            tracing::info!(
                                fields_count = fields.len(),
                                "resolved tabular section fields"
                            );
                            current_fields = fields;
                        }
                        None => {
                            tracing::warn!(
                                parent = %parent_mdo_name,
                                ts_name = %ts_name,
                                "failed to resolve tabular section fields"
                            );
                            return SdblType::Unknown;
                        }
                    }
                }
                SdblType::AnyRef | SdblType::AnyObjectRef { .. } => {
                    tracing::debug!(
                        ty = ?current_type,
                        "reached AnyRef/AnyObjectRef, cannot traverse further"
                    );
                    return SdblType::Unknown;
                }
                _ => {
                    tracing::debug!(
                        field = %field_name,
                        ty = ?current_type,
                        "reached primitive type, cannot traverse further"
                    );
                    return SdblType::Unknown;
                }
            }
        }

        current_type
    }

    fn resolve_ref_fields(&self, mdo_ref: &crate::types::MdoRef) -> Option<Vec<FieldDef>> {
        let resolver = self.metadata.as_ref()?.resolver();

        let mdo_object = resolver.resolve_metadata_object(mdo_ref.mdo_type, &mdo_ref.name)?;

        let mut fields = Vec::new();

        for attr in &mdo_object.attributes {
            let is_standard = bsl_metadata::is_standard_attribute_name(&attr.name);
            fields.push(FieldDef::new_with_names(
                attr.name.clone(),
                attr.name_en.clone(),
                SdblType::from_attribute_type(&attr.attr_type),
                is_standard,
            ));
        }

        for ts in &mdo_object.tabular_sections {
            fields.push(FieldDef::new_with_names(
                ts.name().to_string(),
                ts.name_en().map(|s| s.to_string()),
                SdblType::TabularSectionRef {
                    parent_mdo_type: mdo_ref.mdo_type,
                    parent_mdo_name: mdo_ref.name.clone(),
                    ts_name: ts.name().to_string(),
                },
                false,
            ));
        }

        Some(fields)
    }

    fn resolve_tabular_section_fields(
        &self,
        parent_mdo_type: bsl_metadata::MdoType,
        parent_mdo_name: &str,
        ts_name: &str,
    ) -> Option<Vec<FieldDef>> {
        let resolver = self.metadata.as_ref()?.resolver();

        let mdo_object = resolver.resolve_metadata_object(parent_mdo_type, parent_mdo_name)?;

        let ts = mdo_object.find_tabular_section(ts_name)?;

        let mut fields = Vec::new();

        fields.push(FieldDef::standard(
            "Ссылка",
            "Ref",
            SdblType::reference(parent_mdo_type, parent_mdo_name),
        ));

        fields.push(FieldDef::standard("НомерСтроки", "LineNumber", SdblType::number()));

        for attr in ts.attributes() {
            fields.push(FieldDef::new_with_names(
                attr.name().to_string(),
                attr.name_en().map(|s| s.to_string()),
                SdblType::from_attribute_type(attr.attr_type()),
                false,
            ));
        }

        Some(fields)
    }

    fn resolve_composite_fields(&self, types: &[SdblType]) -> Option<Vec<FieldDef>> {
        let mut all_fields = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for ty in types {
            match ty {
                SdblType::Ref(mdo_ref) => {
                    if let Some(fields) = self.resolve_ref_fields(mdo_ref) {
                        for field in fields {
                            if seen_names.insert(field.name.fold_lower()) {
                                all_fields.push(field);
                            }
                        }
                    }
                }
                SdblType::DefinedType { name, underlying_type } => {
                    if let Some(fields) = self.resolve_defined_type_fields(name, underlying_type) {
                        for field in fields {
                            if seen_names.insert(field.name.fold_lower()) {
                                all_fields.push(field);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if all_fields.is_empty() {
            None
        } else {
            Some(all_fields)
        }
    }

    fn resolve_defined_type_fields(
        &self,
        _name: &str,
        underlying_type: &Option<Box<SdblType>>,
    ) -> Option<Vec<FieldDef>> {
        let underlying = underlying_type.as_ref()?;

        match underlying.as_ref() {
            SdblType::Ref(mdo_ref) => self.resolve_ref_fields(mdo_ref),
            SdblType::Composite { types } => self.resolve_composite_fields(types),
            SdblType::DefinedType { name: inner_name, underlying_type: inner_underlying } => {
                self.resolve_defined_type_fields(inner_name, inner_underlying)
            }
            _ => None,
        }
    }

    pub fn get_fields_for_ref(&self, mdo_ref: &crate::types::MdoRef) -> Vec<FieldDef> {
        self.resolve_ref_fields(mdo_ref).unwrap_or_default()
    }

    pub fn get_fields_for_composite(&self, types: &[SdblType]) -> Vec<FieldDef> {
        self.resolve_composite_fields(types).unwrap_or_default()
    }

    pub fn get_fields_for_defined_type(
        &self,
        name: &str,
        underlying_type: &Option<Box<SdblType>>,
    ) -> Vec<FieldDef> {
        self.resolve_defined_type_fields(name, underlying_type).unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct ColumnCompletion {
    pub column_name: Name,
    pub table_name: Name,
    pub ty: SdblType,
    pub is_standard: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::ResolvedTable;
    use bsl_metadata::MdoType;

    fn make_table(name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: vec![Name::from(name)],
            full_name: name.to_string(),
            alias: alias.map(Name::from),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: name.to_string(),
                fields,
                field_model_complete: false,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: Vec::new(),
            range: syntax::MODULE_RANGE,
        }
    }

    #[test]
    fn test_scope_basic() {
        let mut scope = Scope::new();

        let table = make_table(
            "Справочник.Валюты",
            Some("В"),
            vec![
                FieldDef::standard(
                    "Ссылка",
                    "Ref",
                    SdblType::reference(MdoType::Catalog, "Валюты"),
                ),
                FieldDef::standard("Код", "Code", SdblType::string()),
            ],
        );

        scope.add_table(table);

        assert!(scope.find_table("в").is_some());
        assert!(scope.find_table("В").is_some());

        let ty = scope.resolve_column_type(Some("в"), "Код");
        assert_eq!(ty, SdblType::string());
    }

    #[test]
    fn test_scope_nested() {
        let mut scope = Scope::new();

        let outer_table =
            make_table("Outer", None, vec![FieldDef::new("Field1", SdblType::string())]);
        scope.add_table(outer_table);

        scope.push_frame();

        let inner_table =
            make_table("Inner", None, vec![FieldDef::new("Field2", SdblType::number())]);
        scope.add_table(inner_table);

        assert!(scope.find_table("Inner").is_some());
        assert!(scope.find_table("Outer").is_some());

        scope.pop_frame();

        assert!(scope.find_table("Outer").is_some());
        assert!(scope.find_table("Inner").is_none());
    }

    #[test]
    fn test_unqualified_column_resolution() {
        let mut scope = Scope::new();

        let table1 = make_table(
            "Table1",
            Some("T1"),
            vec![
                FieldDef::new("UniqueField", SdblType::string()),
                FieldDef::new("SharedField", SdblType::number()),
            ],
        );
        let table2 = make_table(
            "Table2",
            Some("T2"),
            vec![FieldDef::new("SharedField", SdblType::number())],
        );

        scope.add_table(table1);
        scope.add_table(table2);

        let ty = scope.resolve_column_type(None, "UniqueField");
        assert_eq!(ty, SdblType::string());

        let ty = scope.resolve_column_type(None, "SharedField");
        assert_eq!(ty, SdblType::Error);
    }

    #[test]
    fn defined_type_chained_underlying_unwraps_to_ref_fields() {
        use bsl_metadata::{Attribute, AttributeType, Configuration, MetadataObject};
        use std::sync::Arc;

        let mut config = Configuration::new("Test");
        let mut catalog = MetadataObject::new(MdoType::Catalog, "Валюты");
        catalog.add_attribute(Attribute {
            name: "Код".to_string(),
            name_en: Some("Code".to_string()),
            attr_type: AttributeType::String { length: Some(10) },
        });
        config.add_metadata_object(catalog);

        let scope = Scope::new_with_metadata(Some(Arc::new(config)));

        let inner = SdblType::DefinedType {
            name: "B".to_string(),
            underlying_type: Some(Box::new(SdblType::Ref(crate::types::MdoRef {
                mdo_type: MdoType::Catalog,
                name: "Валюты".to_string(),
            }))),
        };
        let outer_underlying = Some(Box::new(inner));

        let fields = scope.get_fields_for_defined_type("A", &outer_underlying);
        assert!(
            fields.iter().any(|f| f.name == "Код"),
            "chained DefinedType A → B → CatalogRef must surface `Код` from \
             the catalog underlying; got fields: {:?}",
            fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_no_duplicate_fields_when_metadata_has_standard_attributes() {
        use bsl_metadata::{Attribute, AttributeType, Configuration, MetadataObject};
        use std::sync::Arc;

        let mut config = Configuration::new("TestConfig");
        let mut obj = MetadataObject::new(MdoType::Catalog, "Валюты");

        obj.add_attribute(Attribute {
            name: "Ссылка".to_string(),
            name_en: Some("Ref".to_string()),
            attr_type: AttributeType::String { length: None },
        });
        obj.add_attribute(Attribute {
            name: "ПометкаУдаления".to_string(),
            name_en: Some("DeletionMark".to_string()),
            attr_type: AttributeType::Boolean,
        });
        obj.add_attribute(Attribute {
            name: "Код".to_string(),
            name_en: Some("Code".to_string()),
            attr_type: AttributeType::String { length: Some(10) },
        });
        obj.add_attribute(Attribute {
            name: "Наименование".to_string(),
            name_en: Some("Description".to_string()),
            attr_type: AttributeType::String { length: Some(100) },
        });
        obj.add_attribute(Attribute {
            name: "Курс".to_string(),
            name_en: Some("Rate".to_string()),
            attr_type: AttributeType::Number { precision: 15, scale: 4 },
        });

        config.add_metadata_object(obj);

        let scope = Scope::new_with_metadata(Some(Arc::new(config)));
        let mdo_ref =
            crate::types::MdoRef { mdo_type: MdoType::Catalog, name: "Валюты".to_string() };
        let fields = scope.get_fields_for_ref(&mdo_ref);

        let mut names: Vec<&str> = fields.iter().map(|f| f.name.as_str()).collect();
        names.sort_unstable();
        let original_len = names.len();
        names.dedup();
        assert_eq!(names.len(), original_len, "Duplicate fields found in resolve_ref_fields");

        let ref_field = fields.iter().find(|f| f.name == "Ссылка");
        assert!(ref_field.is_some(), "Ссылка field should be present");
        assert!(ref_field.unwrap().is_standard, "Ссылка should be marked as standard");

        let deletion_mark = fields.iter().find(|f| f.name == "ПометкаУдаления");
        assert!(deletion_mark.is_some(), "ПометкаУдаления field should be present");
        assert!(deletion_mark.unwrap().is_standard, "ПометкаУдаления should be marked as standard");

        let rate_field = fields.iter().find(|f| f.name == "Курс");
        assert!(rate_field.is_some(), "Курс field should be present");
        assert!(!rate_field.unwrap().is_standard, "Курс should not be marked as standard");
    }
}
