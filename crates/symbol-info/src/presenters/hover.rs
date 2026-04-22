//! Renders a [`SymbolSignature`] as Markdown suitable for an LSP hover.

use crate::domain::{Lang, MethodKind, SignatureSource, SymbolSignature, TypeRef};
use crate::presenters::signature_help::render_signature_help;

/// Render a hover markdown body for the given signature.
///
/// `lang` is reserved for future bilingual headers; only Russian is currently
/// emitted (matches existing UX).
pub fn render_hover_markdown(sig: &SymbolSignature, _lang: Lang) -> String {
    let mut out = String::new();

    out.push_str(&header_line(sig));
    out.push_str("\n\n");

    // Type owner (e.g. "**Тип:** Строка") for platform methods that have one.
    if let Some(qualifier) = sig.qualifier.as_deref() {
        let trimmed = qualifier.trim_end_matches('.');
        if !trimmed.is_empty()
            && matches!(sig.source, SignatureSource::Platform | SignatureSource::PlatformManager)
        {
            out.push_str(&format!("**Тип:** {}\n\n", trimmed));
        }
    }

    // Inline signature in a fenced bsl code block.
    let inline = render_signature_help(sig, 0).signature;
    out.push_str("**Синтаксис:**\n```bsl\n");
    out.push_str(&inline);
    out.push_str("\n```\n\n");

    if let Some(desc) = sig.description.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("**Описание:**\n");
        out.push_str(desc);
        out.push_str("\n\n");
    }

    if !sig.params.is_empty() {
        out.push_str("**Параметры:**\n");
        for p in &sig.params {
            out.push_str(&format!("- **{}**", p.name));
            if !p.types.is_empty() {
                out.push_str(&format!(": {}", join_types(&p.types)));
            }
            if let Some(d) = p.description.as_deref().filter(|s| !s.is_empty()) {
                out.push_str(&format!(" — {}", d));
            }
            out.push('\n');
        }
        out.push('\n');
    }

    if !sig.returns.is_empty() {
        out.push_str(&format!("**Возвращает:** {}\n\n", join_types(&sig.returns)));
    }

    if !sig.examples.is_empty() {
        out.push_str("**Примеры:**\n");
        for (i, ex) in sig.examples.iter().enumerate() {
            if let Some(d) = ex.description.as_deref().filter(|s| !s.is_empty()) {
                out.push_str(&format!("{}. {}\n", i + 1, d));
            }
            out.push_str("```bsl\n");
            out.push_str(&ex.code);
            out.push_str("\n```\n\n");
        }
    }

    if let Some(notes) = sig.notes.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("**Примечание:**\n");
        out.push_str(notes);
        out.push_str("\n\n");
    }

    if let Some(dep) = sig.deprecation.as_deref().filter(|s| !s.is_empty()) {
        out.push_str("**Устарела:**\n");
        out.push_str(dep);
        out.push_str("\n\n");
    }

    out.trim_end().to_string()
}

fn header_line(sig: &SymbolSignature) -> String {
    let bilingual = sig
        .name_english
        .as_ref()
        .map(|en| format!("{} / {}", sig.name_russian, en))
        .unwrap_or_else(|| sig.name_russian.to_string());

    let label = match sig.source {
        SignatureSource::Platform | SignatureSource::PlatformManager => "Метод",
        SignatureSource::GlobalFunction => "Глобальная функция",
        SignatureSource::PlatformConstructor => "Конструктор",
        SignatureSource::CommonModule | SignatureSource::ManagerModule | SignatureSource::Local => {
            match sig.kind {
                MethodKind::Function => "Функция",
                MethodKind::Procedure => "Процедура",
            }
        }
    };
    format!("**{}:** {}", label, bilingual)
}

fn join_types(types: &[TypeRef]) -> String {
    types.iter().map(|t| t.russian.as_str()).collect::<Vec<_>>().join(" | ")
}
