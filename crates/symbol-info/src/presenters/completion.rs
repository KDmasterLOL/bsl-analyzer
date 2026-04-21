//! Renders a [`SymbolSignature`] into the parts needed for an LSP completion
//! item.
//!
//! The consumer (the IDE crate) wraps this in its `CompletionItem` shape,
//! adding `kind`, `sort_text`, and `source`.

use crate::domain::{SignatureSource, SymbolSignature};
use crate::presenters::signature_help::render_signature_help;

/// View-model for a completion item, populated from a [`SymbolSignature`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionDetail {
    /// Item label — Russian method/function name.
    pub label: String,
    /// One-line detail — the inline signature
    /// (`"Функция ВРег(Значение: Строка): Строка"`).
    pub detail: String,
    /// Long-form documentation (markdown).
    pub documentation: String,
    /// LSP `insertText` snippet.
    pub insert_text: String,
    /// Optional `filterText` (bilingual matching for platform symbols).
    pub filter_text: Option<String>,
}

/// Render a signature into completion-item view-model.
///
/// The completion `detail` line drops the qualifier that signature_help would
/// print, since the user has already typed the receiver before the cursor and
/// repeating it in the hint adds visual noise.
pub fn render_completion_detail(sig: &SymbolSignature) -> CompletionDetail {
    let label = sig.name_russian.to_string();
    let mut sig_for_inline = sig.clone();
    sig_for_inline.qualifier = None;
    let detail = render_signature_help(&sig_for_inline, 0).signature;

    let mut documentation = String::new();
    if let Some(en) = &sig.name_english {
        documentation.push_str(&format!("{} / {}\n\n", sig.name_russian, en));
    }
    if let Some(desc) = sig.description.as_deref().filter(|s| !s.is_empty()) {
        documentation.push_str(desc);
        documentation.push_str("\n\n");
    } else if let Some(purpose) = sig.purpose.as_deref().filter(|s| !s.is_empty()) {
        documentation.push_str(purpose);
        documentation.push_str("\n\n");
    }
    if !sig.params.is_empty() {
        documentation.push_str("Параметры:\n");
        for p in &sig.params {
            documentation.push_str(&format!("- {}", p.name));
            if !p.types.is_empty() {
                let joined: Vec<&str> = p.types.iter().map(|t| t.russian.as_str()).collect();
                documentation.push_str(&format!(": {}", joined.join(" | ")));
            }
            if let Some(d) = p.description.as_deref().filter(|s| !s.is_empty()) {
                documentation.push_str(&format!(" — {}", d));
            }
            documentation.push('\n');
        }
        documentation.push('\n');
    }
    if !sig.returns.is_empty() {
        let joined: Vec<&str> = sig.returns.iter().map(|t| t.russian.as_str()).collect();
        documentation.push_str(&format!("Возвращает: {}\n", joined.join(" | ")));
    }
    let documentation = documentation.trim_end().to_string();

    // Cursor lands inside the parens regardless of whether the method takes
    // arguments — keeps signatureHelp triggered on insertion and matches the
    // long-standing UX from the platform-completion path.
    let insert_text = format!("{}($0)", sig.name_russian);

    let filter_text = match sig.source {
        SignatureSource::Platform
        | SignatureSource::PlatformManager
        | SignatureSource::GlobalFunction => {
            sig.name_english.as_ref().map(|en| format!("{} {}", sig.name_russian, en))
        }
        _ => None,
    };

    CompletionDetail { label, detail, documentation, insert_text, filter_text }
}
