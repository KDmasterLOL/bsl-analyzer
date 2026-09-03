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
| Hot edit-to-diagnostic | 7.33 s (см. Baseline #4, снято 2026-09-01); после яруса 1 github#113 — 2.10 s, после яруса 2 — 1.17 s; вставка метода в начало файла после яруса 3 — 3.07 s (было 4.92) | цель github#113 — снижение на порядок |
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

## Baseline #4: Hot edit-to-diagnostic

Снято 2026-09-01 на `ba55f5c8` (v0.2.76), release-сборка, driver —
`bsl-analyzer bench` с курируемым манифестом
`scripts/bench/manifests/issue113-erp.json` (github#113). Корпус — ERP,
модуль `CommonModules/МенеджерОбменаЧерезУниверсальныйФормат/Ext/Module.bsl`:
107 143 строки, 9 965 284 байта, 2349 методов. Правка идёт через настоящий
`didChange` (ranged), затем измеряется первый `diagnostics_push`
(`edit.after_edit_ns`).

| Точка | Правка | after_edit | edit_apply |
|---|---|---:|---:|
| `edit/body_insert_common_large/01` | вставка оператора в тело (стр. 57680) | **7.33 s** | 12.6 ms |
| `edit/signature_param_common_large/01` | новый параметр того же метода (стр. 57679) | **7.51 s** | 12.3 ms |

Одинаковая цена обеих правок — диагноз github#113: пересчёт файловый.
`--mode recompute` по правке тела: `infer_method_query` execute 2382 /
validate 209, `method_body_query` 2374 / 413, `doc_see_signature_query`
2333 / 603; 7201 ключей в 12 модулях. Разложение по фазам
(`BSL_PROFILE='*'`): инференс 2.9 s, лоуэринг тел дважды (1.7 s + 1.6 s),
хендлеры по файлу ~0.7 s, `parse` 0.26 s (3.4 %).

Команды:
```
target/release/bsl-analyzer-app bench run -s <erp> \
    -m scripts/bench/manifests/issue113-erp.json \
    -p edit/body_insert_common_large/01 --mode latency --json out.json
target/release/bsl-analyzer-app bench run -s <erp> \
    -m scripts/bench/manifests/issue113-erp.json \
    -p edit/body_insert_common_large/01 --mode recompute --json churn.json
scripts/bench/run-matrix.sh -s <erp> -m scripts/bench/manifests/issue113-erp.json -o out/
```
Ceiling из §10 (+20 %) к этой линии не применяется: цель github#113 —
снижение на порядок, гейт — `bench compare` по той же точке.

### После яруса 1 github#113 (позиционная независимость ядра)

Снято 2026-09-01 той же процедурой на ветке `feat/issue113-method-increment`.

| Точка | after_edit | edit_apply |
|---|---:|---:|
| `edit/body_insert_common_large/01` | **2.10 s** (было 7.33) | 14.2 ms |
| `edit/signature_param_common_large/01` | **3.35 s** (было 7.51) | 19.8 ms |
| `edit_burst/body_insert_common_large/01` (5 правок) | 2.39 s | 45.2 ms |

Правка сигнатуры стала дороже правки тела — стенд их различает.
`--mode recompute` по правке тела: `method_lower_query`, `method_body_query`,
`infer_method_query` execute **1** (validate 6396 / 2783 / 2348); единственные
семейства с execute > 1 — `method_syntax_query` 2349 (точка отсечения:
пересчёт с равным значением) и `reaching_definitions_query` 1609 (срез файлового
`module_reaching_definitions`, бэкдейтится). Память режима `memory`:
пик фазы 1.42 ГБ, VmHWM 1.97 ГБ. Остаток 2.1 s — файловые dataflow и хендлеры
(ярус 2).

### После яруса 2 github#113 (диагностики и dataflow по методу)

Снято 2026-09-02 той же процедурой на `b19d3706` (release-сборка в чистом
воркдереве по sha; сторона A сверок — честная сборка `abf618da`).

| Точка | after_edit |
|---|---:|
| `edit/body_insert_common_large/01` | **1.17 s** (было 2.10) |
| `edit/signature_param_common_large/01` | **3.05 s** (было 3.35) |

`--mode recompute` по правке тела: девять per-method семейств
(`method_cfg`, `reaching_definitions`, `method_path_terminates`,
`method_hir_metrics`, `method_cyclomatic`, `method_security_state`,
`method_arg_diagnostics`, `infer_method`, `method_diagnostics`) execute **1**.
Эквивалентность: SARIF-сверка `analyze` по ERP (`~/bench-bsl/ab/abdiff.py`)
против `abf618da` — 0 расхождений (положительный контроль: та же сверка
ловила +9/−8 шовных до починки). Пакетный `analyze` ERP, ABBA на тихой машине:
user CPU 1953/1974 s (A) против 1935/1933 s (B) — −1.5 %, RSS 5.97/6.02 →
5.74/5.74 ГБ. Память режима `memory`: пик фазы 1.33 ГБ, VmHWM 1.95 ГБ
(ярус 1: 1.42 / 1.97). Остаток окна правки тела (профиль fp-сборки): `deep_verify`
~20 %, файловые строчные/синтаксические проходы ~25 % (LineLength,
MissingSpace, IncorrectLineBreak, CommentedCode — им нужна строка целиком,
«плита» с единоличным владением строкой не делалась), parse 8 %,
`module_call_summary` 8 %, `region_tree` 4 %. Цель 0.6 s этим ярусом не
достигнута: дальше — переразбор и тождество метода (ярусы 3–4).

### После яруса 3 github#113 (тождество метода вместо позиции)

Стенд яруса — две новые точки того же манифеста: вставка метода в начало
файла (`edit/method_insert_top_common_large/01`, байт 438) и в конец
(`edit/method_insert_bottom_common_large/01`, байт 9 965 228). Снято
2026-09-02 на `ddb8cede` (стенд до миграции) и на `7a340248` (release-сборка
в чистом воркдереве по sha).

| Точка | до (`ddb8cede`) | после |
|---|---:|---:|
| `edit/method_insert_top_common_large/01` | 4.92 / 4.95 s | **3.07 / 3.10 s** |
| `edit/method_insert_bottom_common_large/01` | 3.06 / 3.05 s | 3.09 / 3.11 s |
| `edit/body_insert_common_large/01` | 1.17 s | 1.19 s |
| `edit/signature_param_common_large/01` | 3.09 s | 3.08 s |

Разница 1.9 s между вставкой сверху и снизу была ценой позиционного
`local_id`: `--mode recompute` на базе исполнял `method_lower`, `method_body`,
`method_cfg` и остальные per-method семейства по всем 2349 методам при вставке
сверху и по одному — при вставке снизу. После яруса оба режима совпадают по
всем 55 семействам до единицы; per-method семейства execute **1**. Общий
остаток обеих вставок равен цене правки сигнатуры: `infer_method` 2368
(интерфейс модуля читается целиком), `method_syntax` 2350 (точка отсечения),
`method_diagnostics` 2350, `doc_see_signature` 2333 — это предмет
следующего шага (проекция интерфейса по имени, ярус 3b).
Эквивалентность: SARIF-сверка `analyze` по ERP против `25163a16` —
0 расхождений на 1 357 665 результатах (чувствительность той же сверки
показана на ярусе 2: +9/−8). ABBA на тихой машине: user CPU 1931/1940 s (A) против 1936/1943 s (B) — +0.2 %, RSS 5.72/5.74 → 5.77/5.79 ГБ (+0.8 %).

### После яруса 3b github#113 (объявление как единица зависимости)

Снято 2026-09-02 на `c83794e3` (release-сборка в чистом воркдереве по sha)
против `7a340248` (ярус 3a) на тех же четырёх точках манифеста.

| Точка | после 3a (`7a340248`) | после 3b (`c83794e3`) |
|---|---:|---:|
| `edit/signature_param_common_large/01` | 3.08 s | **1.08 s** |
| `edit/method_insert_bottom_common_large/01` | 3.07 s | **1.05 s** |
| `edit/method_insert_top_common_large/01` | 3.07 s | **1.06 s** |
| `edit/body_insert_common_large/01` | 1.19 s | 1.05 s |

Три точки, стоившие цену смены интерфейса, сравнялись с правкой тела.
Per-method запросы читали `module_interface` целиком, а его значение
меняется при правке любого объявления файла; теперь метод зависит от
объявлений, которые он читает: своё — по ключу (`interface_method_query`,
`MethodIdInput`), вызываемые — через попадание по имени в тот же ключ
`{имя, 0}`, промах — через множество имён модуля (модули до 256 методов и
все модульные переменные) либо `bool`-мемо по имени (модули крупнее, где
имён много, а переисполнение файла — то, ради чего проекция и существует).
`--mode recompute` на точке сигнатуры: `infer_method` **2** (метод и его
единственный прямой вызывающий — диспетчер по имени процедуры),
`method_diagnostics` 2, `method_arg_diagnostics` 2, `doc_see_signature` 0;
проекции переисполняются на каждый прочитанный ключ и бэкдейтятся
(`module_declares_method` 4507, `interface_method` 2349), `method_syntax`
2349 — точка отсечения parse, как и в точке тела. Вставка снизу — то же с
точностью до единицы; сверху == снизу (гейт 3a держится).

Два промежуточных замера этого яруса провалили память и записаны как
уроки. Мемо на каждый промах по имени (`e9d6e9b3`): 1.9 млн ключей на
lombard, RSS ERP +13 %, и замок LRU на каждом чтении — доля CPU 478 %
против 637 % у базы при равном user CPU. Проекция без кэпа (`d933fce0`):
объявления всех методов остаются живыми после вытеснения интерфейсов —
RSS +11 % при salsa-tracked +190 МБ. Итог на `c83794e3`: память lombard
VmHWM 5 811 → 6 185 МБ (+6.4 %), ключей по имени 254 тыс.
Эквивалентность: SARIF-сверка `analyze` по ERP против `7a340248` — 0
расхождений на 1 357 665 результатах, по lombard с расширениями — 0 на
1 188 708. ABBA на тихой машине (старт при load 0.27): user CPU
1948/1932 s (A) против 1950/1968 s (B) — +1.0 % при разбросе внутри групп
под 1 %, RSS 5.80/5.77 → 5.93/5.98 ГБ (+3.0 %); доля CPU и переключения
контекста у сторон сопоставимы — замка на пути чтения не осталось.

### После яруса 4 github#113 (переразбор одного метода)

Снято 2026-09-03 на `f281af33` (release-сборка в чистом воркдереве по sha)
против `c83794e3` (ярус 3b) на тех же четырёх точках манифеста, тихая
машина (старт при load 0.7).

| Точка | после 3b (`c83794e3`) | после 4 (`f281af33`) |
|---|---:|---:|
| `edit/body_insert_common_large/01` | 1.05 s | **0.83 s** (0.834 / 0.829 / 0.850) |
| `edit/signature_param_common_large/01` | 1.08 s | **0.87 s** |
| `edit/method_insert_top_common_large/01` | 1.06 s | 1.03 s |
| `edit/method_insert_bottom_common_large/01` | 1.05 s | 1.02 s |

Правка внутри метода — тела или заголовка — разбирается как фрагмент
нового текста в границах старого узла и вклеивается в старое дерево
(`parse_outcome: spliced 1`); вставка целого метода лежит вне любого узла и
идёт полным разбором (`refused: OutsideMethod 1`) — это контроль, и он не
ускорился. Спан `parse` в точке тела под `BSL_PROFILE`: **0.95 мс** против
279 мс (плюс скрытый обход дерева ради оценки памяти, 72 мс на этом модуле,
которого больше нет: оценку считает билдер). Остаток 0.83 с — целиком
файловые проходы: строчные и токенные хендлеры файла (`LineLength` 144 мс,
`MissingSpace` 117, `DuplicateStringLiteral` 56) и файловые своды
(`module_call_summary` 68, `module_code_diagnostics` 52, `region_tree` 39,
`module_bodies` 35, `item_tree` 24, `lower_file` 23, `module_interface` 20);
per-method цепочка — единицы миллисекунд. Гейт яруса по точке тела
(≤ 0.80 с) не взят на 4 %: ожидание ≈ 0.68 с считало parse по профилю под
нагрузкой (0.28 + 0.07 с), тихий parse был 0.19 с. `--mode recompute` на точке
сигнатуры — как после 3b: `infer_method` 2, `method_diagnostics` 2,
`method_arg_diagnostics` 2; `method_syntax` переисполняется на 2349 методах
и бэкдейтится сравнением по указателю.

Эквивалентность. SARIF-сверка `analyze` по ERP против `c83794e3` — 0
расхождений на 1 357 665 результатах, по lombard с расширениями — 0.
Тождество на пути `parse_query` под `BSL_REPARSE_VERIFY=1` на четырёх
точках ERP — 0 расхождений (`mismatched 0`, `spliced 1` на теле и
сигнатуре); под проверкой точка тела стоит 1.02 с, то есть полный разбор
на тихой машине — ≈ 0.19 с, а не 0.28 по профилю под нагрузкой. ABBA на тихой машине: user CPU 1960/1977 s (A)
против 1973/1959 s (B) — −0.1 % при разбросе внутри групп под 1 %, RSS
5.98/5.97 → 5.98/5.96 ГБ (±0 %): пакетный режим снимков не пишет, и цена
учёта памяти в билдере не видна.

### После яруса 5 github#113 (плита метода для строчных хендлеров)

Снято 2026-09-03 на `2e063a87` (release-сборка в чистом воркдереве по sha)
против `f281af33` (ярус 4) на тех же четырёх точках манифеста, тихая
машина (старт при load 1.2).

| Точка | после 4 (`f281af33`) | после 5 (`2e063a87`) | гейт яруса |
|---|---:|---:|---:|
| `edit/body_insert_common_large/01` | 0.83 s | **0.34 s** (0.341 / 0.338 / 0.337) | ≤ 0.45 |
| `edit/signature_param_common_large/01` | 0.87 s | **0.38 s** | ≤ 0.50 |
| `edit/method_insert_top_common_large/01` | 1.03 s | **0.53 s** | ≤ 0.60 |
| `edit/method_insert_bottom_common_large/01` | 1.02 s | **0.52 s** | ≤ 0.60 |

Четыре строчных хендлера (`LineLength`, `MissingSpace`,
`IncorrectLineBreak`, `CommentedCode`) считаются по плите метода — тексту
строк, которыми метод владеет монопольно, — и запоминаются по тождеству
метода; строки вне плит собраны в остаток файла. `DuplicateStringLiteral`
в режиме по методу идёт по телу. Профиль точки тела: `slab_layout` 4.9 мс,
`slab_remainder` 4.5 мс, `method_slab` 3.2 мс на 2349 вызовов — 12.6 мс
против гейта 40; спанов `LineLength::check`, `MissingSpace::check` и
`h_*` в окне правки нет. Остаток 0.34 с — `file_diagnostics_query`
317 мс: модульные хендлеры файла (`diagnostic` в сумме 106 мс), файловые
своды (`module_call_summary` 66, `module_code_diagnostics` 51,
`region_tree` 38, `module_bodies` 33, `item_tree` 23, `lower_file` 23,
`module_interface` 20). `--mode recompute` на точке сигнатуры:
`method_slab_query` переисполняется на 2349 методах и бэкдейтится по
равенству текста (validate 0 ниже него); `infer_method`,
`method_diagnostics`, `method_arg_diagnostics` — по 2, как после 3b.

Эквивалентность. SARIF-сверка `analyze` по ERP против `f281af33` — 0
расхождений на 1 357 665 результатах. По lombard с расширениями — 2
расхождения `CommentedCode` в одном файле расширения
(`Documents/АКСЛМБАвизоПоЗаймамИсходящее/Ext/ManagerModule.bsl:2661,2693`):
литерал запроса внутри `#Вставка`, где парсер не строит узел литерала, и
старое древесное правило «комментарий — текст строки» считало `//|`-строки
внутри строки комментариями; лексическое правило (слева `STRING_START` или
`STRING_PART`, справа `STRING_PART` или `STRING_TAIL`) судит их как текст
строки, как и вне `#Вставка`. Это единственный известный класс расхождений.
Тождество мемо-пути: `analyze` по всему ERP под `BSL_SLAB_VERIFY=1`
(плиты, остаток и подъём сверяются с файлом одним блоком на каждом модуле)
— `slab verify mismatches: 0` за 8:02; четыре точки бенча под тем же
флагом — `slab_verify 0`, точка тела под проверкой 0.66 с. ABBA на тихой
машине: user CPU 1955/1989 s (A) против 1867/1867 s (B) — −5.3 % при
разбросе внутри групп до 1.8 %; RSS 5.99/5.97 → 5.98/5.96 ГБ (−0.2 %).
Пакетный путь стал дешевле, а не дороже, хотя платит лишний проход лексера
по каждому файлу: четыре хендлера читают плоский вектор токенов строки
вместо обхода дерева.

Починка `e03f5139` (факты последней строки узла метода снимаются из самого
узла: блок остатка, начатый строкой, на которой метод кончается посреди
строки, терял левый контекст знака) пересчитана на тех же точках: тело
0.35 с (0.349 / 0.351 / 0.347), сигнатура 0.40, вставки 0.56 / 0.56 (старт
при load 4.6); `slab_layout` 7.1 мс; свод ERP под `BSL_SLAB_VERIFY=1` — 0;
SARIF ERP против `f281af33` — 0 расхождений на 1 357 665, lombard — те же 2
`CommentedCode`; один прогон `analyze` ERP: user CPU 1879 s, RSS 5.97 ГБ.

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
