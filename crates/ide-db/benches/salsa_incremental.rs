use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hir::DefDatabase;
use ide_db::{
    base_db::{RootQueryDb, SourceDatabase},
    RootDatabaseImpl,
};
use vfs::{file_set::FileSet, FileId, VfsPath};

fn setup_db(num_files: u32) -> RootDatabaseImpl {
    let mut db = RootDatabaseImpl::new();

    let mut file_set = FileSet::new();
    for i in 0..num_files {
        let file_id = FileId(i);
        file_set.insert(file_id, VfsPath::new(format!("/test{}.bsl", i)));
    }

    let source_root = ide_db::base_db::SourceRoot::new_local(file_set);
    db.set_source_root(ide_db::base_db::SourceRootId(0), source_root);

    for i in 0..num_files {
        let file_id = FileId(i);
        db.set_file_source_root(file_id, ide_db::base_db::SourceRootId(0));
        db.set_file_text(
            file_id,
            &format!(
                r#"
Процедура Процедура{}()
    Сообщить("Тест {}", {});
КонецПроцедуры

Функция Функция{}() Экспорт
    Возврат {};
КонецФункции
"#,
                i, i, i, i, i
            ),
        );
    }

    db
}

fn bench_cache_hit(c: &mut Criterion) {
    let db = setup_db(100);
    let file_id = FileId(50);

    let _ = db.parse(file_id);

    c.bench_function("cache_hit", |b| {
        b.iter(|| {
            let _ = db.parse(black_box(file_id));
        });
    });
}

fn bench_incremental_update(c: &mut Criterion) {
    c.bench_function("incremental_update", |b| {
        let mut db = setup_db(100);
        let file_id = FileId(50);

        let _ = db.parse(file_id);

        let mut counter = 0;
        b.iter(|| {
            counter += 1;
            db.set_file_text(
                file_id,
                black_box(&format!(
                    r#"
Процедура ТестоваяПроцедура{}()
    Сообщить("Изменение {}", {});
КонецПроцедуры
"#,
                    counter, counter, counter
                )),
            );
            let _ = db.parse(file_id);
        });
    });
}

fn bench_item_tree_cache_hit(c: &mut Criterion) {
    let db = setup_db(100);
    let file_id = FileId(50);

    let _ = db.item_tree(file_id);

    c.bench_function("item_tree_cache_hit", |b| {
        b.iter(|| {
            let _ = db.item_tree(black_box(file_id));
        });
    });
}

fn bench_item_tree_incremental(c: &mut Criterion) {
    c.bench_function("item_tree_incremental", |b| {
        let mut db = setup_db(100);
        let file_id = FileId(50);

        let _ = db.item_tree(file_id);

        let mut counter = 0;
        b.iter(|| {
            counter += 1;
            db.set_file_text(
                file_id,
                black_box(&format!(
                    r#"
Процедура НоваяПроцедура{}()
КонецПроцедуры

Функция НоваяФункция{}() Экспорт
    Возврат {};
КонецФункции
"#,
                    counter, counter, counter
                )),
            );
            let _ = db.item_tree(file_id);
        });
    });
}

fn bench_symbol_tree_cache_hit(c: &mut Criterion) {
    let db = setup_db(100);
    let module_id = hir::ModuleId::new(FileId(50));

    let _ = db.symbol_tree(module_id);

    c.bench_function("symbol_tree_cache_hit", |b| {
        b.iter(|| {
            let _ = db.symbol_tree(black_box(module_id));
        });
    });
}

fn bench_large_file_set(c: &mut Criterion) {
    c.bench_function("large_file_set_lru", |b| {
        let db = setup_db(200);

        b.iter(|| {
            for i in 0..200 {
                let file_id = FileId(black_box(i % 200));
                let _ = db.parse(file_id);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_cache_hit,
    bench_incremental_update,
    bench_item_tree_cache_hit,
    bench_item_tree_incremental,
    bench_symbol_tree_cache_hit,
    bench_large_file_set
);
criterion_main!(benches);
