//! Разбор на паре входов, отличающихся долей тривии.
//!
//! Пара нужна именно парой: правка, снимающая тривию с потока событий, обязана
//! двигать второй вход и почти не двигать первый. Замер по одному входу такого
//! различия не показывает, а стена врёт настолько, что односторонний выигрыш
//! из неё не читается.
//!
//! Величина, которую правка двигает прямо, — число событий; она
//! детерминирована и меряется тестом `one_token_event_per_significant_lexeme`.
//! Здесь меряется время, и читать его следует только рядом с ним.

use criterion::{criterion_group, criterion_main, Criterion};

/// Тривии почти нет: разделителями работают операторы, а не пробелы.
fn nearly_no_trivia() -> String {
    (0..2000).map(|i| format!("Процедура П{i}()А=1;Б=2;В=А+Б;КонецПроцедуры\n")).collect()
}

/// Больше половины лексем — комментарии и переводы строк.
fn mostly_comments() -> String {
    (0..2000)
        .map(|i| {
            format!(
                "// комментарий к процедуре {i}\n// ещё строка описания\nПроцедура П{i}()\n\t// что делаем\n\tА = 1;\n\tБ = 2;\n\tВ = А + Б;\nКонецПроцедуры\n\n"
            )
        })
        .collect()
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (name, text) in
        [("nearly_no_trivia", nearly_no_trivia()), ("mostly_comments", mostly_comments())]
    {
        group.throughput(criterion::Throughput::Bytes(text.len() as u64));
        group.bench_function(name, |b| b.iter(|| parser::parse(std::hint::black_box(&text))));
    }
    group.finish();
}

criterion_group!(benches, bench_parse);
criterion_main!(benches);
