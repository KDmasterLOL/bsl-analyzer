//! LSP position encoding negotiation.

use lsp_types::{ClientCapabilities, PositionEncodingKind};

/// Encoding used for LSP `Position.character` and semantic-token columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
}

impl PositionEncoding {
    /// Prefer UTF-8 when the client explicitly supports it.
    ///
    /// Internally the parser and HIR ranges are UTF-8 byte offsets, and Neovim
    /// applies semantic token columns in byte space. Falling back to UTF-16
    /// preserves the LSP default for clients that do not advertise UTF-8.
    pub fn negotiate(capabilities: &ClientCapabilities) -> Self {
        let Some(general) = &capabilities.general else {
            return Self::Utf16;
        };
        let Some(encodings) = &general.position_encodings else {
            return Self::Utf16;
        };

        if encodings.contains(&PositionEncodingKind::UTF8) {
            Self::Utf8
        } else {
            Self::Utf16
        }
    }

    pub fn as_lsp_kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
        }
    }

    pub fn as_offset_encoding(self) -> &'static str {
        match self {
            Self::Utf8 => "utf-8",
            Self::Utf16 => "utf-16",
        }
    }
}
