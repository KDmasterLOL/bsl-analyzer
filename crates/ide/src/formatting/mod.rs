//! BSL code formatting.
//!
//! This module implements code formatting for BSL language:
//! - Full document formatting (`textDocument/formatting`)
//! - Range formatting (`textDocument/rangeFormatting`)
//! - On-type formatting (`textDocument/onTypeFormatting`)
//!
//! ## Architecture
//!
//! The formatting engine is AST-based (using Rowan CST) rather than regex-based.
//! It produces minimal TextEdits by computing diffs between original and formatted text.
//!
//! ## Algorithm
//!
//! Based on RDT1C indentation algorithm adapted for AST traversal:
//! - Track `base_indent` (from first line)
//! - Track `instruction_level` (depth of block constructs)
//! - Track `continuation_level` (for multi-line expressions)
//! - Compute indent for each line as: `base + instruction + continuation`

mod config;
mod engine;
mod indent;
mod on_type;
mod whitespace;

#[cfg(test)]
mod tests;

pub use config::FormattingConfig;
pub use engine::{format_file, format_range, FormattingResult, TextEdit};
pub use on_type::on_char_typed;
