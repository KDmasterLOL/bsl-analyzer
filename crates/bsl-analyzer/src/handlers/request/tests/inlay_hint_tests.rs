use super::*;

#[test]
fn inlay_hints_preserve_repeated_nested_receiver_labels_at_lsp_positions() {
    let mut state = create_test_state();
    state.init_empty_source_root();

    let uri = lsp_types::Url::parse("file:///repeated-inlay-hints.bsl").unwrap();
    let source = "Процедура Тест()\n    Массив = Новый Массив;\n    Список = Новый СписокЗначений;\n    Массив.Добавить(1);\n    Список.Добавить(2);\n    Массив.Добавить(Массив.Добавить(3));\n    Массив.Добавить(,\nКонецПроцедуры\n";
    open_source(&mut state, &uri, source);

    let params = InlayHintParams {
        work_done_progress_params: Default::default(),
        text_document: TextDocumentIdentifier { uri },
        range: lsp_types::Range {
            start: Position { line: 3, character: 0 },
            end: Position { line: 7, character: 0 },
        },
    };

    let hints = handle_inlay_hint(&latency_ctx(&state), params).unwrap().unwrap();
    let rendered: Vec<(String, Position)> = hints
        .into_iter()
        .map(|hint| {
            let InlayHintLabel::String(label) = hint.label else {
                panic!("expected string label");
            };
            (label, hint.position)
        })
        .collect();

    assert_eq!(
        rendered,
        vec![
            ("Значение:".to_string(), Position { line: 3, character: 20 }),
            ("Значение:".to_string(), Position { line: 4, character: 20 }),
            ("Значение:".to_string(), Position { line: 5, character: 20 }),
            ("Значение:".to_string(), Position { line: 5, character: 36 }),
        ],
    );
}
