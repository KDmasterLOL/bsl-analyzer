mod config;
mod engine;
mod ir;
mod on_type;
mod whitespace;

#[cfg(test)]
mod tests;

pub use config::FormattingConfig;
pub use engine::{format_file, format_range, FormattingResult, TextEdit};
pub use on_type::on_char_typed;
