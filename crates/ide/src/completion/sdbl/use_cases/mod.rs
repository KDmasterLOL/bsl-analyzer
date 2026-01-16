//! Use cases for SDBL completion.
//!
//! Each use case represents a specific completion scenario:
//! - complete_keywords: SDBL keywords (SELECT, FROM, WHERE, etc.)
//! - complete_fields: Table fields/columns
//! - complete_mdo: MDO types and objects
//! - complete_aliases: Table aliases

pub mod complete_aliases;
pub mod complete_fields;
pub mod complete_keywords;
pub mod complete_mdo;

pub use complete_aliases::CompleteAliasesUseCase;
pub use complete_fields::CompleteFieldsUseCase;
pub use complete_keywords::CompleteKeywordsUseCase;
pub use complete_mdo::CompleteMdoUseCase;
