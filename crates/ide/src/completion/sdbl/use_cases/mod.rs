//! Use cases for SDBL completion.
//!
//! Each use case represents a specific completion scenario:
//! - complete_keywords: SDBL keywords (SELECT, FROM, WHERE, etc.)
//! - complete_fields: Table fields/columns
//! - complete_mdo: MDO types and objects
//! - complete_aliases: Table aliases
//! - complete_nested_fields: Nested field references (chains)
//! - complete_join_types: JOIN type keywords
//! - complete_nested_elements: Tabular sections and virtual tables

pub mod complete_aliases;
pub mod complete_fields;
pub mod complete_join_types;
pub mod complete_keywords;
pub mod complete_mdo;
pub mod complete_nested_elements;
pub mod complete_nested_fields;
pub mod complete_value_elements;

pub use complete_aliases::CompleteAliasesUseCase;
pub use complete_fields::CompleteFieldsUseCase;
pub use complete_join_types::CompleteJoinTypesUseCase;
pub use complete_keywords::CompleteKeywordsUseCase;
pub use complete_mdo::CompleteMdoUseCase;
pub use complete_nested_elements::CompleteNestedElementsUseCase;
pub use complete_nested_fields::CompleteNestedFieldsUseCase;
pub use complete_value_elements::CompleteValueElementsUseCase;
