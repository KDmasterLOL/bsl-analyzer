//! SDBL HIR - semantic representation of SDBL queries.
//!
//! This crate provides High-level Intermediate Representation (HIR) for SDBL
//! (Structured Data Base Language) - the query language used in 1C:Enterprise.
//!
//! # Features
//!
//! - **Type inference**: Infer field types from 1C metadata
//! - **Name resolution**: Resolve tables to metadata objects, fields to definitions
//! - **Semantic diagnostics**: Collected during lowering (QueryToMissingMetadata, etc.)
//! - **LSP support**: Foundation for completion, hover, go-to-definition
//!
//! # Architecture
//!
//! ```text
//! SDBL AST (from parser)
//!     ↓
//! lower_sdbl_to_hir() with metadata context
//!     ↓
//! SdblHir (typed, resolved)
//!     ↓
//! Diagnostics + LSP features
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use sdbl_hir::{lower_sdbl_to_hir, SdblHir};
//!
//! let sdbl_hir = lower_sdbl_to_hir(&sdbl_ast, &metadata);
//!
//! // Check for semantic errors
//! for diag in &sdbl_hir.diagnostics {
//!     println!("Error: {:?}", diag);
//! }
//!
//! // Access typed fields
//! for field in &sdbl_hir.select.fields {
//!     println!("Field: {} (type: {:?})", field.alias_or_name(), field.ty);
//! }
//! ```

mod context_detector;
mod diagnostics;
mod hir;
mod literal;
mod lower;
mod position_detector;
mod scope;
mod source_map;
mod standard_fields;
mod types;

pub use context_detector::{detect_context, is_mdo_type, parse_nested_column_ref};
pub use diagnostics::{LikeUsageKind, SdblDiagnostic};
pub use hir::{
    ExprHir, FieldDef, FieldHir, FunctionKind, GroupByHir, InValues, JoinHir, JoinType, Name,
    OrderByHir, OrderByItem, ResolvedTable, SdblHir, SdblPackage, SdblQuery, SelectHir, TableRef,
    UnionHir, WhenClause,
};
pub use lower::{lower_sdbl_to_hir, SdblLowerResult};
pub use position_detector::detect_sdbl_at_position;
pub use scope::Scope;
pub use source_map::{SdblSourceMap, TokenCategory, TokenInfo};
pub use types::{MdoRef, SdblType};

use text_size::TextSize;

/// Information about SDBL query at a specific position.
///
/// Returned by `detect_sdbl_at_position()` when cursor is inside an SDBL query string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SdblQueryInfo {
    /// Full query text (including quotes if present)
    pub query_text: String,
    /// Offset within the query string (relative to query start, after opening quote)
    pub offset_in_query: TextSize,
    /// Text range of the BSL string literal containing this SDBL query
    pub bsl_literal_range: syntax::TextRange,
}

