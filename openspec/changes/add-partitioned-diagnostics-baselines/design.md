## Context

`add-diagnostics-baseline` реализовал один schema v1 JSON-файл. Его `scope` содержит
нормализованный `source_root` и всю упорядоченную топологию расширений
`{name, path, depends_on}`. Общий классификатор в `ide` сопоставляет диагностики после
правил и штатных подавлений; CLI владеет записью, `ide-host-core` — снимком файла, а
MCP/LSP — наблюдением и перезагрузкой без пересоздания Salsa.

Эта архитектура семантически корректна, но один крупный файл плохо масштабируется как
артефакт владения. Набор из 1 596 175 записей занимает около 1,29 ГБ. Требуется
разделить хранение и сопровождение, не разделяя анализ: все исходные корни по-прежнему
загружаются в одну базу, а расширение анализируется с видимостью основной конфигурации
и своей dependency closure.

## Locked Decisions

- Источник истины — текущий checkout и завершённый change `add-diagnostics-baseline`;
  установленный из VS Code бинарник 0.2.68 не определяет контракт.
- Семантический анализ остаётся единым для полной нормализованной топологии. Отдельный
  запуск анализатора для расширения запрещён.
- Все настроенные partition применяются одновременно как один снимок.
- Одиночный `[diagnostics.baseline].path` остаётся обратно совместимым.
- Отпечаток диагностики, порядок штатных подавлений и защита
  `UnknownSuppressionCode`/`SuppressionWithoutCode` остаются общими для CLI, MCP, LSP
  и репортёров.
- Обычный анализ, MCP и LSP не изменяют базовые линии; принятие новых диагностик
  возможно только явной CLI-командой.
- Удалённое хранилище, UI и автоматическое принятие новых диагностик не входят в change.

## Goals / Non-Goals

**Goals:**

- независимые логические partition для main, каждого расширения и явных групп;
- минимальная конфигурация без повторного объявления основной топологии и параметров
  диагностик;
- ровно один владелец каждой текущей и сохранённой диагностики;
- атомарная публикация согласованного набора файлов;
- общая и per-partition классификация `new`/`known`/`resolved`;
- безопасная явная миграция schema v1;
- bounded-memory загрузка порядка 1,6 млн записей;
- перезагрузка одного partition без пересоздания Salsa.

**Non-Goals:**

- удалённое, сетевое или объектное хранилище;
- UI, фоновые задания принятия или автоматическое обновление;
- отдельная Salsa-база, процесс или анализ для каждого partition;
- эвристика переименований файлов/расширений;
- разные правила диагностик или разные рецепты отпечатка по partition;
- изменение `analysis.diff_base`, `search.baseline` или source suppression;
- межпроектное разделение одного набора baseline;
- гарантия транзакции между файловой системой и внешней системой контроля версий.

## Configuration Contract Alternatives

### Вариант A — автоматический набор по source roots

```toml
[diagnostics.baseline]
directory = ".bsl-analyzer/diagnostics-baselines"

[[diagnostics.baseline.groups]]
name = "vendor"
extensions = ["VendorCore", "VendorReports"]
```

`main` и все расширения выводятся из уже проверенной `ExtensionTopology`. Расширение,
не включённое в группу, получает собственный partition. Группа является только
исключением из автоматического правила: её участники получают один общий partition и
не получают параллельные индивидуальные partition.

Плюсы: одна новая обязательная настройка, нет повторения путей/dependencies, добавленное
расширение автоматически появляется в ожидаемом наборе, диагностические параметры
остаются общими. Минусы: нужен детерминированный внутренний layout и manifest; имена
физических файлов не задаются вручную.

### Вариант B — явное сопоставление partition с файлами

```toml
[diagnostics.baseline.partitions]
main = "baselines/main.json"

[[diagnostics.baseline.partitions.extensions]]
name = "VendorCore"
path = "baselines/vendor-core.json"

[[diagnostics.baseline.partitions.groups]]
name = "vendor"
extensions = ["VendorCore", "VendorReports"]
path = "baselines/vendor.json"
```

Плюсы: полный контроль имён и расположения файлов. Минусы: каждое расширение нужно
объявить второй раз, добавление/переименование легко пропустить, явные mappings могут
расходиться с `source.extensions`, а атомарное переключение набора всё равно требует
manifest или журнала.

