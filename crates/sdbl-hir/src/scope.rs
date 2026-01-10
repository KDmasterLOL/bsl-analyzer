//! Name resolution scope for SDBL queries.
//!
//! Tracks available tables and their fields for column resolution.

use rustc_hash::FxHashMap;

use crate::hir::{FieldDef, Name, TableRef};
use crate::types::SdblType;

/// Scope for name resolution in SDBL queries.
///
/// Maintains a stack of scopes for handling subqueries.
#[derive(Debug, Default)]
pub struct Scope {
    /// Stack of scope frames (innermost last).
    frames: Vec<ScopeFrame>,
}

/// Single scope frame.
#[derive(Debug, Default)]
struct ScopeFrame {
    /// Tables in this scope, keyed by effective name (alias or full name).
    tables: FxHashMap<String, TableRef>,
}

impl Scope {
    /// Create a new empty scope.
    pub fn new() -> Self {
        Self { frames: vec![ScopeFrame::default()] }
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
                tracing::warn!(
                    full_name = %table_name,
                    "column_completions: Table has no metadata!"
                );
            }
        }

        result
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
