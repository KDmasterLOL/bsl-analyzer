# ADR: Postgres-first runtime для централизованного поиска

## Статус

Принято и реализуется поэтапно.

## Контекст

Проект уже поддерживает:

- локальный SQLite runtime;
- публикацию baseline snapshot'ов в PostgreSQL;
- branch policy для выбора baseline;
- overlay для локальных изменений рабочей копии.

Нужно было зафиксировать, какой backend является каноническим для shared search:
просто публикационный источник или полноценный shared runtime.

## Решение

Shared baseline для централизованного поиска рассматривается как
**PostgreSQL-first backend**.

Это означает:

1. опубликованные shared snapshot'ы хранятся в PostgreSQL;
2. `reference` рассматривается как общий централизованный corpus;
3. baseline для `workspace-code` публикуются как immutable snapshot'ы;
4. локальная разработка на `feature/*`, `fix/*`, `bug/*` обычно работает как
   `shared baseline + local overlay`;
5. merge baseline и overlay выполняется на уровне приложения, а не через
   cross-database SQL.

## Почему не только SQLite

SQLite остаётся полезным:

- как полностью локальный режим;
- как fallback;
- как кэш локального runtime.

Но для shared search у PostgreSQL есть преимущества:

- общая точка публикации;
- единый retention/GC контур;
- естественная интеграция с `pgvector`;
- переиспользование snapshot'ов, file objects и embeddings между машинами.

## Почему ветки разработчиков не публикуются постоянно

Поведение «публиковать всё подряд» плохо масштабируется:

- возрастает сетевой и write-трафик;
- усложняется retention;
- растёт шум в shared storage;
- теряется смысл локального overlay как отражения незакоммиченных изменений.

Поэтому interactive runtime опирается на baseline, а developer delta держит у
себя локально.
