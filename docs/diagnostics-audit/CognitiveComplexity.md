# CognitiveComplexity

Статус: `done`, `needs-code-work`

Дата разбора: 2026-05-07

## Суть правила

Диагностика находит процедуры и функции, у которых когнитивная сложность выше
настроенного порога. В отличие от цикломатической сложности, эта метрика должна
оценивать не количество путей выполнения, а то, насколько тяжело читать
ветвления, вложенность и нелинейные переходы в методе.

Прямой привязки к стандартам 1С нет. Правило опирается на публичную модель
SonarSource Cognitive Complexity.

## Проверенные источники

- Реализация handler'а:
  `crates/ide-diagnostics/src/handlers/cognitive_complexity.rs`.
- Расчет HIR-метрики:
  `crates/hir-def/src/cognitive_complexity.rs`.
- Эмиссия method-scoped diagnostics:
  `crates/hir-def/src/body/lower/mod.rs`,
  `crates/hir-def/src/body.rs`,
  `crates/ide-diagnostics/src/hir_dispatch.rs`.
- Смежная метрика:
  `crates/hir-def/src/cyclomatic_complexity.rs`,
  `crates/ide-diagnostics/src/handlers/cyclomatic_complexity.rs`.
- Конфигурация:
  `docs/configuration/DIAGNOSTICS.md`,
  `docs/configuration/PROJECT_CONFIGURATION.md`.
- Rule-доки:
  `crates/ide-diagnostics/docs/ru/CognitiveComplexity.md`,
  `crates/ide-diagnostics/docs/en/CognitiveComplexity.md`.
- Provenance:
  `docs/legal/diagnostics/CognitiveComplexity.md`.
- Локальный `v8std`:
  `/home/itrous/src/tools_migration/lsp/v8std/docs/diagnostics/bslls/CognitiveComplexity.md`.
- Внешние источники:
  `https://www.sonarsource.com/docs/CognitiveComplexity.pdf`,
  `https://www.sonarsource.com/blog/cognitive-complexity-because-testability-understandability/`,
  `https://community.sonarsource.com/t/inconsistent-cognitive-complexity-calculation-for-logical-operators/136166`.

## Как реализовано

Во время lowering метода строится HIR `Body`, затем сразу считаются две метрики:
`cognitive_complexity::calculate_complexity()` и
`cyclomatic_complexity::calculate_complexity()`. Для cognitive diagnostic
кандидат эмитится только если значение больше нуля, а финальный handler уже
применяет настройку `complexityThreshold` с дефолтом `15`.

Расчет делает рекурсивный обход HIR statements:

- `Если`, циклы, `Исключение` и `#Если` дают `1 + nesting`;
- `ИначеЕсли`, `Иначе`, `#ИначеЕсли`, `#Иначе` дают `+1` без собственного
  штрафа вложенности, но тело обходится глубже;
- `Перейти` дает `+1`;
- тернарный оператор дает `+1`;
- каждый бинарный `И` / `ИЛИ` дает `+1`;
- вложенность применяется только на уровне statements, не expressions.

Диагностика подсвечивает имя метода, quick-fix нет.

## Что покрыто

- отсутствие срабатывания на простой функции;
- один `Если`;
- вложенные `Если` с ростом за nesting;
- `ИначеЕсли` и `Иначе`;
- `Пока`, `Для`, `Для каждого`;
- `Попытка/Исключение`;
- `Перейти`;
- логические `И` / `ИЛИ`;
- глубокая вложенность;
- кастомный `complexityThreshold`;
- интеграционный HIR-путь с diagnostic range на имени метода;
- прямой unit-тест расчета значения для большой функции.

Покрытие хорошее для базовой реализации HIR-метрики и порогового diagnostic
handler'а.

## Пробелы покрытия

- Логические операторы считаются по каждому бинарному `И` / `ИЛИ`. В модели
  Sonar когнитивная сложность должна расти по последовательностям логических
  операторов: `А И Б И В` не должно стоить так же, как смешанная цепочка
  `А И Б ИЛИ В`. Текущие тесты закрепляют более грубую модель.
- `Прервать` и `Продолжить` сейчас не добавляют сложность. В Sonar-описании
  jumps to labels включают `goto`, `break` и `continue`; для BSL нужно явно
  решить, являются ли `Прервать/Продолжить` аналогичными нелинейными
  переходами. Сейчас они точно не покрыты.
- Рекурсивные вызовы не учитываются. Текущая функция расчета получает только
  `Body`, без имени метода и без symbol context, поэтому не может отличить
  обычный вызов от рекурсии.
