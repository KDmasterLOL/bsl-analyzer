use ide_db::RootDatabase;
use stdx::case::CaseExt;
use symbol_info::{build_signature, resolve_callee_at};
use syntax::{NodeOrToken, SyntaxKind, SyntaxNode, TextRange, TextSize};
use vfs::FileId;

/// A single inlay hint — a label the editor renders inline at `position`,
/// kept free of `lsp_types` so the adapter maps the offset with its encoding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub position: TextSize,
    pub label: String,
    pub kind: InlayHintKind,
    pub padding_left: bool,
    pub padding_right: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlayHintKind {
    Parameter,
    Type,
}

/// Inlay hints whose position falls inside `range` (the editor's visible span).
///
/// Currently emits parameter-name hints at call arguments. Inferred-type hints
/// for variables are a planned follow-up (they need the receiver-independent
/// per-binding inference surface, not yet exposed to this layer).
pub fn inlay_hints<DB: RootDatabase>(db: &DB, file_id: FileId, range: TextRange) -> Vec<InlayHint> {
    let _span = tracing::info_span!("inlay_hints", ?file_id).entered();
    let parse = db.parse(file_id);
    let root = parse.syntax_node();

    let mut hints = Vec::new();
    for node in root.descendants() {
        if node.kind() != SyntaxKind::ARG_LIST {
            continue;
        }
        if range.intersect(node.text_range()).is_none() {
            continue;
        }
        parameter_hints_for_arg_list(db, file_id, &node, range, &mut hints);
    }
    hints
}

fn parameter_hints_for_arg_list<DB: RootDatabase>(
    db: &DB,
    file_id: FileId,
    arg_list: &SyntaxNode,
    range: TextRange,
    hints: &mut Vec<InlayHint>,
) {
    // Anchor callee resolution inside the argument list so it selects this call
    // (and not an enclosing one); an empty list carries nothing to label.
    let Some(first_arg) = arg_list.children().next() else {
        return;
    };
    let Some((callee, _active)) = resolve_callee_at(db, file_id, first_arg.text_range().start())
    else {
        return;
    };
    let Some(signature) = build_signature(db, file_id, &callee) else {
        return;
    };

    // Positional slot = number of commas before the argument, matching how the
    // signature's parameters are ordered (empty slots keep the count aligned).
    let mut slot = 0usize;
    for element in arg_list.children_with_tokens() {
        match element {
            NodeOrToken::Token(token) => {
                if token.kind() == SyntaxKind::COMMA {
                    slot += 1;
                }
            }
            NodeOrToken::Node(arg) => {
                if let Some(param) = signature.params.get(slot) {
                    maybe_push_param_hint(&arg, param.name.as_str(), range, hints);
                }
            }
        }
    }
}

fn maybe_push_param_hint(
    arg: &SyntaxNode,
    param_name: &str,
    range: TextRange,
    hints: &mut Vec<InlayHint>,
) {
    if param_name.is_empty() {
        return;
    }
    let position = arg.text_range().start();
    if !range.contains(position) {
        return;
    }
    // Drop the hint when the argument already spells the parameter — the label
    // would only echo the code, e.g. `Записать(Режим)`. Case-insensitive to
    // match BSL identifier folding.
    if arg.text().to_string().fold_lower() == param_name.fold_lower() {
        return;
    }
    hints.push(InlayHint {
        position,
        label: format!("{param_name}:"),
        kind: InlayHintKind::Parameter,
        padding_left: false,
        padding_right: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::base_db::{SourceDatabase, SourceRoot, SourceRootId};
    use ide_db::RootDatabaseImpl;
    use vfs::{file_set::FileSet, VfsPath};

    fn single_file(source: &str) -> (RootDatabaseImpl, FileId) {
        let mut db = RootDatabaseImpl::default();
        let file_id = FileId(0);
        let mut file_set = FileSet::new();
        file_set.insert(file_id, VfsPath::new("/test.bsl"));
        let source_root = SourceRoot::new_local(file_set);
        db.set_source_root(SourceRootId(0), source_root);
        db.set_file_source_root(file_id, SourceRootId(0));
        db.set_file_text(file_id, source);
        (db, file_id)
    }

    fn whole_range(source: &str) -> TextRange {
        TextRange::new(TextSize::from(0), TextSize::from(source.len() as u32))
    }

    fn labels_at(source: &str, hints: &[InlayHint]) -> Vec<(String, String)> {
        hints
            .iter()
            .map(|h| {
                let after = &source[usize::from(h.position)..];
                let word: String = after.chars().take_while(|c| c.is_alphanumeric()).collect();
                (h.label.clone(), word)
            })
            .collect()
    }

    const MODULE: &str = r#"
Функция Сложить(Первое, Второе)
    Возврат Первое + Второе;
КонецФункции

Процедура Тест()
    Сложить(10, 20);
КонецПроцедуры
"#;

    #[test]
    fn parameter_hints_label_each_argument() {
        let (db, file_id) = single_file(MODULE);
        let hints = inlay_hints(&db, file_id, whole_range(MODULE));
        assert!(hints.iter().all(|h| h.kind == InlayHintKind::Parameter));
        // Hints attach to the argument literals 10 and 20.
        let seen = labels_at(MODULE, &hints);
        assert!(seen.contains(&("Первое:".to_string(), "10".to_string())), "{seen:?}");
        assert!(seen.contains(&("Второе:".to_string(), "20".to_string())), "{seen:?}");
    }

    #[test]
    fn skips_hint_when_argument_echoes_parameter_name() {
        let source = r#"
Функция Сложить(Первое, Второе)
    Возврат Первое + Второе;
КонецФункции

Процедура Тест()
    Первое = 1;
    Сложить(Первое, 20);
КонецПроцедуры
"#;
        let (db, file_id) = single_file(source);
        let hints = inlay_hints(&db, file_id, whole_range(source));
        assert!(
            hints.iter().all(|h| h.label != "Первое:"),
            "argument spelled like the parameter must not get a hint: {hints:?}"
        );
        assert!(hints.iter().any(|h| h.label == "Второе:"));
    }

    #[test]
    fn no_hints_for_call_without_arguments() {
        let source =
            "Процедура Х()\nКонецПроцедуры\n\nПроцедура Тест()\n    Х();\nКонецПроцедуры\n";
        let (db, file_id) = single_file(source);
        assert!(inlay_hints(&db, file_id, whole_range(source)).is_empty());
    }

    #[test]
    fn hints_are_confined_to_the_requested_range() {
        let (db, file_id) = single_file(MODULE);
        // A range covering only the function declaration, not the call site.
        let decl_only = TextRange::new(
            TextSize::from(0),
            TextSize::from(MODULE.find("Процедура Тест").unwrap() as u32),
        );
        assert!(inlay_hints(&db, file_id, decl_only).is_empty());
    }
}
