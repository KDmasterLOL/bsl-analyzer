use ide_db::{base_db::SourceDatabase, RootDatabaseImpl};
use test_fixture::Fixture;

fn main() {
    let fixture_text = r#"
//- /test.bsl
// Вычисляет сумму двух чисел.
//
// Параметры:
//   А - Число - первое слагаемое
//   Б - Число - второе слагаемое
//
// Возвращаемое значение:
//   Число - результат сложения
//
// Пример:
//   Результат = Сумма(2, 3); // Результат = 5
//
Функция Сумма(А, Б) Экспорт
    Возврат А + Б;
КонецФункции

// См. Сумма()
Функция СуммаЧисел(А, Б) Экспорт
    Возврат Сумма(А, Б);
КонецФункции

// Возвращает информацию о пользователе.
//
// Параметры:
//   ID - Число - идентификатор пользователя
//
// Возвращаемое значение:
//   Структура:
//     * Имя - Строка - имя пользователя
//     * Возраст - Число - возраст пользователя
//     * Email - Строка - адрес электронной почты
//
Функция ПолучитьПользователя(ID) Экспорт
    Результат = Новый Структура;
    Результат.Вставить("Имя", "Иван");
    Результат.Вставить("Возраст", 30);
    Результат.Вставить("Email", "ivan@example.com");
    Возврат Результат;
КонецФункции
"#;

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║          HIR Method Documentation API Demo                    ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let fixture = Fixture::parse(fixture_text);
    let file_id = fixture.first_file().expect("fixture should have a file");

    let mut db = RootDatabaseImpl::new();
    for (fid, file) in &fixture.files {
        db.set_file_text(*fid, &file.content);
    }

    let module_id = hir_def::ModuleId::new(file_id);
    let module_data = db.module_data(module_id);

    println!("📄 File: test.bsl\n");
    println!("Found {} functions\n", module_data.functions.len());

    for (idx, method_id) in module_data.functions.iter().enumerate() {
        let method = hir::Method::new(&db, *method_id);
        let name = method.name();

        println!("┌─────────────────────────────────────────────────────────────┐");
        println!("│ Function #{}: {}", idx + 1, name);
        println!("└─────────────────────────────────────────────────────────────┘");

        if let Some(docs) = method.docs() {
            if let Some(purpose) = &docs.purpose {
                println!("\n📝 Purpose:");
                println!("   {}", purpose);
            }

            if let Some(link) = &docs.link {
                println!("\n🔗 Hyperlink:");
                println!("   {}", link);
            }

            if !docs.parameters.is_empty() {
                println!("\n📋 Parameters:");
                for param in &docs.parameters {
                    let types: Vec<_> = param.types.iter().map(|t| &t.name).collect();
                    print!("   • {} ({}", param.name, types.join(", "));

                    if let Some(type_doc) = param.types.first() {
                        if let Some(desc) = &type_doc.description {
                            print!(") - {}", desc);
                        } else {
                            print!(")");
                        }
                    }
                    println!();
                }
            }

            if !docs.returned_value.is_empty() {
                println!("\n↩️  Return Value:");
                for type_doc in &docs.returned_value {
                    print!("   • {}", type_doc.name);

                    if let Some(desc) = &type_doc.description {
                        print!(" - {}", desc);
                    }
                    println!();

                    if !type_doc.parameters.is_empty() {
                        println!("     Fields:");
                        for field in &type_doc.parameters {
                            let field_types: Vec<_> =
                                field.types.iter().map(|t| &t.name).collect();
                            print!("       • {} ({}", field.name, field_types.join(", "));

                            if let Some(field_type) = field.types.first() {
                                if let Some(desc) = &field_type.description {
                                    print!(") - {}", desc);
                                } else {
                                    print!(")");
                                }
                            }
                            println!();
                        }
                    }
                }
            }

            if !docs.examples.is_empty() {
                println!("\n💡 Examples:");
                for example in &docs.examples {
                    println!("   {}", example);
                }
            }

            println!();
            if docs.is_hyperlink() {
                println!("   🔗 This is a hyperlink reference");
            }
            if docs.is_deprecated() {
                println!("   ⚠️  This method is deprecated");
                if let Some(dep) = &docs.deprecation {
                    println!("      {}", dep);
                }
            }
        } else {
            println!("\n   (no documentation)");
        }

        println!();
    }

    println!("\n✅ All methods processed successfully!");
    println!("\n💡 Key features demonstrated:");
    println!("   • Structured documentation parsing");
    println!("   • Purpose, parameters, return values");
    println!("   • Nested structure fields");
    println!("   • Hyperlink references");
    println!("   • Salsa caching (automatic)");
}
