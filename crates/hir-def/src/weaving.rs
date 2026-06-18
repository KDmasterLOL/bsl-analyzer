//! Pure recognition of configuration-extension method *weaving* annotations
//! (`&Вместо` / `&Перед` / `&После`, i.e. Around / Before / After) — Phase 3,
//! increment 1.
//!
//! Unlike `&ИзменениеИКонтроль` (text-splice merge, see [`crate::extension_merge`]),
//! weaving does not rewrite the base module text: an interceptor is a separate
//! method that wraps or replaces a base method `M`. This module only recognizes the
//! annotation and extracts the base method name it targets; the cross-module name
//! resolution and `ПродолжитьВызов` return typing that act on that target live in
//! later increments (`hir-ty`). Like the rest of the pure engine it is free of any
//! database / Salsa dependency so the recognition can be unit-tested in isolation.

use syntax::{SyntaxKind, SyntaxNode};

use crate::extension_merge::annotation_first_string_arg;

/// Interned identity of a *weaving* extension module: the (ext, base) file pair whose
/// own bodies are inferred with the base module as a same-module sibling fallback.
/// Unlike [`crate::effective_module::EffectiveModuleId`] there is no text splice — the
/// extension module keeps its own native text; only cross-module name resolution gains
/// the base fallback.
#[salsa::interned(debug)]
pub struct WeavingModuleId<'db> {
    pub ext_file: vfs::FileId,
    pub base_file: vfs::FileId,
}

/// Which weaving annotation an interceptor method carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterceptionKind {
    /// `&Вместо("M")` — replaces base `M`; its body may call the original via
    /// `ПродолжитьВызов(...)`.
    Around,
    /// `&Перед("M")` — runs before base `M`.
    Before,
    /// `&После("M")` — runs after base `M`.
    After,
}

/// A recognized weaving interceptor: the kind of interception and the base method
/// name it targets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interception {
    pub kind: InterceptionKind,
    /// Name of the base method this interceptor wraps or replaces.
    pub target: String,
}

/// Recognize a `&Вместо` / `&Перед` / `&После` interceptor method and extract the
/// base method name it targets. Returns `None` when the method carries none of the
/// three weaving annotations, or the annotation's first string argument is absent
/// (degrade to no-weave — never invent a target).
///
/// A method carrying more than one of these annotations is malformed BSL; the first
/// recognized kind (Around, then Before, then After) wins, which is sufficient for
/// the well-formed single-annotation shape this targets.
pub fn interceptor_target(method: &SyntaxNode) -> Option<Interception> {
    const KINDS: [(SyntaxKind, InterceptionKind); 3] = [
        (SyntaxKind::ANN_AROUND, InterceptionKind::Around),
        (SyntaxKind::ANN_BEFORE, InterceptionKind::Before),
        (SyntaxKind::ANN_AFTER, InterceptionKind::After),
    ];

    KINDS.into_iter().find_map(|(ann_kind, kind)| {
        annotation_first_string_arg(method, ann_kind).map(|target| Interception { kind, target })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn method_node(code: &str) -> SyntaxNode {
        let parse = parser::parse(code);
        parse
            .syntax_node()
            .children()
            .find(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
            .unwrap_or_else(|| panic!("no method node in: {code:?}"))
    }

    #[test]
    fn recognizes_around() {
        let m = method_node("&Вместо(\"М\")\nПроцедура Расш1_М()\nКонецПроцедуры");
        assert_eq!(
            interceptor_target(&m),
            Some(Interception { kind: InterceptionKind::Around, target: "М".into() })
        );
    }

    #[test]
    fn recognizes_before() {
        let m =
            method_node("&Перед(\"ПриЗаписи\")\nПроцедура Расш1_ПриЗаписи(Отказ)\nКонецПроцедуры");
        assert_eq!(
            interceptor_target(&m),
            Some(Interception {
                kind: InterceptionKind::Before, target: "ПриЗаписи".into()
            })
        );
    }

    #[test]
    fn recognizes_after() {
        let m = method_node("&После(\"Функция1\")\nФункция Расш1_Функция1()\nКонецФункции");
        assert_eq!(
            interceptor_target(&m),
            Some(Interception { kind: InterceptionKind::After, target: "Функция1".into() })
        );
    }

    #[test]
    fn recognizes_english_instead() {
        // 1C's English form of `&Вместо` is `&Instead`.
        let m = method_node("&Instead(\"M\")\nProcedure Ext1_M()\nEndProcedure");
        assert_eq!(
            interceptor_target(&m),
            Some(Interception { kind: InterceptionKind::Around, target: "M".into() })
        );
    }

    #[test]
    fn plain_method_is_not_an_interceptor() {
        let m = method_node("Процедура Обычная()\nКонецПроцедуры");
        assert_eq!(interceptor_target(&m), None);
    }

    #[test]
    fn change_and_validate_is_not_a_weaving_interceptor() {
        // `&ИзменениеИКонтроль` is handled by the text-splice merge, not weaving.
        let m = method_node("&ИзменениеИКонтроль(\"М\")\nПроцедура Расш1_М()\nКонецПроцедуры");
        assert_eq!(interceptor_target(&m), None);
    }

    #[test]
    fn nested_annotation_arg_is_rejected() {
        // The target string lives inside a nested annotation, not as the direct first
        // string arg → no direct STRING token → degrade to no-weave.
        let m = method_node("&Вместо(&Ann(\"М\"))\nПроцедура Расш1_М()\nКонецПроцедуры");
        assert_eq!(interceptor_target(&m), None);
    }

    #[test]
    fn missing_target_string_is_rejected() {
        let m = method_node("&Вместо()\nПроцедура Расш1_М()\nКонецПроцедуры");
        assert_eq!(interceptor_target(&m), None);
    }
}
