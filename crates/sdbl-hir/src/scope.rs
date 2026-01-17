//! Name resolution scope for SDBL queries.
//!
//! Tracks available tables and their fields for column resolution.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::hir::{FieldDef, Name, TableRef};
use crate::types::SdblType;

/// Scope for name resolution in SDBL queries.
///
/// Maintains a stack of scopes for handling subqueries.
#[derive(Debug)]
pub struct Scope {
    /// Stack of scope frames (innermost last).
    frames: Vec<ScopeFrame>,

    /// Metadata provider for resolving references.
    /// Wrapped in Arc for cheap cloning.
    metadata: Option<Arc<bsl_metadata::Configuration>>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

/// Single scope frame.
#[derive(Debug, Default)]
struct ScopeFrame {
    /// Tables in this scope, keyed by effective name (alias or full name).
    tables: FxHashMap<String, TableRef>,

    /// Temporary tables created with INTO clause.
    /// Keyed by lowercase table name.
    temp_tables: FxHashMap<String, TempTableDef>,
}

/// Temporary table definition.
#[derive(Debug, Clone)]
pub struct TempTableDef {
    /// Table name.
    pub name: String,

    /// Fields from SELECT clause that created this table.
    pub fields: Vec<FieldDef>,
}

impl Scope {
    /// Create a new empty scope without metadata.
    ///
    /// For backwards compatibility. Prefer `new_with_metadata()` for full functionality.
    pub fn new() -> Self {
        Self { frames: vec![ScopeFrame::default()], metadata: None }
    }

    /// Create a new empty scope with metadata provider.
    ///
    /// Metadata is used for resolving nested field references through reference types.
    pub fn new_with_metadata(metadata: Option<Arc<bsl_metadata::Configuration>>) -> Self {
        Self { frames: vec![ScopeFrame::default()], metadata }
    }

    /// Push a new scope frame (for subqueries).
    pub fn push_frame(&mut self) {
        self.frames.push(ScopeFrame::default());
    }

    /// Pop the innermost scope frame.
    pub fn pop_frame(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Add a table to the current scope.
    pub fn add_table(&mut self, table: TableRef) {
        if let Some(frame) = self.frames.last_mut() {
            let key = table.effective_name().to_lowercase();
            frame.tables.insert(key, table);
        }
    }

    /// Add a temporary table to the current scope.
    ///
    /// Temporary tables are created with INTO clause and available
    /// in subsequent queries (e.g., UNION parts).
    pub fn add_temp_table(&mut self, name: String, fields: Vec<FieldDef>) {
        if let Some(frame) = self.frames.last_mut() {
            let key = name.to_lowercase();
            tracing::debug!(name = %name, fields = fields.len(), "Adding temporary table to scope");
            frame.temp_tables.insert(key, TempTableDef { name, fields });
        }
    }

    /// Find temporary table by name (case-insensitive).
    ///
    /// Searches from innermost to outermost scope.
    pub fn find_temp_table(&self, name: &str) -> Option<&TempTableDef> {
        let name_lower = name.to_lowercase();
        for frame in self.frames.iter().rev() {
            if let Some(temp_table) = frame.temp_tables.get(&name_lower) {
                tracing::debug!(name = %name, "Found temporary table in scope");
                return Some(temp_table);
            }
        }
        None
    }

    /// Find table by name (case-insensitive).
    ///
    /// Searches from innermost to outermost scope.
    pub fn find_table(&self, name: &str) -> Option<&TableRef> {
        let name_lower = name.to_lowercase();
        for frame in self.frames.iter().rev() {
            if let Some(table) = frame.tables.get(&name_lower) {
                return Some(table);
            }
        }
        None
    }

    /// Get all tables in current scope (not including parent scopes).
    pub fn current_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.frames.last().into_iter().flat_map(|f| f.tables.values())
    }

    /// Get all tables in scope (including parent scopes).
    pub fn all_tables(&self) -> impl Iterator<Item = &TableRef> {
        self.frames.iter().flat_map(|f| f.tables.values())
    }

