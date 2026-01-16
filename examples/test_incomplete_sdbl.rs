use parser::parse_sdbl;

fn main() {
    let query = r#"ВЫБРАТЬ РАЗЛИЧНЫЕ
    ЗадачиЭлементовСхемы.ИмяЭлемента,
    ЗадачиЭлементовСхемы.ЗадачаПроцесса
ПОМЕСТИТЬ ВТ_ЗадачиСхемы
ИЗ
    &ЗадачиЭлементовСхемы КАК ЗадачиЭлементовСхемы
;

////////////////////////////////////////////////////////////////////////////////
ВЫБРАТЬ
    ВТ_ЗадачиСхемы.ИмяЭлемента
ИЗ
    ВТ_ЗадачиСхемы КАК ВТ_ЗадачиСхемы
        ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ДанныеБизнесПроцессов КАК ДанныеБизнесПроцессов
            ВНУТРЕННЕЕ СОЕДИНЕНИЕ РегистрСведений.ПроцессыДействий КАК ПроцессыДействий
            ПО ДанныеБизнесПроцессов.БизнесПроцесс = ПроцессыДействий.
            И ПроцессыДействий.Действие = &Действие
        ПО ВТ_ЗадачиСхемы.ЗадачаПроцесса = ДанныеБизнесПроцессов."#;

    println!("Query length: {}", query.len());
    
    let parse = parse_sdbl(query);
    
    println!("\nParse errors: {}", parse.errors().len());
    for (i, err) in parse.errors().iter().enumerate() {
        println!("  Error {}: {:?}", i + 1, err);
    }
    
    use syntax::ast::AstNode;
    let root = parse.syntax_node();
    
    if let Some(package) = syntax::ast::SdblQueryPackage::cast(root.clone()) {
        let queries: Vec<_> = package.queries().collect();
        println!("\nFound {} queries", queries.len());
        
        for (i, q) in queries.iter().enumerate() {
            println!("\nQuery {}: range {:?}", i, q.syntax().text_range());
            println!("  Text length: {}", q.syntax().text().len());
        }
    }
    
    // Try lowering to HIR
    println!("\n\nLowering to HIR...");
    let hir = sdbl_hir::lower::lower_sdbl_to_hir(&parse, None);
    
    println!("HIR package has {} queries", hir.queries.len());
    
    for (i, q) in hir.queries.iter().enumerate() {
        println!("\nHIR Query {}: range {:?}", i, q.range);
        println!("  FROM tables: {}", q.hir.from.len());
        println!("  JOIN tables: {}", q.hir.joins.len());
        
        for (j, join) in q.hir.joins.iter().enumerate() {
            println!("    JOIN {}: {} (alias: {:?})", j, join.table.full_name, join.table.alias);
        }
    }
    
    println!("\nSource map has {} tokens", hir.source_map.all_tokens().count());
}
