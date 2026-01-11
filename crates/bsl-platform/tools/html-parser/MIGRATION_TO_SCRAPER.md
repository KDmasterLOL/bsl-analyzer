# План миграции HTML парсера на библиотеку scraper v0.20

## Текущее состояние

**Текущий подход:**
- Ручной поиск строк (`find()`, substring slicing)
- Уязвим к изменениям в HTML структуре
- Проблемы с UTF-8 границами при slicing
- Множество условных проверок и fallback логики
- Сложно поддерживать и расширять

**Достигнутые результаты:**
- 6,666 методов извлечено
- 91.8% параметров с типами (6,498/7,074)
- 99.95% методов с контекстом (6,663/6,666)
- 62.8% методов с return types (4,184/6,666)

## Целевое состояние (с scraper)

**Преимущества:**
- ✅ CSS селекторы вместо ручного поиска
- ✅ Браузерный HTML5 парсер (html5ever от Servo)
- ✅ Надёжная обработка невалидного HTML
- ✅ Автоматическая обработка UTF-8
- ✅ Простота чтения и поддержки кода
- ✅ Стандартный подход для Rust экосистемы

**Библиотека:**
- Имя: `scraper`
- Версия: `0.25.0` (latest stable)
- Репутация: High (155,996+ примеров кода)
- Основа: `html5ever` + `selectors` от Servo

## Этапы миграции

### Этап 1: Подготовка (30 мин)

**1.1. Добавить зависимость**
```toml
# crates/bsl-platform/tools/html-parser/Cargo.toml
[dependencies]
scraper = "0.25.0"
```

**1.2. Изучить примеры scraper**
- Базовый парсинг: `Html::parse_fragment()`
- CSS селекторы: `Selector::parse()`
- Итерация: `html.select(&selector)`
- Извлечение текста: `element.text().collect::<String>()`
- Атрибуты: `element.value().attr("class")`

### Этап 2: Создать новую версию парсера (2 часа)

**2.1. Создать файл `src/scraper_parser.rs`**

Параллельная реализация рядом с текущей для сравнения результатов.

**2.2. Определить CSS селекторы для основных элементов**

```rust
// Заголовки глав
const CHAPTER_SELECTOR: &str = "p.V8SH_chapter";

// Параметры метода
const RUBRIC_SELECTOR: &str = "div.V8SH_rubric";

// Ссылки на типы
const TYPE_LINK_SELECTOR: &str = "a[href^='v8help://']";
```

**2.3. Переписать функции извлечения**

Основные функции для переписывания:
1. `extract_version()` - извлечение версии
2. `extract_context()` - доступность (контекст)
3. `extract_return_type()` - возвращаемое значение
4. `extract_parameters()` - параметры методов

### Этап 3: Реализация функций (3 часа)

#### 3.1. `extract_context()` - Самая проблемная сейчас

**Текущий подход (сложный):**
```rust
// Ищем "Доступность:", потом ищем </p>, потом следующий <p>
if let Some(start) = html.find("Доступность:") {
    let after_start = &html[start..];
    if let Some(p_start) = after_start.find("</p>") {
        // ... еще несколько уровней вложенности
    }
}
```

**С scraper (простой):**
```rust
use scraper::{Html, Selector};

fn extract_context_scraper(html_content: &str) -> Option<ContextAvailability> {
    let html = Html::parse_fragment(html_content);

    // Найти заголовок "Доступность:"
    let chapter_sel = Selector::parse("p.V8SH_chapter").unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Доступность") {
            // Взять следующий элемент <p> после chapter
            if let Some(next_p) = chapter.next_sibling()
                .and_then(|n| n.value().as_element())
                .filter(|e| e.name() == "p")
            {
                let context_text = next_p.text().collect::<String>().to_lowercase();
                return Some(parse_context_flags(&context_text));
            }
        }
    }
    None
}

fn parse_context_flags(text: &str) -> ContextAvailability {
    ContextAvailability {
        thick_client: text.contains("толстый клиент"),
        thin_client: text.contains("тонкий клиент"),
        web_client: text.contains("веб-клиент"),
        server: text.contains("сервер"),
        mobile_client: text.contains("мобильный клиент"),
        external_connection: text.contains("внешнее соединение")
            || text.contains("интеграция"),
    }
}
```

