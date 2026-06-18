//! Pure engine for merging a configuration extension's `&ИзменениеИКонтроль`
//! method onto its base module (Phase 2, increment 1).
//!
//! These functions are deliberately free of any database / Salsa dependency so
//! the merge semantics can be unit-tested in isolation. The Salsa wiring that
//! produces the effective module and routes diagnostics lives in later
//! increments (see `.omc/plans/extension-merge-phase2.md`).

use std::path::{Path, PathBuf};

use syntax::{
    ast::{self, AstNode},
    SyntaxKind, SyntaxNode, TextRange, TextSize,
};

/// Map an extension module file path to its candidate base-config module path.
///
/// `roots` mirrors `all_config_paths`: the single `None`-labelled entry is the
/// base configuration root; every `Some(name)` entry is an extension root.
/// Pairing is purely path-structural — the extension mirrors the base metadata
/// layout, so stripping the extension root and rebasing onto the base root yields
/// the base module path. It works for any module kind (object / manager /
/// recordset / form); whether the resulting base file actually exists is the
/// caller's concern (an extension's *own* object has no base counterpart).
///
/// Returns `None` when `ext_path` is not under any extension root (e.g. it is
/// itself a base-config file) or when no base root is registered.
pub fn pair_base_module_path(
    roots: &[(Option<String>, PathBuf)],
    ext_path: &Path,
) -> Option<PathBuf> {
    let base_root = roots.iter().find_map(|(label, p)| label.is_none().then_some(p))?;

    let ext_root = roots
        .iter()
        .filter_map(|(label, p)| label.as_ref().map(|_| p))
        .filter(|p| ext_path.starts_with(p))
        .max_by_key(|p| p.components().count())?;

    let rel = ext_path.strip_prefix(ext_root).ok()?;
    Some(base_root.join(rel))
}

/// The result of recognizing an `&ИзменениеИКонтроль("M")` method: the base
/// method name it modifies, plus a verification that its insert/delete markers
/// are well-formed. Returned only for the supported v1 shape — malformed or
/// unsupported directive usage degrades to `None` (no merge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeControl {
    /// Name of the base method this extension method replaces.
    pub target: String,
}

/// Where a span of the directive-stripped effective body came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Copied verbatim from the base method (outside any `#Вставка`). Diagnostics
    /// here are suppressed on the extension file (not author-written deltas).
    Copied,
    /// Inside a `#Вставка` block — genuinely extension-authored code. Diagnostics
    /// here are published, remapped to `ext`.
    Inserted,
}

/// A contiguous run of the effective body, mapping its offset back to the source
/// range it was copied from in the extension file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Range within the produced effective-body string.
    pub effective: TextRange,
    /// Corresponding range in the extension module source.
    pub ext: TextRange,
    pub origin: Origin,
}

/// Recognize an `&ИзменениеИКонтроль("M")` method and validate its directive
/// markers. Returns `None` when the method is not change-and-validate, the target
/// name is absent, or the `#Вставка`/`#Удаление` markers are unbalanced/nested
/// (degrade to no-merge — never produce a wrong effective module).
pub fn extract_change_and_validate(method: &SyntaxNode) -> Option<ChangeControl> {
    let target = change_and_validate_target(method)?;
    let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST)?;
    markers_balanced(&body).then_some(ChangeControl { target })
}

fn change_and_validate_target(method: &SyntaxNode) -> Option<String> {
    let annotation = method.children().filter_map(ast::Annotation::cast).find(|ann| {
        ann.kind_token().map(|t| t.kind()) == Some(SyntaxKind::ANN_CHANGE_AND_VALIDATE)
    })?;

    // The target must be the first parameter's own string literal. Search only
    // the parameter's DIRECT tokens — a nested annotation value (e.g.
    // `&ИзменениеИКонтроль(&Foo("M"))`) keeps its string deeper, so it yields no
    // direct STRING and is correctly rejected (degrade to no-merge).
    let first_param = annotation
        .syntax()
        .children()
        .find(|n| n.kind() == SyntaxKind::ANNOTATION_PARAMS)?
        .children()
        .find(|n| n.kind() == SyntaxKind::ANNOTATION_PARAM)?;

    let string = first_param
        .children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::STRING)?;

    Some(unquote_bsl_string(string.text()))
}

fn unquote_bsl_string(raw: &str) -> String {
    raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(raw).replace("\"\"", "\"")
}

