use lsp_types::{ClientCapabilities, PositionEncodingKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
}

impl PositionEncoding {
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