#### 3.2. `extract_parameters()` - Сложная вложенная логика

**Текущий подход:**
- Ручной поиск `<div class="V8SH_rubric">`
- Сложная логика границ поиска (600 символов)
- Проблемы с UTF-8 slicing

**С scraper:**
```rust
fn extract_parameters_scraper(html_content: &str) -> Vec<MethodParameter> {
    let html = Html::parse_fragment(html_content);
    let rubric_sel = Selector::parse("div.V8SH_rubric").unwrap();

    let mut parameters = Vec::new();

    for rubric in html.select(&rubric_sel) {
        let inner = rubric.inner_html();

        // Извлечь имя параметра: &lt;ИмяПараметра&gt;
        if let Some(param_name) = extract_param_name(&inner) {
            let is_optional = inner.contains("(необязательный)");

            // Тип может быть внутри или после </div>
            let param_type = extract_param_type(&rubric);

            parameters.push(MethodParameter {
                name: param_name,
                param_type,
                is_optional,
            });
        }
    }

    parameters
}

fn extract_param_type(element: &ElementRef) -> Option<String> {
    // Сначала ищем внутри текущего элемента
    let inner = element.inner_html();
    if let Some(type_pos) = inner.find("Тип: ") {
        let after_type = &inner[type_pos + "Тип: ".len()..];
        if let Some(br_pos) = after_type.find("<br>") {
            let type_text = &after_type[..br_pos];
            return Some(strip_html_tags(type_text).trim().to_string());
        }
    }

    // Если не нашли, ищем в следующем текстовом узле
    if let Some(next) = element.next_sibling() {
        if let Some(text) = next.value().as_text() {
            let content = text.text.to_string();
            if let Some(type_pos) = content.find("Тип: ") {
                // ... извлечение типа
            }
        }
    }

    None
}
```

#### 3.3. `extract_return_type()` - Аналогично

```rust
fn extract_return_type_scraper(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);
    let chapter_sel = Selector::parse("p.V8SH_chapter").unwrap();

    for chapter in html.select(&chapter_sel) {
        let text = chapter.text().collect::<String>();
        if text.contains("Возвращаемое значение") {
            // Взять содержимое после заголовка
            let mut content = String::new();
            let mut node = chapter.next_sibling();

            // Собрать текст до следующего chapter
            while let Some(n) = node {
                if let Some(elem) = n.value().as_element() {
                    if elem.name() == "p" && elem.attr("class") == Some("V8SH_chapter") {
                        break; // Следующая глава
                    }
                }

                // Собираем текст
                if let Some(text_node) = n.value().as_text() {
                    content.push_str(text_node.text);
                }

                node = n.next_sibling();
            }

            // Извлечь "Тип: XXX"
            if let Some(type_pos) = content.find("Тип: ") {
                let after = &content[type_pos + "Тип: ".len()..];
                let end = after.find('.').unwrap_or(after.len());
                return Some(after[..end].trim().to_string());
            }
        }
    }
    None
}
```

#### 3.4. `extract_version()` - Простой случай

```rust
fn extract_version_scraper(html_content: &str) -> Option<String> {
    let html = Html::parse_fragment(html_content);

    // Ищем в любом тексте "Доступен, начиная с версии X.Y"
    for text_node in html.root_element().descendants() {
        if let Some(text) = text_node.value().as_text() {
            if let Some(pos) = text.text.find("Доступен, начиная с версии ") {
                let after = &text.text[pos + "Доступен, начиная с версии ".len()..];
                if let Some(dot) = after.find('.') {
                    return Some(after[..dot].trim().to_string());
                }
            }
        }
    }
    None
}
```