/// A `#Вставка`/`#Удаление` marker is open exactly once and closed by its own end
/// marker before any other marker opens; the two kinds never nest or overlap.
fn markers_balanced(body: &SyntaxNode) -> bool {
    #[derive(PartialEq)]
    enum State {
        Outside,
        Insert,
        Delete,
    }
    let mut state = State::Outside;
    for token in body.descendants_with_tokens().filter_map(|el| el.into_token()) {
        state = match (token.kind(), &state) {
            (SyntaxKind::PRE_INSERT, State::Outside) => State::Insert,
            (SyntaxKind::PRE_END_INSERT, State::Insert) => State::Outside,
            (SyntaxKind::PRE_DELETE, State::Outside) => State::Delete,
            (SyntaxKind::PRE_END_DELETE, State::Delete) => State::Outside,
            (
                SyntaxKind::PRE_INSERT
                | SyntaxKind::PRE_END_INSERT
                | SyntaxKind::PRE_DELETE
                | SyntaxKind::PRE_END_DELETE,
                _,
            ) => return false,
            _ => continue,
        };
    }
    state == State::Outside
}

/// Produce the directive-stripped effective body of an `&ИзменениеИКонтроль`
/// method body (`STMT_LIST`): `#Удаление` blocks are dropped entirely, `#Вставка`
/// markers are removed keeping their content, everything else is copied verbatim.
/// Each output run is tagged with its [`Origin`] and the extension-source range it
/// came from, so diagnostics computed on the effective text can be remapped to the
/// extension file (and base-copied runs suppressed).
///
/// Assumes balanced markers (validate with [`extract_change_and_validate`] first).
pub fn strip_directives(body: &SyntaxNode) -> (String, Vec<Segment>) {
    enum Mode {
        Copy,
        Drop,
        Insert,
    }
    let mut mode = Mode::Copy;
    let mut text = String::new();
    let mut segments: Vec<Segment> = Vec::new();

    for token in body.descendants_with_tokens().filter_map(|el| el.into_token()) {
        match token.kind() {
            SyntaxKind::PRE_DELETE => mode = Mode::Drop,
            SyntaxKind::PRE_END_DELETE => mode = Mode::Copy,
            SyntaxKind::PRE_INSERT => mode = Mode::Insert,
            SyntaxKind::PRE_END_INSERT => mode = Mode::Copy,
            _ => {
                let origin = match mode {
                    Mode::Drop => continue,
                    Mode::Insert => Origin::Inserted,
                    Mode::Copy => Origin::Copied,
                };
                emit(&mut text, &mut segments, token.text(), token.text_range(), origin);
            }
        }
    }

    (text, segments)
}

