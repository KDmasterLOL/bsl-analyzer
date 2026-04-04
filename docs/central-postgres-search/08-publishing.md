# Публикация

## Что делает publish pipeline

Публикация превращает локально собранный corpus в immutable snapshot и
записывает его в shared PostgreSQL storage.

## Базовые принципы

- публикация детерминирована для одинакового входа;
- snapshot после публикации не меняется;
- branch head обновляется только после успешной публикации;
- file objects и embeddings переиспользуются, если содержимое уже известно;
- старые snapshot'ы не переписываются «на месте».

## Что публикуется в первую очередь

Нормальные shared target'ы:

- `reference`;
- `workspace-code` для `vendor`;
- `workspace-code` для `develop`.

Feature-ветки обычно не являются обязательными publish-target'ами interactive runtime.

## Что нужно для публикации

Типичный publish использует:

- corpus id;
- source directory;
- branch и commit;
- optional parent snapshot;
- PostgreSQL URL/schema;
- optional embedder для генерации shared embeddings.

## Что получается на выходе

Успешная публикация создаёт:

- новую запись snapshot;
- связи snapshot -> files;
- новые или переиспользованные file objects;
- новые или переиспользованные content objects;
- новые или переиспользованные embeddings.

## Parent snapshot и delta-lineage

Parent snapshot нужен не для «изменения прошлого», а для lineage и эффективного
переиспользования данных.

Через него можно:

- не переписывать неизменившиеся файлы;
- корректно фиксировать deletions;
- восстанавливать resolved view по цепочке snapshot'ов.
