# Схема хранения

Этот документ фиксирует схему PostgreSQL на уровне основных сущностей. Здесь
разделены **реально используемые таблицы** и возможные будущие улучшения.

## Реально используемые таблицы

### `snapshots`

Хранит immutable snapshot'ы корпусов.

Ключевые поля:

- `id`
- `corpus`
- `fingerprint`
- `parent_snapshot_id`
- `branch`
- `commit_sha`
- `created_at`

### `snapshot_files`

Связывает логический путь внутри snapshot с file object.

Ключевые поля:

- `snapshot_id`
- `collection`
- `root_id`
- `path`
- `file_fingerprint`
- `document_count`
- `file_object_id`

Тождество файла — тройка `(collection, root_id, path)`. Расширение повторяет
раскладку конфигурации, поэтому один относительный путь под двумя корнями
называет два РАЗНЫХ файла; ключ без корня слил бы их в одну строку. Корень
конфигурации — пустая строка, поэтому строки, записанные до появления корней,
сохраняют смысл под составным ключом без переписывания.

### `file_objects`

Хранит дедуплицированное файловое содержимое на уровне объекта файла.

Ключевые поля:

- `id`
- `collection`
- `file_fingerprint`
- `document_count`

### `file_object_items`

Хранит структурированные item'ы одного file object.

Ключевые поля:

- `file_object_id`
- `ordinal`
- `symbol_name`
- `kind`
- `line_start`
- `line_end`
- `content_hash`

### `content_objects`

Дедуплицированное текстовое содержимое item/chunk.

Ключевые поля:

- `content_hash`
- `text`

### `semantic_embeddings`

Общее хранилище эмбеддингов.

Ключевые поля:

- `embedding_key`
- `model_id`
- `dimension`
- `embedding`

### `snapshot_deletions`

Фиксирует удаления путей относительно parent snapshot.

Ключевые поля:

- `snapshot_id`
- `collection`
- `root_id`
- `path`

Корень входит в ключ по той же причине, что и в `snapshot_files`, и здесь она
дороже: удаление, записанное без корня, погасило бы живой файл другого корня.

Эта таблица важна для корректного восстановления resolved view по цепочке delta
snapshot'ов.

## Характеристики текущей схемы

Схема уже сейчас оптимизирована под:

- immutable publication;
- reuse file objects;
- reuse текстовых payload'ов;
- reuse embeddings между snapshot'ами.

## Возможные будущие улучшения

### `snapshot_heads`

Отдельная таблица быстрых head-ссылок для веток может быть полезной как
дополнительный индекс выбора baseline, но её не стоит считать обязательной
частью текущего минимального design reference.