### Выбор

Выбран вариант A. Он является минимальным декларативным контрактом и использует
существующую топологию как единственный источник имён, путей и зависимостей. Вариант B
не даёт новой семантической возможности, но создаёт второй реестр расширений и больше
ошибочных состояний.

Поля `path` и `directory` взаимоисключающие. `path` включает прежний одиночный режим;
`directory` — partitioned-режим. `groups` допустимы только вместе с `directory`.
Неизвестное расширение, пустая группа, повторное членство, совпадающие имена групп и
case-fold collision являются ошибками `check-config` до анализа.

## Decisions

### 1. Partitioning выполняется после одного семантического анализа

`Project` продолжает строить одну `ExtensionTopology`, один набор source roots и одну
Salsa-базу. Маршрутизатор partition использует root identity каждого `WalkedFile`, а не
повторный строковый поиск по пути. Основной root соответствует `main`; root расширения
соответствует его topology node и затем индивидуальному либо групповому partition.

Кандидат диагностики получает `PartitionId` только после вычисления диагностики общим
`ide`. Отпечаток не включает partition: это сохраняет parity с Code Quality и позволяет
без изменения идентичности разложить schema v1. Разные partition не могут владеть
одним root. Неизвестный root, один файл под несколькими roots или отсутствие правила
в partitioned-режиме являются fail-closed ошибкой, а не активной диагностикой без
baseline.

Имена CLI стабильны и однозначны:

- `main`;
- `extension:<точное имя из topology>`;
- `group:<имя группы>`.

Сопоставление ссылок на расширения выполняется по существующему case-fold правилу
`ExtensionTopology`, но опубликованный id сохраняет каноническое объявленное имя.

### 2. Идентичность partition выводится из нормализованного проекта

Partition-файл schema v2 содержит только `schema_version`, `partition` и
`diagnostics`. Полный portable scope хранится один раз в manifest.

- `main`: `{kind: "main", path: <source_root>}`;
- extension: `{kind: "extension", name, path, depends_on}`;
- group: `{kind: "group", name, members: [{name, path, depends_on}, ...]}`.

Members группы упорядочены топологически. `depends_on` хранит прямые зависимости,
включая зависимости на расширения за пределами группы. Таким образом идентичность
фиксирует требуемые main/name/path/dependencies, но не копирует всю топологию в каждый
файл.

Manifest `project_scope_fingerprint` — BLAKE3 канонического переносимого представления
`{source_root, ordered extensions{name,path,depends_on}}` с отдельной версией рецепта.
Абсолютные пути не входят в него. Любое переименование, изменение пути, dependencies
или порядка меняет scope fingerprint всего набора. Файл partition остаётся пригодным
для повторного использования, только если его собственная identity и hash не изменились;
совместимость полного набора всегда определяет новый manifest.

### 3. Правила групп образуют полное непересекающееся покрытие

Алгоритм планирования:

1. создать partition `main` для основного root;
2. проверить группы и пометить каждого участника ровно одной группой;
3. создать group partition для каждой группы;
4. создать extension partition для каждого непомеченного расширения;
5. построить двунаправленную таблицу `root identity <-> PartitionId`;
6. отвергнуть одинаковые, вложенные или иным образом неоднозначные source roots.

Группы не меняют dependency graph и не ограничивают видимость. Они меняют только
владельца baseline-записей. Main нельзя включить в группу. Пустой проект без
расширений всё равно имеет один `main` partition.

### 4. Manifest атомарно публикует набор content-addressed файлов

Нельзя атомарно заменить несколько независимых имён обычной последовательностью
`rename`. Поэтому `directory` является управляемым layout, где неизменный partition
сохраняет прежний путь между публикациями:

```text
<directory>/
  manifest.json
  objects/
    main/<blake3>.json
    extensions/<partition-key>/<blake3>.json
    groups/<partition-key>/<blake3>.json
  .baseline.lock
```

