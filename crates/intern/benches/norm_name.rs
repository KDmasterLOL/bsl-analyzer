//! Microbenchmarks for the NormName pool: the interned hit path must beat
//! the per-occurrence fold it replaces, single-threaded and under parallel
//! load, or the pipeline migration has no basis.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use intern::NormName;
use stdx::case::fold_lower_per_char;

/// Deterministic mixed RU/EN identifier corpus shaped like BSL code: a small
/// vocabulary repeated many times with varied casing.
fn corpus() -> Vec<String> {
    let stems = [
        "ОбщегоНазначения",
        "ПолучитьЗначение",
        "РаботаСФайлами",
        "ExecuteQuery",
        "СтрокаСоединения",
        "ОбработкаПроведения",
        "ValueTable",
        "ТекущаяДатаСеанса",
    ];
    let mut out = Vec::with_capacity(stems.len() * 64);
    for round in 0..64u32 {
        for stem in stems {
            let spelling: String = stem
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    if (i as u32 + round).is_multiple_of(3) {
                        c.to_uppercase().next().unwrap()
                    } else {
                        c.to_lowercase().next().unwrap()
                    }
                })
                .collect();
            out.push(spelling);
        }
    }
    out
}

fn bench_norm_name(c: &mut Criterion) {
    let names = corpus();
    for name in &names {
        NormName::intern(name);
    }

    c.bench_function("hit/interned_lookup", |b| {
        b.iter(|| {
            for name in &names {
                black_box(NormName::intern(black_box(name)));
            }
        })
    });

    c.bench_function("baseline/fold_per_occurrence", |b| {
        b.iter(|| {
            for name in &names {
                black_box(fold_lower_per_char(black_box(name)));
            }
        })
    });

    let mut unique = 0u64;
    c.bench_function("miss/new_spelling", |b| {
        b.iter(|| {
            unique += 1;
            black_box(NormName::intern(&format!("УникальноеИмя{unique}")))
        })
    });

    c.bench_function("hit/parallel_8_threads", |b| {
        b.iter_custom(|iters| {
            let start = std::time::Instant::now();
            std::thread::scope(|s| {
                for _ in 0..8 {
                    let names = &names;
                    s.spawn(move || {
                        for _ in 0..iters {
                            for name in names {
                                black_box(NormName::intern(black_box(name)));
                            }
                        }
                    });
                }
            });
            start.elapsed()
        })
    });
}

criterion_group!(benches, bench_norm_name);
criterion_main!(benches);
