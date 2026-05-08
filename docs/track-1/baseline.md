# Track 1 — Performance & Memory Baseline

Снято до начала имплементации Track 1 (см. план
`<engineering scratch plans>/linear-tumbling-noodle.md`, §10 Performance &
memory budget). Все будущие правки сравниваются с этими числами.

## Configuration

- **Branch**: `track-1-foundation`
- **HEAD commit**: `e18f3a60c5b66bfc9c5ed4ff889092119b8a832e`
- **Date (UTC)**: 2026-05-07T20:09:51Z
- **Host**: Linux 6.19.13-arch1-1
- **Cargo profile**: `release` (analyze) и `bench` (Criterion)

## Targets (из плана §10)

| Метрика | Baseline | Ceiling (+budget) |
|---|---:|---:|
| Cold reanalysis wall | 49.11 s | 56.48 s (+15%) |
| Hot edit-to-diagnostic | TBD | TBD (+20%) |
| RSS peak | 1 669 196 KB ≈ 1.59 GB | ≈ 1.64 GB (+50 MB) |
| Salsa LRU caps | unbounded today | явный `cap` для каждой новой query |

## Baseline #1: Salsa incremental (Criterion)

Бенч `crates/ide-db/benches/salsa_incremental.rs` — синтетические BSL файлы,
измеряет cache hit / incremental update / memory efficiency для Salsa
queries.

Команда: `cargo bench -p ide-db --bench salsa_incremental`
Полный лог: `/tmp/baseline-criterion.log` (build + 6 benches).

| Benchmark | low | median | high | outliers (mild/severe) |
|---|---:|---:|---:|---|
| `cache_hit` | 30.356 ns | 30.447 ns | 30.565 ns | 4 / 6 |
| `incremental_update` | 13.754 µs | 14.239 µs | 14.781 µs | 3 / 1 |
| `item_tree_cache_hit` | 30.710 ns | 30.763 ns | 30.823 ns | 6 / 2 |
| `item_tree_incremental` | 17.661 µs | 18.953 µs | 20.331 µs | 3 / 6 |
| `symbol_tree_cache_hit` | 31.288 ns | 31.331 ns | 31.383 ns | 7 / 4 |
| `large_file_set_lru` | 6.888 µs | 6.912 µs | 6.937 µs | 3 / 6 |

Notes:
- `*_cache_hit` ~30 ns — Salsa direct lookup без аллокаций.
- `incremental_update` / `item_tree_incremental` — пере-вычисление одного
  query при изменении входа (микросекунды).
- `large_file_set_lru` 6.9 µs — LRU eviction round-trip.
- Регрессионный критерий после Track 1: median не сдвигается более чем
  на +10 % (для `*_cache_hit` строже — +5 %, это direct lookup).

## Baseline #2: Cold reanalysis на BSL corpus

Корпус: локальный BSL-проект (~13 442 .bsl-файлов, 5.4 GB исходников).
Путь не коммитится; corpus shape (число файлов, размер) фиксируется.

Команда:
```
/usr/bin/time -v target/release/bsl-analyzer-app analyze \
    -s <corpus> -q -o /tmp/baseline-analyze-out
```

Результаты (`/usr/bin/time -v` через `getrusage`):

| Метрика | Значение |
|---|---:|
| Wall clock | **49.11 s** |
| User CPU | 455.14 s |
| System CPU | 4.82 s |
| CPU parallelism factor | ~9.4× (User+Sys / Wall) |
| Major page faults | 0 |
| Minor page faults | 1 672 761 |
| Voluntary ctx switches | 40 115 |
| Involuntary ctx switches | 47 578 |
| Exit code | 0 |

Notes:
- 9.4× parallelism — Salsa + rayon corpus-level work хорошо насыщают CPU.
- Major page faults = 0 — рабочий set вмещается в RAM, нет swap-pressure.
- Wall 49.11 s = baseline для §10.3 «cold reanalysis +15 % ceiling»
  (56.48 s).

## Baseline #3: Memory peak (RSS)

Из того же запуска:

| Метрика | Значение |
|---|---:|
| Maximum RSS | **1 669 196 KB** (1 630.07 MB ≈ 1.59 GB) |

Notes:
- Это пик `/proc/self/status` VmHWM, измеренный getrusage'ом в конце
  процесса.
- Baseline для §10 «RSS +50 MB ceiling» — следующий снимок не должен
  превышать ~1 719 196 KB (≈ 1.64 GB).

## Baseline #4: Hot edit-to-diagnostic (deferred)

Не зафиксировано. Для воспроизводимого LSP-сценария
(`textDocument/didChange` → `publishDiagnostics`) нужен driver, которого
сейчас в репо нет. Создание driver — отдельная задача в Track 1
(добавлю в Step C/E когда LSP-side изменения станут видны диагностикам).
До тех пор §10.4 acceptance gate помечен `pending hot-baseline driver`.

## Reproducibility

```bash
git checkout e18f3a60c5b66bfc9c5ed4ff889092119b8a832e
cargo build --release -p bsl-analyzer --bin bsl-analyzer-app

# Baseline #1
cargo bench -p ide-db --bench salsa_incremental 2>&1 | tee criterion.log

# Baseline #2 + #3
/usr/bin/time -v target/release/bsl-analyzer-app analyze \
    -s <corpus> -q -o /tmp/analyze-out 2> cold.log
grep -E "Elapsed|User time|System time|Maximum resident" cold.log
```

Сравнение:
- Criterion: median не более +10 % (cache hits — +5 %).
- Cold wall: не более +15 % (49.11 s → 56.48 s).
- RSS peak: не более +50 MB (1 669 196 → 1 719 196 KB).

## Snapshot policy

При прохождении ключевых этапов плана (Steps E, G, Q — новые Salsa
queries) бенчи повторяются и сохраняются как
`docs/track-1/baseline-step-{e|g|q}.md`. Каждый snapshot фиксирует
delta vs current baseline; превышение ceiling — блокер для merge'а
до выяснения причины (см. §6 acceptance gate).