`manifest.json` содержит собственную версию схемы, `generation`,
`project_scope_fingerprint` и упорядоченный список `{partition_id, file, blake3}`.
Идентичность partition находится в самом partition-файле; manifest её не дублирует.
`generation` детерминированно вычисляется из scope fingerprint и упорядоченных хэшей
partition-файлов. `partition-key` — lowercase hex BLAKE3 канонических UTF-8 bytes
`PartitionId`; display name остаётся в identity/manifest. Фиксированный ASCII-компонент
не зависит от Unicode normalization, регистра файловой системы, Windows reserved names,
trailing dot/space, `/`, `\` или `%`; совпадение ключей разных ids проверяется и
отклоняется до записи.

Изменяющая CLI-команда захватывает межпроцессную эксклюзивную блокировку через уже
используемый в `bsl-analyzer` стандартный `File::try_lock`; новый FFI/dependency не
вводится, занятая блокировка немедленно возвращает `busy`. Lock-file имеет стабильный
путь и никогда не удаляется/не заменяется. Затем команда
записывает только отсутствующие content-addressed objects во временные соседние файлы,
синхронизирует object и доступные directory entries, повторно читает и атомарно
публикует каждый object. Временный manifest полностью записывается и синхронизируется,
а готовый manifest публикуется единственной атомарной заменой после повторной проверки
исходного generation. Эта замена — commit point: до неё читатели видят старый набор,
после — новый; последующая ошибка sync/cleanup становится предупреждением успешной
операции.
Контракт гарантирует process-visible atomicity, но не обещает durability после внезапной
потери питания/отказа накопителя на файловой системе без такой гарантии.
Параллельный писатель получает `concurrent_update`/занятую блокировку и ничего не
переключает. Существующий object переиспользуется только после проверки hash и bytes;
несовпадение является fail-closed ошибкой обычной операции. Только явный
`create --partition` может атомарно заменить повреждённый object, если регенерация
побайтово подтверждает hash, уже записанный в неизменяемом manifest.

Сбой любого object до переключения сохраняет старый набор. После commit команда
best-effort удаляет только временные файлы текущей операции и точно известные прежние
object paths из исходного manifest, более не указанные новым manifest. Сбой этой
очистки не меняет результат и оставляет orphan; произвольное сканирование, повторная
уборка следующей командой, фоновый cleaner и обещание хранения версий не вводятся.

Selected update строит новый полный manifest: неизменённые content-addressed paths и
hashes переиспользуются напрямую, без hard link, копирования или повторной сериализации.
Опубликованные objects никогда не редактируются на месте; repair использует атомарную
замену теми же ожидаемыми bytes.

Читатель дважды наблюдает manifest: загружает manifest, проверяет каждый перечисленный
hash/schema/identity, затем убеждается, что manifest не сменился. При гонке он делает
ограниченный повтор; смешанный снимок не устанавливается.

### 5. Форматы и миграция

Одиночный schema v1 остаётся неизменным в режиме `path`. Partitioned-файл использует
schema v2:

```json
{
  "schema_version": 2,
  "partition": {"kind": "extension", "name": "Ext", "path": "src/cfe/Ext", "depends_on": []},
  "diagnostics": []
}
```

Порядок и формат записей остаются детерминированными. Рецепт diagnostic fingerprint
schema v1 не меняется. Manifest имеет независимую `schema_version = 1`, чтобы layout и
формат partition развивались независимо.

Миграция явная:

```text
bsl-analyzer-app diagnostics baseline create -s . --from-v1 <old-file>
```

Команда допустима только в configured `directory`-режиме, требует полного анализа и
обычного файла без symlink/reparse point внутри канонического корня проекта,
строго проверяет v1 scope против текущей топологии, маршрутизирует каждую старую запись
по root owner и публикует полный schema v2 набор. Она не добавляет текущие новые
диагностики и не удаляет resolved-записи: первая последующая `check` показывает тот же
долг, что показал бы v1. `--from-v1` несовместим с `--partition`.

Автоматическое переписывание v1 запрещено. Переход обратно состоит в возврате `path`;
partitioned schema v2 не склеивается в v1 автоматически. Миграция побайтово не меняет
source v1. Такой rollback восстанавливает только pre-migration snapshot: diagnostics,
принятые последующими v2 update, будут потеряны до повторной миграции/обновления.

### 6. Классификация объединяет индексы, но сохраняет владение

Загрузчик создаёт `DiagnosticsBaselineSetSnapshot` с таблицей
`PartitionId -> Arc<PartitionSnapshot>`. Общий fingerprint-map не создаётся: root-owner
router сначала выбирает partition, затем выполняется lookup в его компактном индексе.
Все partition применяются одновременно.
Перед установкой снимка проверяются:

- полный ожидаемый набор partition;
- scope fingerprint и точная identity каждого partition;
- hash из manifest;
- отсутствие дубликатов identity/fingerprint внутри partition; межфайловый дубликат
  выявляется как нарушение root owner при проверке каждой сохранённой записи;
- принадлежность каждой сохранённой записи root своего partition;
- отсутствие защитных диагностик.

Ошибки нормализуются детерминированно: сначала сравниваются множества partition ids
(`missing_partition`, затем `orphan_partition`, по id), затем identities общих id,
hash/schema/content, и только при совпавших множествах/identities отдельное неверное
значение manifest scope даёт `scope_mismatch`. Это исключает разные коды для одного
изменения topology.

Каждая текущая диагностика маршрутизируется в partition, получает прежний fingerprint
и сравнивается только с индексом владельца. Общая сводка является суммой
per-partition-сводок, но `state` и `complete` вычисляются отдельно, а не суммируются.
Ошибка одного partition делает общий state `error`; ответ содержит ошибку конкретного
partition и не фильтрует результат частично загруженным набором. Обычный CLI analyze
завершается без отчёта, MCP возвращает прежнюю error-ветвь без findings и
классификационных счётчиков. Только LSP работает fail-open и публикует все текущие
диагностики.

### 7. Coverage и resolved вычисляются по partition

`CoverageProof` расширяется множеством полностью завершённых файлов с owner partition.

- Partition `Full`, если завершены все его файлы, общий семантический снимок актуален и
  нет unreadable/failed/cancelled/truncated/out-of-scope условий для этого partition.
- Partition `Partial` иначе; `resolved` считается только для его завершённых целых
  файлов.
- При stale, незавершённой или неуспешной reload completed set пуст для всех partition,
  поэтому `resolved = 0`.
- Общий `Full/complete=true` возможен только когда каждый ожидаемый partition `Full`.
- `disabled` остаётся `complete=true`; `error` всегда `complete=false`.

Фильтры диапазона, кода, severity, бюджета и строк diff не доказывают завершение файла.
Группа суммирует покрытие файлов всех участников и не считается полной, если неполон
хотя бы один member root.

### 8. CLI работает со всем набором или выбранным partition

Команды сохраняют прежние имена и добавляют общий необязательный селектор:

```text
diagnostics baseline create [--partition <id>] [--from-v1 <path>]
diagnostics baseline check  [--partition <id>]
diagnostics baseline update [--partition <id>]
```

Без selector операция относится ко всему ожидаемому набору. С selector анализатор всё
равно загружает единую топологию и получает полный coverage proof проекта; он только
ограничивает сравнение и изменение выбранным владельцем.

Для всех трёх CLI-команд, включая selected-варианты, обязателен глобальный
`CoverageProof::Full`. Per-partition `Full`/`Partial` применяется к обычному analyze,
MCP и LSP, но не разрешает CLI менять набор по частичному анализу.

- `create` без selector создаёт первый полный набор и требует отсутствия manifest.
- `create --partition` является узкой repair-операцией, сохранённой из-за явного
  требования selected create: только при совпадении текущего scope/plan с manifest она
  восстанавливает перечисленный в manifest отсутствующий или повреждённый object, если
  повторно сформированные bytes дают уже записанный hash. Manifest и
  baseline-состав не меняются. Создать неполный первый набор, принять новую диагностику
  или изменение topology выбранной операцией нельзя.
- `check` всегда read-only; выбранный режим не требует чистоты других счётчиков, но
  требует корректной загрузки всего набора.
- `update --partition` заменяет только логическое содержимое выбранного partition и
  переносит остальные неизменными в новое поколение.
- неизвестный selector — ошибка до анализа и записи.

Машинный результат содержит `operation`, `generation`, общую сводку и упорядоченный
массив per-partition результатов. Строгий код `check` успешен, когда в выбранной области
нет `new` и `resolved` и coverage полный.

### 9. Изменения topology являются явными несовместимостями

- Добавление расширения создаёт новый ожидаемый partition. Текущий набор получает
  `missing_partition`; только `update` без selector публикует новый полный plan.
- Удаление расширения оставляет orphan partition в старом manifest и меняет scope
  fingerprint. Анализ и `check` завершаются `orphan_partition`; явный `update` без
  selector строит manifest текущей topology и удаляет orphan из активного набора.
  Физические orphan objects не являются активными после switch.
- Переименование, смена path или dependencies является remove+add, а не эвристическим
  переносом. Требуется `update` без selector; новые текущие диагностики не считаются
  известными по старому partition автоматически.
- Изменение состава/имени группы также меняет partition identities и требует того же
  полного update. Selected create/update при любом отличии scope/plan от manifest
  завершается ошибкой до записи.

Сам вызов изменяющего `update` без selector является явным принятием нового plan;
дополнительный флаг подтверждения не вводится. До записи команда показывает topology
diff и итоговые счётчики, а новые diagnostics принимает только в рамках обычной
семантики явно запущенного `update`.

### 10. MCP и LSP перечитывают набор независимо от Salsa

MCP/LSP наблюдают `manifest.json` и файлы активного поколения. Смена manifest вызывает
загрузку нового набора; изменение активного partition вне штатной транзакции меняет
его observation/hash и переводит весь набор в error. Неизменившиеся partition
переиспользуются по `Arc` и hash. `AnalysisHost`/Salsa, VFS source roots и semantic
indexes не заменяются.

Set epoch вычисляется из manifest bytes и состояний ошибок. Он входит в file/workspace
MCP `result_id`. MCP `diagnostics` повышает schema/outputSchema version с 13 до 14 и
всегда возвращает общую
`baseline`, внутри которой находятся `partitions_total`, `partitions_returned` и
`partitions_truncated`. Упорядоченный по id `baseline.partitions[]` заполняется в
пределах бюджета; file-запрос приоритетно включает owner partition, если хотя бы одна
partition detail помещается. При минимальном бюджете массив может быть пустым, но общая
сводка и три поля усечения обязательны. Error-ветвь дополнительно возвращает первую по id ошибку и
`errors_total`; увеличение `max_output_tokens` раскрывает больше partition/error
details. LSP публикует только new и защитные диагностики. При ошибке набора LSP
fail-open публикует все текущие диагностики и показывает одно уведомление на
`{partition_id, error_epoch}`; recovery молчалив.

Partition id ограничен 64 байтами UTF-8: это сохраняет точную идентичность первой
ошибки в минимальном поддерживаемом MCP-бюджете 256 токенов без усечения или коллизий.
Меньшее явно заданное `max_output_tokens` отклоняется как некорректный параметр.

Переход Ready→Error, Error→Ready или смена set epoch повторно классифицирует и
публикует все открытые документы и активную workspace batch, потому что изменение
валидности одного partition меняет применимость всего набора. Переиспользование `Arc`
не сужает эту область публикации.

### 11. Репортёры сохраняют существующую форму активных диагностик

Console, JSON, JSONL, SARIF и JUnit добавляют per-partition массив рядом с общей
сводкой. Поля старой общей сводки сохраняются. SARIF не выставляет `baselineState` для
Partial. JUnit не меняет число tests/failures. GitLab Code Quality остаётся корневым
массивом активных замечаний без manifest/summary elements и использует прежний общий
fingerprint. Отчёт не дублирует диагностику main в partition расширения.

Общий машинный объект расширяется без переименования legacy-полей:

```json
{
  "baseline": {
    "state": "full", "new": 1, "known": 2, "resolved": 3,
    "path": ".bsl-analyzer/diagnostics-baselines", "schema_version": 2,
    "manifest_schema_version": 1, "complete": true,
    "partitions": [{
      "id": "extension:Ext", "identity": {},
      "path": "objects/extensions/<partition-key>/<hash>.json",
      "schema_version": 2, "state": "full", "new": 1,
      "known": 2, "resolved": 0, "complete": true
    }]
  }
}
```

JSON хранит этот объект в root `baseline`, JSONL — в `done.baseline`, SARIF — в
`runs[].properties.baseline`, JUnit — JSON-значением существующего property
`diagnostics.baseline`. Console печатает общую строку и строки partition. Code Quality
не получает ни одного из этих элементов.

CLI operation-result сохраняет `operation`, `path`, `success`, `added`, `removed`,
`unchanged`, `diagnostics`; `path` равен configured directory. Без selector counts и
`diagnostics` имеют прежнюю семантику, агрегированную по всем partition. С selector они
относятся только к выбранному partition, а additive `selected_partition` содержит его
id; `partitions` при этом всё равно возвращает сводки полного проверенного набора.
Исключение — `create --from-v1`: он сохраняет поле `diagnostics`, но возвращает пустой
массив и только итоговый `added`, чтобы миграция не материализовала весь вход ради
вывода.

### 12. Пути и файловая граница доверия

`directory` разрешается относительно каталога origin-файла конфигурации ровно как
legacy `path`; только программно созданная конфигурация использует project root.
CLI-путь `--from-v1` разрешается относительно project root. `directory`, путь
`--from-v1`, manifest, object paths и partition files должны находиться внутри
канонического корня проекта. Абсолютные пути, `..`, пустые компоненты, обратные слэши в
хранимом POSIX-представлении, symlink/reparse point на любом управляемом компоненте и
case-fold collision запрещены. Читатель открывает только файлы, перечисленные manifest,
после проверки canonical containment. Запись использует только новый проверенный
временный соседний файл и не следует ссылкам. Чужие файлы вне управляемых `objects/`
не удаляются. Все open/create/rename/unlink выполняются относительно
закреплённого handle каталога с no-follow/reparse проверкой каждого компонента;
предварительная строковая canonical-проверка без безопасного открытия недостаточна.

### 13. Производительность и память

Partitioning не уменьшает число известных диагностик, применяемых одновременно,
поэтому формат сам по себе не является достаточной оптимизацией памяти. Загрузчик:

- читает через `BufReader`/streaming deserializer и не держит одновременно сырые bytes
  всех partition;
- декодирует fingerprint из hex в `[u8; 32]`;
- интернирует повторяющиеся path/code/partition ids;
- строит один компактный индекс без объединённого `Vec<DiagnosticsBaselineEntry>`;
- перечитывает детали resolved-записей только для CLI-вывода, когда они нужны;
- ограничивает параллелизм загрузки, чтобы peak зависел от индекса и крупнейшего
  partition, а не от суммы сырых JSON;
- при reload переиспользует неизменившиеся `Arc<PartitionSnapshot>`.

Миграция schema v1 разбирает исходный файл один раз и сразу пишет каждую запись во
временный файл её владельца. Публикация принимает уже синхронизированные и
хешированные файлы, поэтому миграция не создаёт общий raw-bytes/rich-entry vector и
не загружает опубликованный набор повторно. Её результат сообщает счётчики и
generation; массив `diagnostics` намеренно пуст, чтобы машинный ответ не возвращал
миллионы записей и не нарушал тот же предел памяти.

Приёмочный Linux-тест генерирует 1,6 млн записей, проверяет полный и selected lookup и
peak RSS не более 1,5 размера входного набора сверх resident до загрузки. Тестовые
счётчики доказывают линейную структуру работы: каждая baseline-запись разбирается не
более одного раза, каждая текущая диагностика получает fingerprint не более одного
раза, а reload не вызывает loader неизменившихся partition. Отдельный microbenchmark
может измерять throughput, но не является шлюзом и не требует будущего продуктового
решения о числовом пороге времени.

## Audit Matrix

| Область | Решение | Доказательство |
|---|---|---|
| Correctness | один semantic snapshot, root-owner router, disjoint coverage | topology + cross-extension integration tests |
| Compatibility | `path`/schema v1 unchanged; additive reporter fields | legacy contract and golden tests |
| Reliability | immutable objects + atomic manifest switch + writer lock | fault-injection/concurrency tests |
| Security | canonical containment, no links, hash verification | path/link/collision adversarial tests |
| Performance | streaming parse, compact fingerprint, Arc reuse | 1,6M Linux RSS test and reload allocation test |
| Operability | stable ids, all/selected commands, full update for topology change | CLI end-to-end tests |
| Protocols | bounded common + per-partition summaries and set epoch | MCP schema/budget, LSP publication tests |
| Reports | existing active findings and counts preserved | JSON/JSONL/SARIF/JUnit/Code Quality goldens |

**Audit verdict:** архитектура готова к реализации при условии, что manifest является
единственной точкой публикации и partitioning не проникает в семантическую загрузку.
Прямая последовательная замена стабильных файлов и отдельные analyzer runs считаются
блокирующими отклонениями от design.

## Risks / Trade-offs

- Общий компактный индекс всё равно масштабируется с суммарным числом записей; 1,6 млн
  остаётся обязательным реальным нагрузочным доказательством.
- Полный update временно требует места для всех новых content-addressed objects;
  selected update создаёт object только выбранного partition. Hard links и generation
  directories не используются.
- Orphan objects после сбоя точечной cleanup занимают место; автоматического сканера
  нет, поэтому их удаление остаётся явной пользовательской операцией.
- Legacy строковые extension entries могут иметь неоднозначные имена. Partitioned-режим
  требует однозначных topology names и предложит перевести такие entries в structured.
- Вложенные source roots ранее могли сканироваться, но не имеют однозначного владельца;
  partitioned-режим отвергает их.
- `update --partition` сохраняет полный semantic cost. Это осознанная цена отсутствия
  ложных семантических диагностик.
- Повреждение одного активного файла делает весь набор error. Частичное подавление по
  оставшимся файлам было бы опасным и не допускается.
- Windows semantics открытых файлов и directory cleanup требуют отдельного CI-теста;
  корректность commit point не должна зависеть от успешной точечной уборки прежних objects.

## Migration Plan

1. Добавить partition planner и schema v2 рядом с неизменным schema v1.
2. Реализовать set loader/compact index и доказать parity fingerprint/classification.
3. Реализовать content-addressed transaction и CLI all/selected/migrate.
4. Подключить обычный analyze и репортёры с legacy contract tests.
5. Подключить MCP/LSP set reload без замены Salsa.
6. Прогнать 1,6M performance gate, Windows transaction tests и полный regression suite.
7. Документировать переход: заменить `path` на `directory`, выполнить
   `create --from-v1`, проверить `check`, затем удалить старый v1-файл отдельным
   пользовательским изменением после ревью.

Откат до релиза состоит в возврате `path` на сохранённый неизменный v1-файл и явно
восстанавливает только pre-migration snapshot. Partitioned directory можно оставить
неактивным; анализатор его не читает в legacy-mode.

## Execution Plan

1. Контракт и ownership planner.
2. Форматы, set loader, fingerprint parity и миграция.
3. CLI transaction и all/selected semantics.
4. Analyze/reporters.
5. MCP/LSP reload и schemas.
6. Performance, документация и полные шлюзы.

Каждый следующий этап зависит от автоматических доказательств предыдущего; MCP/LSP не
начинаются до стабилизации общей модели snapshot и commit point.

## Assumptions and Open Questions

Блокирующих открытых вопросов нет. Partitioned-режим требует structured уникальные
имена расширений; это фиксированное правило нового режима, а legacy `path` остаётся
без изменений. Производительность принимается по функциональным счётчикам линейного
прохода и RSS-шлюзу. После commit удаляются только точно известные прежние objects;
orphan после неуспешной cleanup удаляет пользователь.

## Exact Wording Fixes Relative to `add-diagnostics-baseline`

- «один снимок/один файл» в старом design трактуется как legacy-mode; новый set snapshot
  заменяет его только при `directory`.
- «область проекта хранится в baseline» для schema v2 уточняется: portable scope
  fingerprint хранится один раз в manifest, а partition-файл содержит только свою
  identity; полная topology остаётся в project config/model.
- «изменение файла меняет epoch» расширяется до изменения manifest, любого активного
  partition либо error state всего набора.
- «атомарная запись файла» расширяется до атомарной публикации набора через manifest;
  последовательные rename нескольких активных файлов не удовлетворяют требованию.