fn emit(
    text: &mut String,
    segments: &mut Vec<Segment>,
    piece: &str,
    ext: TextRange,
    origin: Origin,
) {
    let start = TextSize::new(text.len() as u32);
    text.push_str(piece);
    let effective = TextRange::new(start, TextSize::new(text.len() as u32));

    // Extend the previous run only when it is the same origin AND contiguous in the
    // extension source (a dropped `#Удаление` block breaks ext-contiguity, forcing a
    // new segment so offset remapping stays correct).
    if let Some(last) = segments.last_mut() {
        if last.origin == origin && last.ext.end() == ext.start() {
            last.effective = TextRange::new(last.effective.start(), effective.end());
            last.ext = TextRange::new(last.ext.start(), ext.end());
            return;
        }
    }
    segments.push(Segment { effective, ext, origin });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first_method(code: &str) -> SyntaxNode {
        parser::parse(code)
            .syntax_node()
            .descendants()
            .find(|n| matches!(n.kind(), SyntaxKind::PROCEDURE_DEF | SyntaxKind::FUNCTION_DEF))
            .expect("a method")
    }

    fn roots() -> Vec<(Option<String>, PathBuf)> {
        vec![
            (None, PathBuf::from("/proj/src")),
            (Some("MyExt".into()), PathBuf::from("/proj/src/cfe/MyExt")),
            (Some("Other".into()), PathBuf::from("/proj/src/cfe/Other")),
        ]
    }

    #[test]
    fn borrowed_object_module_maps_to_base() {
        let ext = Path::new("/proj/src/cfe/MyExt/Catalogs/Товары/Ext/ObjectModule.bsl");
        assert_eq!(
            pair_base_module_path(&roots(), ext),
            Some(PathBuf::from("/proj/src/Catalogs/Товары/Ext/ObjectModule.bsl")),
        );
    }

    #[test]
    fn form_module_maps_to_base() {
        let ext = Path::new(
            "/proj/src/cfe/MyExt/Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl",
        );
        assert_eq!(
            pair_base_module_path(&roots(), ext),
            Some(PathBuf::from(
                "/proj/src/Catalogs/Товары/Forms/ФормаЭлемента/Ext/Form/Module.bsl"
            )),
        );
    }

    #[test]
    fn base_file_is_not_paired() {
        let base = Path::new("/proj/src/Catalogs/Товары/Ext/ObjectModule.bsl");
        assert_eq!(pair_base_module_path(&roots(), base), None);
    }

    #[test]
    fn own_extension_object_still_returns_candidate_path() {
        // An extension's own object has no base file on disk, but pairing is
        // path-only — the caller resolves existence and degrades to no-merge.
        let ext = Path::new("/proj/src/cfe/MyExt/Catalogs/СобственныйСпр/Ext/ObjectModule.bsl");
        assert_eq!(
            pair_base_module_path(&roots(), ext),
            Some(PathBuf::from("/proj/src/Catalogs/СобственныйСпр/Ext/ObjectModule.bsl")),
        );
    }

    #[test]
    fn longest_extension_prefix_wins() {
        // A nested extension root must take precedence over a shorter one.
        let nested = vec![
            (None, PathBuf::from("/proj/src")),
            (Some("Outer".into()), PathBuf::from("/proj/src/cfe")),
            (Some("Inner".into()), PathBuf::from("/proj/src/cfe/Inner")),
        ];
        let ext = Path::new("/proj/src/cfe/Inner/Catalogs/Товары/Ext/ObjectModule.bsl");
        assert_eq!(
            pair_base_module_path(&nested, ext),
            Some(PathBuf::from("/proj/src/Catalogs/Товары/Ext/ObjectModule.bsl")),
        );
    }

    #[test]
    fn no_base_root_yields_none() {
        let only_ext = vec![(Some("MyExt".into()), PathBuf::from("/proj/src/cfe/MyExt"))];
        let ext = Path::new("/proj/src/cfe/MyExt/Catalogs/Товары/Ext/ObjectModule.bsl");
        assert_eq!(pair_base_module_path(&only_ext, ext), None);
    }

    // §30.4.2.2.5 dev-guide example (8.3.27): delete the buggy `+` line, insert the
    // fixed `*` line, both inside a loop.
    const CHANGE_CONTROL_EXAMPLE: &str = "\
&ИзменениеИКонтроль(\"ВозведениеВСтепень\")
Функция Расш1_ВозведениеВСтепень(Основание, Степень)
\tРезультат = 1;
\tДля Индекс = 1 По Степень Цикл
#Удаление
\t\tРезультат = Результат + Основание;
#КонецУдаления
#Вставка
\t\tРезультат = Результат * Основание;
#КонецВставки
\tКонецЦикла;
\tВозврат Результат;
КонецФункции";

    #[test]
    fn extracts_change_and_validate_target() {
        let cc = extract_change_and_validate(&first_method(CHANGE_CONTROL_EXAMPLE));
        assert_eq!(cc, Some(ChangeControl { target: "ВозведениеВСтепень".into() }));
    }

    #[test]
    fn non_change_and_validate_method_is_ignored() {
        let code = "&НаКлиенте\nПроцедура Тест()\nКонецПроцедуры";
        assert_eq!(extract_change_and_validate(&first_method(code)), None);
    }

    #[test]
    fn unbalanced_markers_reject() {
        let code = "&ИзменениеИКонтроль(\"M\")\n\
Процедура Расш1_M()\n\
#Вставка\n\
А = 1;\n\
КонецПроцедуры";
        assert_eq!(extract_change_and_validate(&first_method(code)), None);
    }

    #[test]
    fn strip_drops_deletion_keeps_insertion() {
        let method = first_method(CHANGE_CONTROL_EXAMPLE);
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, segments) = strip_directives(&body);

        assert!(text.contains("Результат = Результат * Основание;"), "insertion kept:\n{text}");
        assert!(!text.contains("+ Основание"), "deletion dropped:\n{text}");
        for marker in ["#Вставка", "#КонецВставки", "#Удаление", "#КонецУдаления"]
        {
            assert!(!text.contains(marker), "marker {marker} stripped:\n{text}");
        }

        // The inserted line is the only Inserted-origin run, and its ext range maps
        // back to the same source text it was copied from.
        let inserted: Vec<&Segment> =
            segments.iter().filter(|s| s.origin == Origin::Inserted).collect();
        assert_eq!(inserted.len(), 1, "exactly one inserted run");
        let seg = inserted[0];
        let ext_slice =
            &CHANGE_CONTROL_EXAMPLE[usize::from(seg.ext.start())..usize::from(seg.ext.end())];
        let eff_slice = &text[usize::from(seg.effective.start())..usize::from(seg.effective.end())];
        assert_eq!(ext_slice, eff_slice);
        assert!(
            ext_slice.contains("* Основание"),
            "inserted seg covers the fixed line: {ext_slice:?}"
        );
    }

    #[test]
    fn strip_without_directives_is_all_copied() {
        let code = "Процедура Тест()\nА = 1;\nКонецПроцедуры";
        let method = first_method(code);
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, segments) = strip_directives(&body);
        assert!(text.contains("А = 1;"));
        assert!(segments.iter().all(|s| s.origin == Origin::Copied));
    }

    fn render_strip(code: &str) -> String {
        let method = first_method(code);
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, segments) = strip_directives(&body);
        let mut out = format!("--- effective ---\n{text}\n--- segments ---\n");
        for s in &segments {
            let slice = &code[usize::from(s.ext.start())..usize::from(s.ext.end())];
            out += &format!(
                "{:?} eff={}..{} ext={}..{} {:?}\n",
                s.origin,
                u32::from(s.effective.start()),
                u32::from(s.effective.end()),
                u32::from(s.ext.start()),
                u32::from(s.ext.end()),
                slice,
            );
        }
        out
    }

    // Exact snapshot locks effective text + segment boundaries/origins so the merge
    // engine's trivia handling cannot drift under callers that remap diagnostics.
    #[test]
    fn strip_example_exact_snapshot() {
        expect_test::expect![[r#"
            --- effective ---
            Результат = 1;
            	Для Индекс = 1 По Степень Цикл


            		Результат = Результат * Основание;

            	КонецЦикла;
            	Возврат Результат;

            --- segments ---
            Copied eff=0..78 ext=177..255 "Результат = 1;\n\tДля Индекс = 1 По Степень Цикл\n"
            Copied eff=78..79 ext=364..365 "\n"
            Inserted eff=79..144 ext=380..445 "\n\t\tРезультат = Результат * Основание;\n"
            Copied eff=144..204 ext=470..530 "\n\tКонецЦикла;\n\tВозврат Результат;\n"
        "#]]
        .assert_eq(&render_strip(CHANGE_CONTROL_EXAMPLE));
    }

    #[test]
    fn nested_annotation_param_is_rejected() {
        // The string must be the parameter's own literal, not buried in a nested
        // annotation value (F-1 regression).
        let code = "&ИзменениеИКонтроль(&НаКлиенте)\nПроцедура Расш1_M()\nКонецПроцедуры";
        assert_eq!(extract_change_and_validate(&first_method(code)), None);
    }

    #[test]
    fn bare_change_and_validate_without_args_is_rejected() {
        let code = "&ИзменениеИКонтроль\nПроцедура Расш1_M()\nКонецПроцедуры";
        assert_eq!(extract_change_and_validate(&first_method(code)), None);
    }

    #[test]
    fn no_markers_change_and_validate_is_all_copied() {
        let code = "&ИзменениеИКонтроль(\"M\")\nПроцедура Расш1_M()\nА = 1;\nКонецПроцедуры";
        let method = first_method(code);
        assert_eq!(
            extract_change_and_validate(&method),
            Some(ChangeControl { target: "M".into() }),
        );
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, segments) = strip_directives(&body);
        assert!(text.contains("А = 1;"));
        assert!(segments.iter().all(|s| s.origin == Origin::Copied));
    }

    #[test]
    fn insert_only_keeps_content() {
        let code = "&ИзменениеИКонтроль(\"M\")\n\
Процедура Расш1_M()\n\
#Вставка\n\
Новое = 1;\n\
#КонецВставки\n\
КонецПроцедуры";
        let method = first_method(code);
        assert!(extract_change_and_validate(&method).is_some());
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, segments) = strip_directives(&body);
        assert!(text.contains("Новое = 1;"));
        assert_eq!(segments.iter().filter(|s| s.origin == Origin::Inserted).count(), 1);
    }

    #[test]
    fn delete_only_drops_content() {
        let code = "&ИзменениеИКонтроль(\"M\")\n\
Процедура Расш1_M()\n\
#Удаление\n\
Старое = 1;\n\
#КонецУдаления\n\
КонецПроцедуры";
        let method = first_method(code);
        assert!(extract_change_and_validate(&method).is_some());
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (text, _) = strip_directives(&body);
        assert!(!text.contains("Старое"), "deleted code must be gone:\n{text}");
    }

    #[test]
    fn multiple_insertions_yield_multiple_segments() {
        let code = "&ИзменениеИКонтроль(\"M\")\n\
Процедура Расш1_M()\n\
#Вставка\n\
А = 1;\n\
#КонецВставки\n\
Б = 2;\n\
#Вставка\n\
В = 3;\n\
#КонецВставки\n\
КонецПроцедуры";
        let method = first_method(code);
        assert!(extract_change_and_validate(&method).is_some());
        let body = method.children().find(|n| n.kind() == SyntaxKind::STMT_LIST).unwrap();
        let (_, segments) = strip_directives(&body);
        assert_eq!(segments.iter().filter(|s| s.origin == Origin::Inserted).count(), 2);
    }
}