/// SDBL completion context - describes what kind of completion is appropriate at cursor position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SdblCompletionContext {
    /// Cursor is after FROM/ИЗ keyword - suggest MDO types (Справочник, Catalog, etc.)
    AfterFromKeyword,

    /// Cursor is inside MDO type reference - suggest MDO objects of that type
    /// Example: "Справочник.Вал$0" -> suggest all catalogs starting with "Вал"
    InsideMdoType {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after MDO object name - suggest nested elements (tabular sections, virtual tables)
    /// Example: "Справочник.Номенклатура.$0" -> suggest tabular sections
    /// Example: "РегистрСведений.МойРегистр.$0" -> suggest virtual tables
    AfterMdoObject {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Object name (e.g., "Номенклатура")
        object_name: String,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// General SDBL keywords - suggest SQL keywords (SELECT, WHERE, JOIN, etc.)
    /// Example: "ВЫБРАТЬ * $0" -> suggest FROM, WHERE, etc.
    SdblKeywords {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after table alias - suggest fields from that table
    /// Example: "ВЫБРАТЬ Т.$0" -> suggest fields from table with alias Т
    /// Example: "ВЫБРАТЬ Т.Код$0" -> suggest fields starting with "Код"
    AfterTableAlias {
        /// Table alias (e.g., "Т", "Т1", "Т2")
        alias: String,
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after AS/КАК keyword - suggest alias name
    /// Example: "ВЫБРАТЬ Т.Код КАК $0" -> suggest "Код" (field name)
    /// Example: "ИЗ Справочник.Номенклатура КАК $0" -> suggest "Номенклатура" (table name)
    AfterAsKeyword {
        /// Context where AS appears (SELECT field vs FROM/JOIN table)
        context: AsContext,
        /// Suggested alias name (extracted from preceding expression)
        suggestion: Option<String>,
    },

    /// Cursor is at position where JOIN type keyword should appear
    /// Example: "ИЗ Справочник.Валюты КАК Т Л$0" -> suggest "ЛЕВОЕ СОЕДИНЕНИЕ", "LEFT JOIN"
    JoinTypeKeyword {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor is after ON/ПО keyword - suggest table aliases
    /// Example: "ЛЕВОЕ СОЕДИНЕНИЕ ... ПО $0" -> suggest available aliases (Т, Т1, Т2)
    AfterOnKeyword {
        /// Prefix already typed (for filtering)
        prefix: String,
    },

    /// Cursor after nested field reference chain (3+ levels)
    /// Example: "ВЫБРАТЬ Т.Владелец.Родитель.$0" -> suggest fields from Родитель's type
    /// Example: "ВЫБРАТЬ Т.ВидНоменклатуры.$0" -> suggest fields from ВидНоменклатуры's underlying type
    AfterNestedField {
        /// Table alias (e.g., "Т")
        alias: String,
        /// Chain of field names already traversed (e.g., ["Владелец", "Родитель"])
        field_chain: Vec<String>,
        /// Prefix for filtering final field list
        prefix: String,
    },

    /// Cursor after CAST/ВЫРАЗИТЬ expression closing paren with field access
    /// Example: "ВЫРАЗИТЬ(Т.Регистратор КАК Документ.Продажа).$0" -> suggest fields of Документ.Продажа
    /// Example: "ВЫРАЗИТЬ(...).Контрагент.$0" -> suggest nested fields
    AfterCastExpression {
        /// MDO type from CAST (e.g., MdoType::Document)
        mdo_type: bsl_metadata::MdoType,
        /// MDO object name (e.g., "Продажа", "НачислениеИСписаниеБонусныхБаллов")
        object_name: String,
        /// Chain of field names after the cast (may be empty for direct access)
        field_chain: Vec<String>,
        /// Prefix for filtering
        prefix: String,
    },

    /// Cursor inside VALUE() function - suggest MDO types
    /// Example: "ЗНАЧЕНИЕ($0)" -> suggest Перечисление, Справочник, Документ
    InsideValueFunction,

    /// Cursor after MDO type in VALUE() - suggest MDO objects
    /// Example: "ЗНАЧЕНИЕ(Перечисление.$0)" -> suggest all enums
    InsideValueMdoType {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Prefix already typed
        prefix: String,
        /// True if Russian keywords used (Перечисление vs Enum)
        is_russian: bool,
    },

    /// Cursor after MDO object in VALUE() - suggest enum values or predefined items + EmptyRef
    /// Example: "ЗНАЧЕНИЕ(Перечисление.Статусы.$0)" -> suggest enum values + ПустаяСсылка
    /// Example: "ЗНАЧЕНИЕ(Справочник.Номенклатура.$0)" -> suggest predefined items + ПустаяСсылка
    InsideValueMdoObject {
        /// Metadata object type
        mdo_type: bsl_metadata::MdoType,
        /// Object name (e.g., "Статусы")
        object_name: String,
        /// Prefix already typed
        prefix: String,
        /// True if Russian keywords used (Перечисление vs Enum)
        is_russian: bool,
    },

    /// No specific completion context detected
    None,
}

/// Context for AS/КАК keyword - determines what kind of alias to suggest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AsContext {
    /// AS in SELECT clause - suggest field name from expression
    /// Example: "ВЫБРАТЬ Т.Код КАК |" -> suggest "Код"
    InSelectField,

    /// AS in FROM clause - suggest table name from MDO reference
    /// Example: "ИЗ Справочник.Номенклатура КАК |" -> suggest "Номенклатура"
    InFromClause,

    /// AS in JOIN clause - suggest table name from MDO reference
    /// Example: "ЛЕВОЕ СОЕДИНЕНИЕ Документ.Продажа КАК |" -> suggest "Продажа"
    InJoinClause,
}
