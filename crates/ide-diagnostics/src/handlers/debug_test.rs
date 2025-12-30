#[cfg(test)]
mod tests {
    use ide_db::base_db::{RootQueryDb, SourceDatabase};
    use ide_db::RootDatabaseImpl;
    use syntax::SyntaxKind;
    use test_fixture::Fixture;

    #[test]
    fn test_parse_simple_function() {
        let code = r#"Функция Тест()
    Если Истина Тогда
        Возврат 1;
    КонецЕсли;
КонецФункции"#;

        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        eprintln!("\n=== PARSE TREE ===");
        eprintln!("Root kind: {:?}", root.kind());
        eprintln!("Root text:\n{}", root.text());
        eprintln!("\n=== DESCENDANTS ===");

        let mut func_count = 0;
        for node in root.descendants() {
            let kind = node.kind();

            eprintln!("  {:?}", kind);

            if kind == SyntaxKind::FUNCTION_DEF {
                func_count += 1;
                eprintln!("    ^^^ FOUND FUNCTION_DEF!");
            }
        }

        eprintln!("\nTotal functions found: {}", func_count);
        assert_eq!(func_count, 1, "Should find exactly 1 function");
    }

    #[test]
    fn test_function_name_token_range() {
        let code = r#"Функция ОпределитьСтавкуНДС(Знач Ставка)
    Если Ставка = Истина Тогда
        Возврат 20;
    КонецЕсли;
КонецФункции"#;

        eprintln!("\n=== Source Code ===");
        eprintln!("{}", code);
        eprintln!("\n=== Byte Analysis (first 60 bytes) ===");
        for i in 0..std::cmp::min(60, code.len()) {
            eprintln!("Byte {}: {:?}", i, code.as_bytes()[i] as char);
        }

        // Parse
        let fixture_text = format!("//- /test.bsl\n{}", code);
        let fixture = Fixture::parse(&fixture_text);
        let file_id = fixture.first_file().expect("fixture should have at least one file");

        let mut db = RootDatabaseImpl::new();
        // Save file content for comparison
        let mut file_content = String::new();
        for (fid, file) in &fixture.files {
            db.set_file_text(*fid, &file.content);
            if *fid == file_id {
                file_content = file.content.to_string();
            }
        }

        let parse = db.parse(file_id);
        let root = parse.syntax_node();

        eprintln!("\n=== File content in database (first 60 bytes) ===");
        for i in 0..std::cmp::min(60, file_content.len()) {
            eprintln!("Byte {}: {:?}", i, file_content.as_bytes()[i] as char);
        }

        eprintln!("\n=== Syntax Tree ===");
        for node in root.descendants() {
            if node.kind() == SyntaxKind::FUNCTION_DEF {
                eprintln!("FUNCTION_DEF found at {:?}", node.text_range());

                // Find IDENT token (function name)
                for el in node.children_with_tokens() {
                    if let syntax::NodeOrToken::Token(tok) = el {
                        if tok.kind() == SyntaxKind::IDENT {
                            eprintln!(
                                "  IDENT token: {:?} @ {:?} = {:?}",
                                tok.kind(),
                                tok.text_range(),
                                tok.text()
                            );

                            let range = tok.text_range();
                            let start_byte = u32::from(range.start()) as usize;
                            let end_byte = u32::from(range.end()) as usize;

                            eprintln!("    Byte range: {}..{}", start_byte, end_byte);

                            if start_byte > 0 {
                                eprintln!(
                                    "    Previous byte ({}): {:?}",
                                    start_byte - 1,
                                    file_content.as_bytes().get(start_byte - 1).map(|&b| b as char)
                                );
                            }
                            eprintln!(
                                "    Actual text at range: {:?}",
                                &file_content[start_byte..end_byte]
                            );
                            eprintln!("    Token text: {:?}", tok.text());

                            break;
                        }
                    }
                }

                break;
            }
        }
    }
}