    /// Resolve column type from scope.
    ///
    /// If `table_alias` is provided, looks up in that specific table.
    /// Otherwise, searches all tables in scope.
    ///
    /// Returns `SdblType::Unknown` if not found.
    pub fn resolve_column_type(&self, table_alias: Option<&str>, column_name: &str) -> SdblType {
        if let Some(alias) = table_alias {
            // Qualified column reference: Table.Column
            if let Some(table) = self.find_table(alias) {
                return self.find_column_type_in_table(table, column_name);
            }
            return SdblType::Unknown;
        }

        // Unqualified column reference: Column
        // Search all tables in scope
        let mut found_type: Option<SdblType> = None;

        for table in self.all_tables() {
            let ty = self.find_column_type_in_table(table, column_name);
            if !ty.is_unknown_or_error() {
                if found_type.is_some() {
                    // Ambiguous - column found in multiple tables
                    return SdblType::Error;
                }
                found_type = Some(ty);
            }
        }

        found_type.unwrap_or(SdblType::Unknown)
    }

    /// Find column in a specific table.
    fn find_column_type_in_table(&self, table: &TableRef, column_name: &str) -> SdblType {
        if let Some(ref resolved) = table.metadata {
            if let Some(field) = resolved.find_field(column_name) {
                return field.ty.clone();
            }
        }
        SdblType::Unknown
    }

    /// Find all tables that contain a given column (for error messages).
    pub fn find_tables_with_column(&self, column_name: &str) -> Vec<String> {
        let mut result = Vec::new();
        let column_lower = column_name.to_lowercase();

        for table in self.all_tables() {
            if let Some(ref resolved) = table.metadata {
                if resolved.fields().iter().any(|f| f.name.to_lowercase() == column_lower) {
                    result.push(table.effective_name().to_string());
                }
            }
        }

        result
    }

    /// Get field definition for a column.
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

        // Search all tables
        for table in self.all_tables() {
            if let Some(ref resolved) = table.metadata {
                if let Some(field) = resolved.find_field(column_name) {
                    return Some(field);
                }
            }
        }

        None
    }

    /// Check if a name is a known table alias in scope.
    pub fn is_table_alias(&self, name: &str) -> bool {
        self.find_table(name).is_some()
    }

