# Эксплуатация

## Базовая модель эксплуатации

Для shared search ожидается такой production-контур:

- один PostgreSQL instance или cluster для shared search;
- CI публикует snapshot'ы;
- разработчики читают baseline через ограниченные runtime credentials;
- retention и GC обслуживаются как отдельные штатные операции.

## Что нужно контролировать

Минимальный operational baseline:

- доступность PostgreSQL;
- latency lexical и semantic запросов;
- рост таблиц snapshot/file/embedding;
- reuse vs create статистику на publish;
- количество кандидатов на GC;
- состояние branch support policy.

## Retention

Retention должен работать на уровне snapshot'ов, а не «живых веток» как mutable state.

Ожидаемое направление:

- активные head-снимки не удаляются;
- `vendor` хранит ограниченное число последних heads;
- `develop` хранится по временному окну;
- `reference` хранит текущий и, при необходимости, предыдущий снимок.

## Garbage collection

GC должен удалять только unreachable-объекты:

- file objects;
- content objects;
- embeddings.

Важно:

- сначала dry-run;
- затем явное подтверждение destructive path;
- после GC не должны ломаться retained snapshot'ы и heads.

## Безопасность

Нормальная operational модель:

- разработчики получают read-only или близкие к тому права;
- публикация и очистка доступны только контролируемым pipeline/операторам;
- секреты для PostgreSQL и embedder'ов хранятся вне репозитория.
