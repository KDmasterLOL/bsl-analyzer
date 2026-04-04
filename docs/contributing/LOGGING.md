# Руководство по логированию и профилированию

Этот документ описывает, как использовать инфраструктуру логирования и
профилирования в `bsl-analyzer`.

## Переменные окружения

| Переменная | Назначение | Пример |
|------------|------------|--------|
| `BSL_LOG` | фильтрация логов через targets syntax | `hir_def=debug,cfg=warn` |
| `BSL_PROFILE` | иерархическое профилирование | `*@3>10` |
| `BSL_PROFILE_JSON` | выгрузка профиля в JSON | `*` |
| `BSL_LOG_FILE` | запись логов в файл | `/tmp/bsl.log` |

## Уровни логирования

Используйте уровень логирования в зависимости от сценария:

| Уровень | Когда использовать |
|---------|--------------------|
| `error` | невосстановимые ошибки, влияющие на работоспособность |
| `warn` | восстановимые ошибки и деградация функциональности |
| `info` | крупные этапы выполнения: загрузка, завершение анализа, запуск сервера |
| `debug` | подробности для отладки обычного выполнения |
| `trace` | очень подробная трассировка: отдельные узлы AST, итерации, низкоуровневые шаги |

## Использование `tracing` в коде

### Базовое логирование

```rust
use tracing::{trace, debug, info, warn, error};

// Простые сообщения
info!("Analysis started");
debug!("Processing file");

// Структурированные поля
info!(file_count = files.len(), "Loading files");
debug!(file_id = ?file_id, method = %method_name, "Lowering method");
```

### Спаны для профилирования

Для измерения длительности операций используйте `span`:

```rust
pub fn expensive_operation(input: &str) -> Result {
    let _span = tracing::info_span!("expensive_operation", len = input.len()).entered();
    // ... код операции
}
```

Если задан `BSL_PROFILE`, время выполнения спанов измеряется автоматически.

### Правила именования спанов

- используйте `snake_case`
- передавайте важный контекст через поля спана
- выбирайте короткие, но понятные имена

```rust
// Хорошо
let _span = tracing::info_span!("lower_method", method_id = ?id).entered();
let _span = tracing::info_span!("parse_file", file_id = ?file_id, len = input.len()).entered();

// Плохо: слишком многословно
let _span = tracing::info_span!("lowering_method_body_to_hir_representation").entered();
```

## Профилирование

### Иерархическое профилирование (`BSL_PROFILE`)

Синтаксис фильтра: `pattern@depth>threshold_ms`

Примеры:

```bash
# Профилировать все операции
BSL_PROFILE='*' cargo run -- analyze ~/project

# Только операции дольше 50ms, глубина 2
BSL_PROFILE='*@2>50' cargo run -- analyze ~/project

# Профилировать только выбранные операции
BSL_PROFILE='parse|analyze' cargo run -- analyze ~/project
```

Пример вывода:

```text
  112ms  cli_analyze
    45ms  load_files
    67ms  run_diagnostics
      23ms  check_file (×6540)
```

### JSON-профилирование (`BSL_PROFILE_JSON`)

Для инструментальной обработки можно писать профиль в JSONL:

```bash
BSL_PROFILE_JSON='*' cargo run -- analyze ~/project 2>timing.jsonl
```

Пример вывода:

```json
{"name":"cli_analyze","elapsed_ms":112}
{"name":"load_files","elapsed_ms":45}
```

## Фильтрация по компонентам

Логи можно фильтровать по crate/module через targets syntax:

```bash
# Только debug-логи из hir-def
BSL_LOG=hir_def=debug cargo run

# Несколько фильтров сразу
BSL_LOG=hir_def=debug,cfg=warn,dataflow=info cargo run

# Общий debug + trace для конкретного модуля
BSL_LOG=debug,hir_def::body::lower=trace cargo run
```

## Практические рекомендации

### Делайте

- ставьте `span` вокруг потенциально дорогих операций (>1ms)
- добавляйте в поля спана полезный контекст: идентификаторы, размеры, счётчики
- используйте `debug!` для штатных технических деталей
- используйте `info!` экономно, только для крупных этапов

### Не делайте

- не используйте `println!` / `eprintln!` для отладки — вместо этого используйте `tracing`
- не создавайте `span` для тривиальных операций (<100μs)
- не логируйте чувствительные данные: содержимое файлов, токены, пароли, креды
- не ставьте `info!` на каждый элемент в больших циклах

## Профилирование памяти и CPU

Для более детальных измерений можно использовать `StopWatch` из crate
`profile`:

```rust
use profile::StopWatch;

let sw = StopWatch::start();
// ... операция
let span = sw.elapsed();
println!("{}", span); // "123ms, 456ki, 789kb"
                      // время, инструкции CPU, изменение памяти
```

Что умеет `StopWatch`:

- CPU instructions (только Linux, через `perf_event`)
- изменение памяти (Linux glibc, Windows или при включённом `jemalloc`)
- время выполнения (все платформы)