    /// Get completion candidates for columns (for LSP).
    pub fn column_completions(&self, table_alias: Option<&str>) -> Vec<ColumnCompletion> {
        let mut result = Vec::new();

        let tables: Vec<&TableRef> = if let Some(alias) = table_alias {
            self.find_table(alias).into_iter().collect()
        } else {
            self.all_tables().collect()
        };

        for table in tables {
            // Use full_name (not effective_name which returns alias) for metadata lookup
            let table_name = &table.full_name;

            tracing::info!(
                full_name = %table_name,
                alias = ?table.alias,
                has_metadata = table.metadata.is_some(),
                fields_count = table.metadata.as_ref().map(|m| m.fields().len()).unwrap_or(0),
                "column_completions: Processing table"
            );

            if let Some(ref resolved) = table.metadata {
                // Log field details
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

    // ========================================
    // Nested field type resolution
    // ========================================

    /// Resolve type through chain of field references.
    ///
    /// Recursively resolves each field in the chain, following reference types.
    ///
    /// # Arguments
    ///
    /// * `table_alias` - Starting table alias
    /// * `field_chain` - Chain of field names to traverse (e.g., ["Владелец", "Родитель"])
    ///
    /// # Returns
    ///
    /// Final `SdblType` after traversing the chain, or `SdblType::Unknown` if any step fails.
    ///
    /// # Algorithm
    ///
    /// 1. Start with table from alias
    /// 2. For each field in chain:
    ///    a. Resolve field type in current context
    ///    b. If type is Ref, Composite, or DefinedType:
    ///       - Extract metadata reference(s)
    ///       - Update current context to referenced table
    ///         c. If type is primitive, stop (cannot traverse further)
    /// 3. Return final type
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Т.Владелец.Родитель
    /// // Step 1: Т (Справочник.Номенклатура)
    /// // Step 2: Владелец → Справочник.Контрагенты (Ref)
    /// // Step 3: Родитель → Справочник.Контрагенты (Ref)
    /// // Step 4: Ready to complete fields of Контрагенты
    /// ```
    pub fn resolve_nested_field_type(&self, table_alias: &str, field_chain: &[String]) -> SdblType {
        const MAX_DEPTH: usize = 10; // Protection from cycles

        if field_chain.len() > MAX_DEPTH {
            tracing::warn!(
                depth = field_chain.len(),
                "exceeded max nesting depth, possible circular reference"
            );
            return SdblType::Error;
        }

        // Find starting table
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

        // Traverse field chain
        for (i, field_name) in field_chain.iter().enumerate() {
            tracing::info!(
                step = i + 1,
                field = %field_name,
                available_fields = current_fields.len(),
                "resolving nested field"
            );

            // Find field in current context
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

            // Resolve next level based on type
            match &current_type {
                SdblType::Ref(mdo_ref) => {
                    // Follow reference to metadata object
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
                SdblType::Composite { types } => {
                    // Merge fields from all types
                    match self.resolve_composite_fields(types) {
                        Some(fields) => current_fields = fields,
                        None => {
                            tracing::debug!("failed to resolve composite fields");
                            return SdblType::Unknown;
                        }
                    }
                }
                SdblType::DefinedType { name, underlying_type } => {
                    // Unwrap DefinedType
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
                    // Follow tabular section reference to its attributes
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
                    // Cannot traverse AnyRef - unknown concrete type
                    tracing::debug!(
                        ty = ?current_type,
                        "reached AnyRef/AnyObjectRef, cannot traverse further"
                    );
                    return SdblType::Unknown;
                }
                _ => {
                    // Primitive type - cannot traverse further
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

    /// Resolve fields for a reference type.
    ///
    /// Loads metadata for the referenced object and returns its fields.
    fn resolve_ref_fields(&self, mdo_ref: &crate::types::MdoRef) -> Option<Vec<FieldDef>> {
        let config = self.metadata.as_ref()?;

        let mdo_object = config.find_metadata_object(mdo_ref.mdo_type, &mdo_ref.name)?;

        let mut fields = Vec::new();

        // Add standard Ссылка field (reference to self)
        fields.push(FieldDef::standard(
            "Ссылка",
            "Ref",
            SdblType::reference(mdo_ref.mdo_type, &mdo_ref.name),
        ));

        // Add standard ПометкаУдаления field
        fields.push(FieldDef::standard("ПометкаУдаления", "DeletionMark", SdblType::Boolean));

        // Add custom attributes (access public field directly)
        for attr in &mdo_object.attributes {
            fields.push(FieldDef::new_with_names(
                attr.name.clone(),
                attr.name_en.clone(),
                SdblType::from_attribute_type(&attr.attr_type),
                false, // is_standard
            ));
        }

        // Add tabular sections as fields with TabularSectionRef type
        for ts in &mdo_object.tabular_sections {
            fields.push(FieldDef::new_with_names(
                ts.name().to_string(),
                ts.name_en().map(|s| s.to_string()),
                SdblType::TabularSectionRef {
                    parent_mdo_type: mdo_ref.mdo_type,
                    parent_mdo_name: mdo_ref.name.clone(),
                    ts_name: ts.name().to_string(),
                },
                false, // is_standard
            ));
        }

        Some(fields)
    }

    /// Resolve fields from a tabular section.
    ///
    /// Returns attributes of the tabular section as fields.
    /// This is the single source of truth for tabular section field resolution.
    fn resolve_tabular_section_fields(
        &self,
        parent_mdo_type: bsl_metadata::MdoType,
        parent_mdo_name: &str,
        ts_name: &str,
    ) -> Option<Vec<FieldDef>> {
        let config = self.metadata.as_ref()?;

        let mdo_object = config.find_metadata_object(parent_mdo_type, parent_mdo_name)?;

        // Find tabular section by name (case-insensitive)
        let ts = mdo_object.find_tabular_section(ts_name)?;

        let mut fields = Vec::new();

        // Add tabular section attributes as fields
        // Single source of truth: use attr_type directly from metadata
        for attr in ts.attributes() {
            fields.push(FieldDef::new_with_names(
                attr.name().to_string(),
                attr.name_en().map(|s| s.to_string()),
                SdblType::from_attribute_type(attr.attr_type()),
                false, // not standard
            ));
        }

        Some(fields)
    }

    /// Merge fields from composite type.
    ///
    /// Returns union of all fields from all types, deduplicating by name.
    fn resolve_composite_fields(&self, types: &[SdblType]) -> Option<Vec<FieldDef>> {
        let mut all_fields = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        for ty in types {
            match ty {
                SdblType::Ref(mdo_ref) => {
                    if let Some(fields) = self.resolve_ref_fields(mdo_ref) {
                        for field in fields {
                            if seen_names.insert(field.name.to_lowercase()) {
                                all_fields.push(field);
                            }
                        }
                    }
                }
                SdblType::DefinedType { name, underlying_type } => {
                    if let Some(fields) = self.resolve_defined_type_fields(name, underlying_type) {
                        for field in fields {
                            if seen_names.insert(field.name.to_lowercase()) {
                                all_fields.push(field);
                            }
                        }
                    }
                }
                _ => {
                    // Ignore non-reference types in composite
                }
            }
        }

        if all_fields.is_empty() {
            None
        } else {
            Some(all_fields)
        }
    }

    /// Resolve fields for a DefinedType.
    ///
    /// Unwraps DefinedType to its underlying type and returns fields.
    fn resolve_defined_type_fields(
        &self,
        _name: &str,
        underlying_type: &Option<Box<SdblType>>,
    ) -> Option<Vec<FieldDef>> {
        let underlying = underlying_type.as_ref()?;

        match underlying.as_ref() {
            SdblType::Ref(mdo_ref) => self.resolve_ref_fields(mdo_ref),
            SdblType::Composite { types } => self.resolve_composite_fields(types),
            _ => None,
        }
    }

    /// Public API: Get fields for reference type.
    pub fn get_fields_for_ref(&self, mdo_ref: &crate::types::MdoRef) -> Vec<FieldDef> {
        self.resolve_ref_fields(mdo_ref).unwrap_or_default()
    }

    /// Public API: Get merged fields for composite type.
    pub fn get_fields_for_composite(&self, types: &[SdblType]) -> Vec<FieldDef> {
        self.resolve_composite_fields(types).unwrap_or_default()
    }

    /// Public API: Get fields for DefinedType.
    pub fn get_fields_for_defined_type(
        &self,
        name: &str,
        underlying_type: &Option<Box<SdblType>>,
    ) -> Vec<FieldDef> {
        self.resolve_defined_type_fields(name, underlying_type).unwrap_or_default()
    }
}

/// Column completion item.
#[derive(Debug, Clone)]
pub struct ColumnCompletion {
    /// Column name.
    pub column_name: Name,
    /// Table name (for disambiguation).
    pub table_name: Name,
    /// Column type.
    pub ty: SdblType,
    /// Is standard attribute.
    pub is_standard: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hir::ResolvedTable;
    use bsl_metadata::MdoType;
    use text_size::TextRange;

    fn make_table(name: &str, alias: Option<&str>, fields: Vec<FieldDef>) -> TableRef {
        TableRef {
            parts: vec![Name::from(name)],
            full_name: name.to_string(),
            alias: alias.map(Name::from),
            metadata: Some(ResolvedTable::Metadata {
                mdo_type: MdoType::Catalog,
                name: name.to_string(),
                fields,
            }),
            is_virtual_table: false,
            virtual_table_params: Vec::new(),
            subquery: None,
            range: TextRange::empty(0.into()),
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

        // Find by alias
        assert!(scope.find_table("в").is_some());
        assert!(scope.find_table("В").is_some());

        // Resolve column
        let ty = scope.resolve_column_type(Some("в"), "Код");
        assert_eq!(ty, SdblType::string());
    }

    #[test]
    fn test_scope_nested() {
        let mut scope = Scope::new();

        let outer_table =
            make_table("Outer", None, vec![FieldDef::new("Field1", SdblType::string())]);
        scope.add_table(outer_table);

        // Push subquery scope
        scope.push_frame();

        let inner_table =
            make_table("Inner", None, vec![FieldDef::new("Field2", SdblType::number())]);
        scope.add_table(inner_table);

        // Inner scope can see both tables
        assert!(scope.find_table("Inner").is_some());
        assert!(scope.find_table("Outer").is_some());

        // Pop scope
        scope.pop_frame();

        // Only outer table visible
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

        // Unique field resolves OK
        let ty = scope.resolve_column_type(None, "UniqueField");
        assert_eq!(ty, SdblType::string());

        // Shared field is ambiguous
        let ty = scope.resolve_column_type(None, "SharedField");
        assert_eq!(ty, SdblType::Error);
    }
}