- Тернарный оператор всегда добавляет только `+1`, потому что expression walker
  не знает statement-level nesting. Тернарник внутри глубоко вложенного `Если`
  будет занижен относительно модели с nesting penalty.
- `Для` не обходит expressions границ `from/to`, а `Для каждого` не обходит
  expression коллекции. Если там есть тернарные или логические выражения, они
  не попадут в cognitive score.
- `Assign` проверяет только значение, но не target expression. Это редко важно
  для BSL, но индексаторы/сложные target expressions могут содержать
  недообследованные выражения.
- Нет тестов на `#Если/#ИначеЕсли/#Иначе`, вложенный тернарный оператор,
  тернарник внутри вложенного statement, рекурсивный вызов, `Прервать`,
  `Продолжить`, `Для` с логическими границами и `Для каждого` со сложной
  коллекцией.
- Streaming metrics в `crates/ide/src/streaming/file_processor.rs` пока
  заполняют `complexity` и `cognitive_complexity` количеством функций. Это не
  ломает саму диагностику, но создает риск расхождения пользовательских метрик
  с diagnostic/code-lens расчетом.

## Может ли инфраструктура улучшить качество

Да. Диагностика уже находится на правильном уровне: HIR дает структурированное
тело метода, а lowering уже знает имя метода, range и тип процедуры/функции.
Основное улучшение — не перенос в AST, а обогащение HIR-метрического слоя:

- общий visitor для method metrics, чтобы `CognitiveComplexity`,
  `CyclomaticComplexity`, `NestedStatements`, `MethodSize` и code lens не
  расходились в обходе HIR;
- expression walker с передачей текущей statement nesting;
- учет логических цепочек как sequences, а не отдельных бинарных узлов;
- опциональный `MethodMetricsContext` с именем метода и доступом к symbols для
  рекурсии;
- переиспользование HIR-метрик в streaming metrics вместо proxy по числу
  методов.

## Возможное объединение

Ближайшие по смыслу диагностики: `CyclomaticComplexity`, `NestedStatements`,
`IfConditionComplexity`, `MethodSize`, `TooManyReturns`, частично
`UsingGoto`.

`CognitiveComplexity` и `CyclomaticComplexity` не стоит объединять в один
внешний `DiagnosticCode`: у них разные цели, разные дефолтные пороги
(`15` против `20`) и разные объяснения для пользователя. Cyclomatic отвечает на
"сколько путей нужно покрыть тестами", cognitive — на "насколько метод тяжело
читать".

Но внутренний расчет стоит объединить. Сейчас `cognitive_complexity.rs` и
`cyclomatic_complexity.rs` имеют почти одинаковые HIR-обходы, и уже видно
расхождение: cyclomatic обходит `For from/to`, `ForEach collection` и assignment
target, cognitive часть этих expressions пропускает. Нужен общий metrics
visitor, который один раз проходит HIR и отдает независимые counters.

## Варианты снятия ограничений

1. Ввести общий `MethodMetricsVisitor` в `hir-def`, который считает cognitive,
   cyclomatic, max nesting и базовые счетчики метода за один HIR-обход.
2. Передавать в expression walker текущий `nesting`, чтобы тернарный оператор
   внутри вложенных блоков получал корректный штраф.
3. Реализовать счетчик логических sequences: одна серия одинаковых `И` или
   `ИЛИ` дает один increment, смена оператора в цепочке дает новый increment.
4. Принять явное решение по `Прервать/Продолжить`: либо считать их
   fundamental increments, либо зафиксировать в docs, почему BSL-реализация
   отличается от Sonar.
5. Расширить контекст расчета именем текущего метода и проверять прямую
   рекурсию; более сложную взаимную рекурсию лучше оставить вне этой
   диагностики.
6. Синхронизировать expression traversal cognitive/cyclomatic для `For`,
   `ForEach`, assignment target, index/field/new/await.
7. Добавить regression tests на логические цепочки, `Прервать/Продолжить`,
   nested ternary, `#Если` и рекурсию.
8. Перевести streaming `FileMetrics` на реальные HIR-метрики или явно
   переименовать proxy-поля, чтобы не смешивать их с diagnostic values.

## Вывод

Диагностика уже полезна и находится на хорошем инфраструктурном уровне: HIR
подходит для method-scoped метрик лучше, чем сырой AST. Главные ограничения не
в handler'е, а в точности самого счетчика. Практичный следующий шаг — общий
HIR visitor для метрик метода и доведение cognitive rules до выбранной модели
Sonar/BSL, особенно для логических sequences, jumps, nested ternary и
рекурсии.