### Этап 4: Тестирование и валидация (1 час)

**4.1. Создать unit тесты**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_context_with_integration() {
        let html = r#"
            <p class="V8SH_chapter">Доступность: </p>
            <p>Тонкий клиент, сервер, интеграция.</p>
        "#;

        let context = extract_context_scraper(html).unwrap();
        assert!(context.thin_client);
        assert!(context.server);
        assert!(context.external_connection); // интеграция
        assert!(!context.web_client);
    }

    #[test]
    fn test_extract_parameters_with_types() {
        let html = r#"
            <div class="V8SH_rubric">
                <p>&lt;Значение&gt; (обязательный)</p>
            </div>
            Тип: <a href="...">Число</a>. <br>
        "#;

        let params = extract_parameters_scraper(html);
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "Значение");
        assert_eq!(params[0].param_type, Some("Число".to_string()));
        assert!(!params[0].is_optional);
    }
}
```

**4.2. Сравнить результаты**

Запустить оба парсера на реальных данных и сравнить:
- Количество извлеченных методов
- Процент параметров с типами
- Процент методов с контекстом
- Качество извлеченных данных

**4.3. Бенчмарки производительности**

Проверить не ухудшилась ли производительность:
```bash
time ./html-parser old /path/to/help /tmp/old.json
time ./html-parser-scraper new /path/to/help /tmp/new.json
```

### Этап 5: Переход (30 мин)

**5.1. Заменить старый код**
- Удалить старые функции
- Переименовать `src/scraper_parser.rs` → интегрировать в `src/main.rs`

**5.2. Обновить документацию**
- Обновить README
- Добавить примеры использования scraper

**5.3. Финальное тестирование**
```bash
cargo test --release
cargo build --release
```

## Риски и митигация

### Риск 1: Производительность
**Митигация:**
- scraper основан на html5ever - очень быстрый парсер
- Если есть проблемы, можно кешировать parsed HTML

### Риск 2: Изменение результатов
**Митигация:**
- Параллельная разработка
- Сравнение результатов перед переходом
- Откат возможен через git

### Риск 3: Сложность CSS селекторов
**Митигация:**
- Начать с простых селекторов
- Использовать fallback на навигацию по DOM если нужно

## Оценка трудозатрат

| Этап | Время | Описание |
|------|-------|----------|
| 1. Подготовка | 30 мин | Зависимости, изучение API |
| 2. Создание структуры | 30 мин | Новый файл, константы |
| 3. Реализация функций | 3 часа | Переписать 4 основные функции |
| 4. Тестирование | 1 час | Unit тесты, сравнение |
| 5. Переход | 30 мин | Замена, документация |
| **ИТОГО** | **~6 часов** | Полная миграция |

## Критерии успеха

✅ Минимум 90% методов с контекстом (сейчас 99.95%)
✅ Минимум 90% параметров с типами (сейчас 91.8%)
✅ Минимум 60% методов с return types (сейчас 62.8%)
✅ Код проще и читабельнее
✅ Нет регрессий в производительности (±10%)
✅ Все тесты проходят

## Следующие шаги

1. ✅ Создать этот план
2. ✅ Обсудить с командой / получить approval
3. ✅ Начать Этап 1: Добавить scraper в Cargo.toml
4. ✅ Реализовать параллельную версию
5. ✅ Заменить старую реализацию на новую
6. ⏳ Сравнить результаты на реальных данных
7. ⏳ Измерить производительность

## Альтернативы

### Вариант A: Оставить как есть
- ✅ Уже работает
- ❌ Хрупкий код
- ❌ Сложно поддерживать

### Вариант B: Использовать другой парсер
- `html5ever` напрямую - более низкоуровневый
- `select` - менее популярный
- `kuchiki` - похож на scraper

**Вывод:** scraper - оптимальный выбор для нашей задачи.
